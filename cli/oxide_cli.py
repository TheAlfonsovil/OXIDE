#!/usr/bin/env python3
"""
OXIDE CLI (mainnet-beta focused).

Prereqs (suggested versions):
  pip install anchorpy==0.20.2 solana==0.30.2 solders==0.18.1

Env/flags (flags override env):
  --program / OXIDE_PROGRAM_ID       OXIDE program ID
  --hook-id / HOOK_PROGRAM_ID        Transfer hook program ID (for clear_debt/withdraw/deposit sync)
  --mint / OXIDE_MINT                Token-2022 mint address
  --rpc / RPC_URL                    RPC endpoint (default mainnet-beta)
  --keypair / KEYPAIR                Path to keypair json
  --idl / OXIDE_IDL                  Path to oxide IDL (defaults to target/idl/oxide.json)
  --hook-idl / HOOK_IDL              Path to hook IDL (defaults to target/idl/oxide_transfer_hook.json)

Commands (subcommands):
  balance                     Muestra balance interno (free/staked) y timestamp
  init-user                   Crea UserAccount PDA si no existe
  stake --amount <OXD>        Mueve de libre a stake
  unstake --amount <OXD>      Mueve de stake a libre (creador bloqueado por código)
  transfer --to <wallet> --amount <OXD>            Transferencia interna entre UserAccounts
  transfer-uninitialized --to <wallet> --amount <OXD>  Crea el destinatario si no existe
  clear-debt --token-account <ATA>                 Limpia deuda sobre SPL y resetea ventana
  deposit --amount <OXD> --token-account <ATA>     Quita SPL (burn) y acredita balance interno
  withdraw --amount <OXD> --token-account <ATA>    Mintea SPL y descuenta balance interno

Notas:
- Cantidades se expresan en OXD con 6 decimales (p.ej. 12.345678).
- Para clear_debt/deposit/withdraw necesitas el ATA Token-2022 del wallet.
- El programa asume seeds: Global=["global"], User=["user", owner], Tracking (hook)=["tracking", owner].
"""

import argparse
import json
import os
from decimal import Decimal, ROUND_DOWN
from pathlib import Path
from typing import Optional

import anchorpy
from anchorpy import Context, Program, Provider, Wallet
from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solana.rpc.async_api import AsyncClient

DECIMALS = 6
DECIMAL_SCALE = Decimal(10) ** DECIMALS
TOKEN_2022_PID = Pubkey.from_string("TokenzQdBnEtFd5Ki41mKq3d8bHPG8LML7jn1RybmZ8u")
DEFAULT_RPC = "https://api.mainnet-beta.solana.com"


def parse_amount(amount_str: str) -> int:
    amt = Decimal(amount_str)
    if amt < 0:
        raise ValueError("Amount must be non-negative")
    scaled = (amt * DECIMAL_SCALE).to_integral_value(rounding=ROUND_DOWN)
    return int(scaled)


def format_amount(amt: int) -> str:
    return str(Decimal(amt) / DECIMAL_SCALE)


def load_keypair(path: str) -> Keypair:
    data = json.loads(Path(path).read_text())
    return Keypair.from_bytes(bytes(data))


def env_or(flag: str, env: str, default: Optional[str] = None) -> Optional[str]:
    return flag if flag else os.getenv(env, default)


def derive_user(owner: Pubkey, program_id: Pubkey):
    return Pubkey.find_program_address([b"user", bytes(owner)], program_id)


def derive_global(program_id: Pubkey):
    return Pubkey.find_program_address([b"global"], program_id)


def derive_tracking(owner: Pubkey, hook_program: Pubkey):
    return Pubkey.find_program_address([b"tracking", bytes(owner)], hook_program)


async def load_program(rpc_url: str, kp: Keypair, program_id: Pubkey, idl_path: Optional[str]):
    client = AsyncClient(rpc_url, commitment="confirmed")
    wallet = Wallet(kp)
    provider = Provider(client, wallet)
    anchorpy.set_provider(provider)
    if idl_path and Path(idl_path).exists():
        idl = json.loads(Path(idl_path).read_text())
    else:
        idl = await Program.fetch_idl(program_id, provider)
        if idl is None:
            raise ValueError("Unable to fetch IDL; provide --idl")
    program = Program(idl, program_id, provider)
    return program, provider


async def cmd_balance(program: Program, owner: Pubkey):
    user_pda, _ = derive_user(owner, program.program_id)
    try:
        acc = await program.account["UserAccount"].fetch(user_pda)
    except Exception:
        print("UserAccount not initialized")
        return
    print(f"User PDA: {user_pda}")
    print(f"Owner:    {acc.owner}")
    print(f"Free:     {format_amount(acc.balance_free)} OXD")
    print(f"Staked:   {format_amount(acc.balance_staked)} OXD")
    print(f"Last ts:  {acc.last_update}")
    print(f"Remainder:{acc.burn_fraction_remainder}")


