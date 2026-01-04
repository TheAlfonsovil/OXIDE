#!/usr/bin/env bash
# ================================================================
# OXIDE Deployment Script (Linux/WSL)
# - Crea workspace Anchor con dos programas: OXIDE + oxide_transfer_hook
# - Copia lib.rs / hook_lib.rs desde el repo raíz
# - Fija declare_id y Anchor.toml con los Program IDs generados
# - Compila y despliega a la red indicada (devnet por defecto)
# Nota: Replica el flujo del antiguo create_OXIDE.ps1 pero en bash.
# ================================================================
set -euo pipefail

NETWORK="devnet"
KEYPAIR=""
SCRIPT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$SCRIPT_ROOT/OXIDE"

usage() {
  echo "Usage: $(basename "$0") [-n devnet|testnet|mainnet-beta] [-k /path/to/keypair.json]"
  exit 1
}

while getopts "n:k:h" opt; do
  case "$opt" in
    n) NETWORK="$OPTARG" ;;
    k) KEYPAIR="$OPTARG" ;;
    h) usage ;;
    *) usage ;;
  esac
done

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "[X] Missing dependency: $cmd" >&2
    exit 1
  fi
}

echo "[1/9] Verificando herramientas..."
for c in rustc anchor solana spl-token node pnpm; do
  require_cmd "$c"
  case "$c" in
    rustc) rustc --version ;;
    anchor) anchor --version ;;
    solana) solana --version ;;
    spl-token) spl-token --version ;;
    pnpm) pnpm --version ;;
    node) node --version ;;
  esac
done

echo "[2/9] Creando workspace Anchor..."
if [ -d "$WORKSPACE_DIR" ]; then
  if [ ! -f "$WORKSPACE_DIR/target/deploy/OXIDE-keypair.json" ] || [ ! -f "$WORKSPACE_DIR/target/deploy/oxide_transfer_hook-keypair.json" ]; then
    echo "[X] Workspace OXIDE ya existe pero faltan keypairs en target/deploy; borrar o restaurar antes de continuar." >&2
    exit 1
  fi
  echo "[X] Workspace OXIDE ya existe con keypairs fijos. No se recrea para no cambiar Program IDs." >&2
  exit 1
fi

anchor init OXIDE
cd "$WORKSPACE_DIR"

# Copiar fuentes
if [ -f "$SCRIPT_ROOT/lib.rs" ]; then
  cp "$SCRIPT_ROOT/lib.rs" programs/OXIDE/src/lib.rs
fi

HOOK_DIR="programs/oxide_transfer_hook"
mkdir -p "$HOOK_DIR/src"
if [ -f "$SCRIPT_ROOT/hook_lib.rs" ]; then
  cp "$SCRIPT_ROOT/hook_lib.rs" "$HOOK_DIR/src/lib.rs"
fi

# Clonar Cargo.toml y renombrar crate del hook
if [ -f "programs/OXIDE/Cargo.toml" ]; then
  perl -pe 's/name\s*=\s*"[^"]+"/name = "oxide_transfer_hook"/ if $. == 1 .. /^\[/ && /package/; s/name\s*=\s*"[^"]+"/name = "oxide_transfer_hook"/ if /^name\s*=\s*"OXIDE"/' programs/OXIDE/Cargo.toml > "$HOOK_DIR/Cargo.toml"
fi

# Compilar primera vez
anchor build

# Obtener Program IDs
PROGRAM_ID=$(anchor keys list | awk '/OXIDE:/ {print $2; exit}')
HOOK_ID=$(anchor keys list | awk '/oxide_transfer_hook:/ {print $2; exit}')
if [ -z "$PROGRAM_ID" ] || [ -z "$HOOK_ID" ]; then
  echo "[X] No se pudieron extraer Program IDs" >&2
  exit 1
fi

echo "[3/9] Program IDs: OXIDE=$PROGRAM_ID hook=$HOOK_ID"

# Actualizar declare_id en lib.rs y hook
perl -0777 -pe "s/declare_id!\(\"[^\"]+\"\);/declare_id!(\"$PROGRAM_ID\");/" -i programs/OXIDE/src/lib.rs
perl -0777 -pe "s/declare_id!\(\"[^\"]+\"\);/declare_id!(\"$HOOK_ID\");/" -i "$HOOK_DIR/src/lib.rs"

# Actualizar Anchor.toml con Program IDs para localnet/devnet/mainnet-beta
python - <<PY
import re
from pathlib import Path

path = Path('Anchor.toml')
text = path.read_text()

def set_addr(block, name, addr):
  # Matches either TOML inline entries or name/address pairs
  block = re.sub(rf'{re.escape(name)}\s*=\s*"[^"]+"', f'{name} = "{addr}"', block)
  block = re.sub(rf'name\s*=\s*"{re.escape(name)}"\s*\n\s*address\s*=\s*"[^"]+"', f'name = "{name}"\naddress = "{addr}"', block)
  return block

def ensure_section(text, section):
  if f'[{section}]' not in text:
    text += f'\n[{section}]\n'
  return text

for section in ("programs.localnet", "programs.devnet", "programs.mainnet-beta"):
  text = ensure_section(text, section)
  section_re = rf'(\[{section}\][^\[]*)'
  def repl(m):
    block = m.group(1)
    block = set_addr(block, 'OXIDE', "$PROGRAM_ID")
    block = set_addr(block, 'oxide_transfer_hook', "$HOOK_ID")
    return block
  text = re.sub(section_re, repl, text, flags=re.DOTALL)

path.write_text(text)
PY

echo "[4/9] Recompilando con IDs fijos..."
anchor build

echo "[5/9] Configurando Solana CLI..."
solana config set --url "$NETWORK"
if [ -n "$KEYPAIR" ]; then
  solana config set --keypair "$KEYPAIR"
fi
solana address
solana balance || true

echo "[6/9] Deploying programas..."
anchor deploy --provider.cluster "$NETWORK"

GLOBAL_PDA=$(node -e "const {PublicKey}=require('@solana/web3.js');const p=new PublicKey('$PROGRAM_ID');const [g]=PublicKey.findProgramAddressSync([Buffer.from('global')],p);console.log(g.toBase58());")

echo "[7/9] PDA global: $GLOBAL_PDA"

echo "[8/9] Guarda configuración en oxide_config.json"
cat > oxide_config.json <<EOF
{
  "programId": "$PROGRAM_ID",
  "hookId": "$HOOK_ID",
  "network": "$NETWORK",
  "globalPda": "$GLOBAL_PDA",
  "createdAt": "$(date -Iseconds)"
}
EOF

echo "[9/9] Hecho. Sigue el runbook en lanzamiento.txt para mint, transferHook, initialize_global y verify_mint_authority." 
