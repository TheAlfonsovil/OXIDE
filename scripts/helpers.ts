import fs from 'fs';
import path from 'path';
import * as anchor from '@coral-xyz/anchor';
import { Idl, Program } from '@coral-xyz/anchor';
import { PublicKey, Keypair } from '@solana/web3.js';

export function getArg(flag: string): string | undefined {
  const idx = process.argv.indexOf(flag);
  if (idx !== -1 && idx + 1 < process.argv.length) {
    return process.argv[idx + 1];
  }
  return process.env[flagToEnv(flag)];
}

export function requireArg(flag: string, hint?: string): string {
  const value = getArg(flag);
  if (!value) {
    throw new Error(`Missing required ${flag}${hint ? ` (${hint})` : ''}`);
  }
  return value;
}

export function flagToEnv(flag: string): string {
  return flag.replace(/^--/, '').replace(/-/g, '_').toUpperCase();
}

export function loadKeypair(keypairPath: string): Keypair {
  const resolved = path.resolve(keypairPath);
  const raw = fs.readFileSync(resolved, 'utf8').trim();
  // Try JSON array first
  try {
    const arr = JSON.parse(raw);
    if (Array.isArray(arr)) {
      return Keypair.fromSecretKey(Uint8Array.from(arr));
    }
  } catch (_) {
    // not JSON, continue
  }
  const nums = raw
    .split(',')
    .map((s) => Number(s.trim()))
    .filter((n) => Number.isFinite(n));
  if (nums.length >= 64) {
    return Keypair.fromSecretKey(Uint8Array.from(nums));
  }
  throw new Error(`Invalid keypair file at ${resolved}`);
}

export async function loadIdl(
  idlPath: string | undefined,
  programId: PublicKey,
  provider: anchor.AnchorProvider
): Promise<Idl> {
  if (idlPath) {
    const resolved = path.resolve(idlPath);
    if (fs.existsSync(resolved)) {
      const content = fs.readFileSync(resolved, 'utf8');
      return JSON.parse(content);
    }
    console.warn(`Local IDL not found at ${resolved}, will try on-chain fetch.`);
  }
  const fetched = await Program.fetchIdl(programId, provider);
  if (!fetched) {
    throw new Error(`Unable to fetch IDL for program ${programId.toBase58()}`);
  }
  return fetched;
}

export function buildProvider(rpcUrl: string, wallet: anchor.Wallet): anchor.AnchorProvider {
  const connection = new anchor.web3.Connection(rpcUrl, 'confirmed');
  const provider = new anchor.AnchorProvider(connection, wallet, {});
  anchor.setProvider(provider);
  return provider;
}

export function toPublicKey(value: string, label: string): PublicKey {
  try {
    return new PublicKey(value);
  } catch (err) {
    throw new Error(`Invalid ${label}: ${value}`);
  }
}