async def cmd_init_user(program: Program, owner: Keypair):
    user_pda, bump = derive_user(owner.pubkey(), program.program_id)
    ix = program.methods.initialize_user().accounts({
        "user": user_pda,
        "owner": owner.pubkey(),
        "system_program": anchorpy.SYS_PROGRAM_ID,
    }).instruction()
    tx = anchorpy.Transaction().add(ix)
    sig = await program.provider.send(tx, opts=anchorpy.TxOpts(skip_confirmation=False))
    print(f"UserAccount created: {user_pda}\nTx: {sig}")


async def cmd_stake(program: Program, owner: Keypair, amount: int):
    user_pda, _ = derive_user(owner.pubkey(), program.program_id)
    ix = program.methods.stake(amount).accounts({
        "user": user_pda,
        "owner": owner.pubkey(),
    }).instruction()
    tx = anchorpy.Transaction().add(ix)
    sig = await program.provider.send(tx)
    print(f"Staked {format_amount(amount)} OXD\nTx: {sig}")


async def cmd_unstake(program: Program, owner: Keypair, amount: int):
    user_pda, _ = derive_user(owner.pubkey(), program.program_id)
    global_pda, _ = derive_global(program.program_id)
    ix = program.methods.unstake(amount).accounts({
        "user": user_pda,
        "global_state": global_pda,
        "owner": owner.pubkey(),
    }).instruction()
    tx = anchorpy.Transaction().add(ix)
    sig = await program.provider.send(tx)
    print(f"Unstaked {format_amount(amount)} OXD\nTx: {sig}")


async def cmd_transfer(program: Program, owner: Keypair, to_wallet: Pubkey, amount: int):
    from_pda, _ = derive_user(owner.pubkey(), program.program_id)
    to_pda, _ = derive_user(to_wallet, program.program_id)
    global_pda, _ = derive_global(program.program_id)
    
    # Fetch global state para obtener authority real del creador
    global_acc = await program.account["GlobalState"].fetch(global_pda)
    creator_pda, _ = derive_user(global_acc.authority, program.program_id)

    ix = program.methods.transfer(amount).accounts({
        "from_user": from_pda,
        "to_user": to_pda,
        "global_state": global_pda,
        "creator_account": creator_pda,
        "owner": owner.pubkey(),
    }).instruction()
    tx = anchorpy.Transaction().add(ix)
    sig = await program.provider.send(tx)
    print(f"Transferred {format_amount(amount)} OXD\nTx: {sig}")


async def cmd_transfer_uninit(program: Program, owner: Keypair, to_wallet: Pubkey, amount: int):
    from_pda, _ = derive_user(owner.pubkey(), program.program_id)
    to_pda, _ = derive_user(to_wallet, program.program_id)
    ix = program.methods.transfer_to_uninitialized(amount).accounts({
        "from_user": from_pda,
        "to_user": to_pda,
        "to_owner": to_wallet,
        "owner": owner.pubkey(),
        "system_program": anchorpy.SYS_PROGRAM_ID,
    }).instruction()
    tx = anchorpy.Transaction().add(ix)
    sig = await program.provider.send(tx)
    print(f"Transferred {format_amount(amount)} OXD (init dest if needed)\nTx: {sig}")


async def cmd_clear_debt(program: Program, hook_program_id: Pubkey, owner: Keypair, mint: Pubkey, token_account: Pubkey):
    user_pda, _ = derive_user(owner.pubkey(), program.program_id)
    global_pda, _ = derive_global(program.program_id)
    tracking_pda, _ = derive_tracking(owner.pubkey(), hook_program_id)
    ix = program.methods.clear_debt().accounts({
        "user_account": user_pda,
        "user_token_account": token_account,
        "mint": mint,
        "global_state": global_pda,
        "tracking_account": tracking_pda,
        "hook_program": hook_program_id,
        "owner": owner.pubkey(),
        "token_program": TOKEN_2022_PID,
        "system_program": anchorpy.SYS_PROGRAM_ID,
    }).instruction()
    tx = anchorpy.Transaction().add(ix)
    sig = await program.provider.send(tx)
    print(f"clear_debt done\nTx: {sig}")


async def cmd_deposit(program: Program, hook_program_id: Pubkey, owner: Keypair, mint: Pubkey, token_account: Pubkey, amount: int):
    user_pda, _ = derive_user(owner.pubkey(), program.program_id)
    global_pda, _ = derive_global(program.program_id)
    tracking_pda, _ = derive_tracking(owner.pubkey(), hook_program_id)
    global_acc = await program.account["GlobalState"].fetch(global_pda)
    creator_pda, _ = derive_user(global_acc.authority, program.program_id)
    ix = program.methods.deposit(amount).accounts({
        "user_account": user_pda,
        "user_token_account": token_account,
        "mint": mint,
        "global_state": global_pda,
        "creator_account": creator_pda,
        "tracking_account": tracking_pda,
        "hook_program": hook_program_id,
        "owner": owner.pubkey(),
        "token_program": TOKEN_2022_PID,
        "system_program": anchorpy.SYS_PROGRAM_ID,
    }).instruction()
    tx = anchorpy.Transaction().add(ix)
    sig = await program.provider.send(tx)
    print(f"Deposited {format_amount(amount)} OXD (burn SPL)\nTx: {sig}")


