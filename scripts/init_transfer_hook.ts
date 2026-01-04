import * as anchor from '@coral-xyz/anchor';
import { Program } from '@coral-xyz/anchor';
import { PublicKey } from '@solana/web3.js';
import { buildProvider, getArg, loadIdl, loadKeypair, requireArg, toPublicKey } from './helpers';

async function main() {
  const hookIdStr = requireArg('--hook-id', 'HOOK_ID env or --hook-id <pubkey>');
  const programIdStr = requireArg('--program', 'PROGRAM_ID env or --program <pubkey>');
  const mintStr = requireArg('--mint', 'MINT_ADDRESS env or --mint <pubkey>');
  const keypairPath = requireArg('--keypair', 'KEYPAIR_PATH env or --keypair <path>');
  const rpcUrl = getArg('--rpc') || 'https://api.mainnet-beta.solana.com';
  const idlPath = getArg('--idl') || 'target/idl/oxide_transfer_hook.json';

  const hookId = toPublicKey(hookIdStr, 'hook program id');
  const programId = toPublicKey(programIdStr, 'program id');
  const mint = toPublicKey(mintStr, 'mint address');

  const wallet = new anchor.Wallet(loadKeypair(keypairPath));
  const provider = buildProvider(rpcUrl, wallet);
  const idl = await loadIdl(idlPath, hookId, provider);
  const program = new Program(idl, hookId, provider);

  const [extraAccountMetaList] = PublicKey.findProgramAddressSync(
    [Buffer.from('extra-account-metas'), mint.toBuffer()],
    hookId
  );

  console.log('RPC:', rpcUrl);
  console.log('Hook Program:', hookId.toBase58());
  console.log('Main Program:', programId.toBase58());
  console.log('Mint:', mint.toBase58());
  console.log('Authority:', wallet.publicKey.toBase58());
  console.log('ExtraAccountMetaList PDA:', extraAccountMetaList.toBase58());

  await program.methods
    .initializeExtraAccountMetaList(programId)
    .accounts({
      payer: wallet.publicKey,
      extraAccountMetaList,
      mint,
      systemProgram: anchor.web3.SystemProgram.programId
    })
    .rpc();

  console.log('initialize_extra_account_meta_list ok');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
