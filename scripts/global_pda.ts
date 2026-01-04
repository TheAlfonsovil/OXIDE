import { PublicKey } from '@solana/web3.js';
import { getArg, requireArg } from './helpers';

async function main() {
  const programIdStr = requireArg('--program', 'PROGRAM_ID env or --program <pubkey>');
  const programId = new PublicKey(programIdStr);
  const [globalPda] = PublicKey.findProgramAddressSync([Buffer.from('global')], programId);
  console.log(globalPda.toBase58());
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
