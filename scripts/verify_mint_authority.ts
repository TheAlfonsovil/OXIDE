import * as anchor from '@coral-xyz/anchor';
import { Program } from '@coral-xyz/anchor';
import { PublicKey } from '@solana/web3.js';
import { buildProvider, getArg, loadIdl, loadKeypair, requireArg, toPublicKey } from './helpers';

async function main() {
  const programIdStr = requireArg('--program', 'PROGRAM_ID env or --program <pubkey>');
  const mintStr = requireArg('--mint', 'MINT_ADDRESS env or --mint <pubkey>');
  const keypairPath = requireArg('--keypair', 'KEYPAIR_PATH env or --keypair <path>');
  const rpcUrl = getArg('--rpc') || 'https://api.mainnet-beta.solana.com';
  const idlPath = getArg('--idl') || 'target/idl/oxide.json';

  const programId = toPublicKey(programIdStr, 'program id');
  const mint = toPublicKey(mintStr, 'mint address');
  const wallet = new anchor.Wallet(loadKeypair(keypairPath));
  const provider = buildProvider(rpcUrl, wallet);
  const idl = await loadIdl(idlPath, programId, provider);
  const program = new Program(idl, programId, provider);

  const [globalPda] = PublicKey.findProgramAddressSync([Buffer.from('global')], programId);

  console.log('RPC:', rpcUrl);
  console.log('Program:', programId.toBase58());
  console.log('Mint:', mint.toBase58());
  console.log('Authority:', wallet.publicKey.toBase58());
  console.log('Global PDA:', globalPda.toBase58());

  await program.methods
    .verifyMintAuthority()
    .accounts({
      globalState: globalPda,
      mint,
      authority: wallet.publicKey
    })
    .rpc();

  console.log('verify_mint_authority ok');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