async def cmd_withdraw(program: Program, hook_program_id: Pubkey, owner: Keypair, mint: Pubkey, token_account: Pubkey, amount: int):
    user_pda, _ = derive_user(owner.pubkey(), program.program_id)
    global_pda, _ = derive_global(program.program_id)
    tracking_pda, _ = derive_tracking(owner.pubkey(), hook_program_id)
    global_acc = await program.account["GlobalState"].fetch(global_pda)
    creator_pda, _ = derive_user(global_acc.authority, program.program_id)
    ix = program.methods.withdraw(amount).accounts({
        "user_account": user_pda,
        "user_token_account": token_account,
        "mint": mint,
        "global_state": global_pda,
        "creator_account": creator_pda,
        "tracking_account": tracking_pda,
        "hook_program": hook_program_id,
        "owner": owner.pubkey(),
        "token_program": TOKEN_2022_PID,
        "system_program": anchorpy.SYS_PROGRAM_ID,
    }).instruction()
    tx = anchorpy.Transaction().add(ix)
    sig = await program.provider.send(tx)
    print(f"Withdrew {format_amount(amount)} OXD (mint SPL)\nTx: {sig}")


def build_parser():
    p = argparse.ArgumentParser(description="OXIDE CLI")
    p.add_argument("--program", dest="program_id", help="OXIDE program id")
    p.add_argument("--hook-id", dest="hook_id", help="Hook program id")
    p.add_argument("--mint", dest="mint", help="Mint address")
    p.add_argument("--rpc", dest="rpc", default=None, help="RPC URL")
    p.add_argument("--keypair", dest="keypair", help="Keypair path")
    p.add_argument("--idl", dest="idl", help="IDL path for OXIDE")
    p.add_argument("--hook-idl", dest="hook_idl", help="IDL path for hook (unused today)")

    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("balance")
    sub.add_parser("init-user")

    stake = sub.add_parser("stake")
    stake.add_argument("--amount", required=True)

    unstake = sub.add_parser("unstake")
    unstake.add_argument("--amount", required=True)

    tx = sub.add_parser("transfer")
    tx.add_argument("--to", required=True)
    tx.add_argument("--amount", required=True)

    txu = sub.add_parser("transfer-uninitialized")
    txu.add_argument("--to", required=True)
    txu.add_argument("--amount", required=True)

    cd = sub.add_parser("clear-debt")
    cd.add_argument("--token-account", required=True)

    dep = sub.add_parser("deposit")
    dep.add_argument("--amount", required=True)
    dep.add_argument("--token-account", required=True)

    wd = sub.add_parser("withdraw")
    wd.add_argument("--amount", required=True)
    wd.add_argument("--token-account", required=True)

    return p


async def main():
    parser = build_parser()
    args = parser.parse_args()

    program_id = Pubkey.from_string(env_or(args.program_id, "OXIDE_PROGRAM_ID"))
    hook_id = Pubkey.from_string(env_or(args.hook_id, "HOOK_PROGRAM_ID", str(program_id)))
    mint = Pubkey.from_string(env_or(args.mint, "OXIDE_MINT"))
    rpc_url = env_or(args.rpc, "RPC_URL", DEFAULT_RPC)
    keypair_path = env_or(args.keypair, "KEYPAIR")
    if not keypair_path:
        raise SystemExit("--keypair or KEYPAIR env is required")

    kp = load_keypair(keypair_path)
    program, provider = await load_program(rpc_url, kp, program_id, env_or(args.idl, "OXIDE_IDL", "target/idl/oxide.json"))

    cmd = args.cmd
    if cmd == "balance":
        await cmd_balance(program, kp.pubkey())
    elif cmd == "init-user":
        await cmd_init_user(program, kp)
    elif cmd == "stake":
        amt = parse_amount(args.amount)
        await cmd_stake(program, kp, amt)
    elif cmd == "unstake":
        amt = parse_amount(args.amount)
        await cmd_unstake(program, kp, amt)
    elif cmd == "transfer":
        amt = parse_amount(args.amount)
        to_wallet = Pubkey.from_string(args.to)
        await cmd_transfer(program, kp, to_wallet, amt)
    elif cmd == "transfer-uninitialized":
        amt = parse_amount(args.amount)
        to_wallet = Pubkey.from_string(args.to)
        await cmd_transfer_uninit(program, kp, to_wallet, amt)
    elif cmd == "clear-debt":
        token_account = Pubkey.from_string(args.token_account)
        await cmd_clear_debt(program, hook_id, kp, mint, token_account)
    elif cmd == "deposit":
        amt = parse_amount(args.amount)
        token_account = Pubkey.from_string(args.token_account)
        await cmd_deposit(program, hook_id, kp, mint, token_account, amt)
    elif cmd == "withdraw":
        amt = parse_amount(args.amount)
        token_account = Pubkey.from_string(args.token_account)
        await cmd_withdraw(program, hook_id, kp, mint, token_account, amt)
    else:
        parser.error("Unknown command")

    await provider.connection.close()


if __name__ == "__main__":
    import asyncio
    asyncio.run(main())
