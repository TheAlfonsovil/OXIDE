#!/usr/bin/env pwsh
# ================================================================
# OXIDE Deployment Script - Arquitectura Completa con Zona Franca
#
# DESCRIPCION:
#   - Dos programas: OXIDE (principal) + oxide_transfer_hook (Token-2022 Hook)
#   - Supply REAL = interno (UserAccounts)
#   - SPL Token = "recibos" (se mintean SOLO via withdraw)
#   - Transfer Hook rastrea deuda, bloquea >15min, permite Zona Franca
#   - Whitelist: Raydium V4, Orca Whirlpool, Meteora DLMM
#   - Empieza con 0 SPL tokens en circulacion
#   - Creador tiene 1T interno en staking
# ================================================================

param(
    [string]$Network = "devnet",  # devnet, testnet, o mainnet-beta
    [string]$KeypairPath = ""     # Ruta opcional a keypair para mainnet/testnet
)

$ErrorActionPreference = "Stop"
$ScriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$SourceOxide = Join-Path $ScriptRoot "lib.rs"
$SourceHook = Join-Path $ScriptRoot "hook_lib.rs"

# Abort if workspace already exists with keypairs to avoid regenerating Program IDs
$existingWorkspace = Test-Path (Join-Path $ScriptRoot "OXIDE")
$existingKeypairs = @(
    Join-Path $ScriptRoot "OXIDE/target/deploy/OXIDE-keypair.json",
    Join-Path $ScriptRoot "OXIDE/target/deploy/oxide_transfer_hook-keypair.json"
)
if ($existingWorkspace) {
    $missingKeys = $existingKeypairs | Where-Object { -not (Test-Path $_) }
    if ($missingKeys.Count -gt 0) {
        Write-Host "[X] Workspace OXIDE existe pero faltan keypairs en target/deploy. Regenerarlos cambiaria los Program IDs. Aborta." -ForegroundColor Red
        exit 1
    }
    Write-Host "[X] Workspace OXIDE ya existe con keypairs fijos. No se recrea para evitar Program IDs nuevos. Borra manualmente solo si quieres IDs nuevos." -ForegroundColor Red
    exit 1
}

Write-Host "`n" -NoNewline
Write-Host "="*70 -ForegroundColor Cyan
Write-Host "  OXIDE Deployment - Deflacionario Autonomo con Zona Franca" -ForegroundColor Cyan
Write-Host "="*70 -ForegroundColor Cyan
Write-Host ""
Write-Host "  Network: $Network" -ForegroundColor Yellow
Write-Host "  Supply interno: 1,000,000,000,000 OXD (1T en staking)" -ForegroundColor Yellow
Write-Host "  SPL inicial: 0 (se mintea via withdraw)" -ForegroundColor Yellow
Write-Host "  Transfer Hook: Bloquea >15min, permite Zona Franca" -ForegroundColor Yellow
Write-Host "  Whitelist: Raydium V4, Orca Whirlpool, Meteora DLMM" -ForegroundColor Yellow
Write-Host ""

# ================================================================
# PASO 1: Verificar herramientas
# ================================================================
Write-Host "[1/9] Verificando herramientas necesarias..." -ForegroundColor Green

function Test-Command {
    param($Command)
    try {
        if (Get-Command $Command -ErrorAction Stop) { return $true }
    } catch { return $false }
}

$tools = @{
    "rustc" = "https://rustup.rs/"
    "anchor" = "cargo install --git https://github.com/coral-xyz/anchor avm --locked"
    "solana" = "https://docs.solana.com/cli/install-solana-cli-tools"
}

foreach ($tool in $tools.Keys) {
    if (-not (Test-Command $tool)) {
        Write-Host "[X] $tool no esta instalado" -ForegroundColor Red
        Write-Host "   Instalar: $($tools[$tool])" -ForegroundColor Yellow
        exit 1
    }
}

Write-Host "[OK] Herramientas verificadas`n" -ForegroundColor Green

# ================================================================
# PASO 2: Crear proyecto Anchor
# ================================================================
Write-Host "[2/9] Creando proyecto Anchor dual..." -ForegroundColor Green

# Crear workspace Anchor
anchor init OXIDE
Set-Location OXIDE

# Copiar fuentes del programa principal si existen en el repo raiz
if (Test-Path $SourceOxide) {
    Copy-Item -Path $SourceOxide -Destination "programs/OXIDE/src/lib.rs" -Force
}

# Crear programa oxide_transfer_hook a partir del template de OXIDE
$hookDir = "programs/oxide_transfer_hook"
if (-not (Test-Path $hookDir)) {
    New-Item -ItemType Directory -Path $hookDir | Out-Null
    New-Item -ItemType Directory -Path "$hookDir/src" | Out-Null
}

# Clonar Cargo.toml de OXIDE y renombrar crate
$baseCargoPath = "programs/OXIDE/Cargo.toml"
if (Test-Path $baseCargoPath) {
    $cargoContent = Get-Content $baseCargoPath -Raw
    $cargoContent = $cargoContent -replace 'name\s*=\s*"[^"]+"', 'name = "oxide_transfer_hook"'
    $cargoContent = $cargoContent -replace 'lib\s*name\s*=\s*"[^"]+"', 'name = "oxide_transfer_hook"'
    Set-Content "$hookDir/Cargo.toml" $cargoContent
}

# Copiar fuente del hook
if (Test-Path $SourceHook) {
    Copy-Item -Path $SourceHook -Destination "$hookDir/src/lib.rs" -Force
}

# Asegurar que Anchor.toml conozca ambos programas (placeholder IDs)
$anchorTomlPath = "Anchor.toml"
if (Test-Path $anchorTomlPath) {
    $anchorToml = Get-Content $anchorTomlPath -Raw
    if ($anchorToml -notmatch 'name = "oxide_transfer_hook"') {
        $anchorToml += "`n[[programs]]`nname = \"oxide_transfer_hook\"`naddress = \"HookxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxXXX\"`n"
    }
    Set-Content $anchorTomlPath $anchorToml
}

Write-Host "[3/9] Compilando programas (primera vez)..." -ForegroundColor Green

anchor build

if ($LASTEXITCODE -ne 0) {
    Write-Host "[X] Error de compilacion" -ForegroundColor Red
    exit 1
}

Write-Host "[OK] Compilacion exitosa`n" -ForegroundColor Green

# ================================================================
# PASO 4: Obtener Program IDs
# ================================================================
Write-Host "[4/9] Extrayendo Program IDs..." -ForegroundColor Green

$keysOutput = anchor keys list
Write-Host $keysOutput -ForegroundColor Cyan

# Extraer IDs (formato: program_name: ADDRESS)
$programId = ($keysOutput | Select-String "OXIDE:" | Select-Object -First 1).ToString()
$hookId = ($keysOutput | Select-String "oxide_transfer_hook:").ToString()

if ([string]::IsNullOrEmpty($programId)) {
    Write-Host "[X] No se pudieron extraer Program IDs" -ForegroundColor Red
    exit 1
}

$programId = $programId -replace "OXIDE:\s*", ""
$hookId = $hookId -replace "oxide_transfer_hook:\s*", ""

Write-Host ""
Write-Host "Program ID (OXIDE): $programId" -ForegroundColor Cyan
Write-Host "Program ID (hook): $hookId" -ForegroundColor Cyan
Write-Host ""

# ================================================================
# PASO 5: Actualizar declare_id! y Anchor.toml
# ================================================================
Write-Host "[5/9] Actualizando declare_id! en codigo..." -ForegroundColor Green

# Actualizar declare_id en lib.rs
$libRsPath = "programs/OXIDE/src/lib.rs"
$libRsContent = Get-Content $libRsPath -Raw
$libRsContent = $libRsContent -replace 'declare_id!\("OXIDExxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"\);', "declare_id!(""$programId"");"
Set-Content $libRsPath $libRsContent
Write-Host "   [OK] lib.rs actualizado"

# Actualizar declare_id en hook_lib.rs
$hookRsPath = "programs/oxide_transfer_hook/src/lib.rs"
if (Test-Path $hookRsPath) {
    $hookRsContent = Get-Content $hookRsPath -Raw
    $hookRsContent = $hookRsContent -replace 'declare_id!\("HookxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxXXX"\);', "declare_id!(""$hookId"");"
    Set-Content $hookRsPath $hookRsContent
    Write-Host "   [OK] hook_lib.rs actualizado"
}

# Actualizar Anchor.toml
$anchorToml = Get-Content "Anchor.toml" -Raw
$anchorToml = $anchorToml -replace 'name = "OXIDE"\s+address = "[^"]*"', "name = ""OXIDE""`n        address = ""$programId"""
$anchorToml = $anchorToml -replace 'name = "oxide_transfer_hook"\s+address = "[^"]*"', "name = ""oxide_transfer_hook""`n        address = ""$hookId"""
Set-Content "Anchor.toml" $anchorToml
Write-Host "   [OK] Anchor.toml actualizado`n"

# ================================================================
# PASO 6: Recompilar
# ================================================================
Write-Host "[6/9] Recompilando con Program IDs correctos..." -ForegroundColor Green

anchor build

if ($LASTEXITCODE -ne 0) {
    Write-Host "[X] Error de compilacion" -ForegroundColor Red
    exit 1
}

Write-Host "[OK] Recompilacion exitosa`n" -ForegroundColor Green

# ================================================================
# PASO 7: Configurar Solana y wallet
# ================================================================
Write-Host "[7/9] Configurando Solana y wallet..." -ForegroundColor Green

solana config set --url $Network

if ($KeypairPath) {
    if (Test-Path $KeypairPath) {
        solana config set --keypair $KeypairPath
    } else {
        Write-Host "[!] Keypair no encontrado en $KeypairPath" -ForegroundColor Yellow
    }
}

$walletPubkey = solana address 2>$null
if ([string]::IsNullOrEmpty($walletPubkey)) {
    Write-Host "[X] No se pudo obtener direccion de wallet" -ForegroundColor Red
    exit 1
}

Write-Host "Wallet: $walletPubkey" -ForegroundColor Cyan

$balance = solana balance 2>$null
Write-Host "Balance: $balance" -ForegroundColor Cyan

if ($Network -eq "devnet") {
    if ($balance -match "^0 SOL" -or $null -eq $balance) {
        Write-Host "[!] Solicitando airdrop de devnet..." -ForegroundColor Yellow
        solana airdrop 2
        Start-Sleep -Seconds 5
        $balance = solana balance
        Write-Host "Nuevo balance: $balance" -ForegroundColor Cyan
    }
} else {
    if ($balance -match "^0 SOL" -or $null -eq $balance) {
        Write-Host "[!] Wallet sin fondos. Para mainnet/testnet necesitas SOL real para fees y rent." -ForegroundColor Yellow
        Write-Host "    Usa una keypair financiada y reejecuta." -ForegroundColor Yellow
    }
}

Write-Host ""

# ================================================================
# PASO 8: Crear SPL Token
# ================================================================
Write-Host "[8/9] Deployando programas on-chain (usa keypairs fijos en target/deploy)..." -ForegroundColor Green

anchor deploy --provider.cluster $Network

if ($LASTEXITCODE -ne 0) {
    Write-Host "[X] Deploy fallo. No se creara el mint." -ForegroundColor Red
    exit 1
}

Write-Host "[OK] Deploy exitoso. Program IDs confirmados on-chain." -ForegroundColor Green

# Intentar calcular PDA global solo despues de deploy
$globalPda = ""
try {
    $pdaCmd = "node -e \"const {PublicKey}=require('@solana/web3.js'); const p=new PublicKey('$programId'); const [g]=PublicKey.findProgramAddressSync([Buffer.from('global')],p); console.log(g.toBase58());\""
    $globalPda = Invoke-Expression $pdaCmd
    Write-Host "Global PDA: $globalPda" -ForegroundColor Cyan
} catch {
    Write-Host "[!] No se pudo calcular GLOBAL_PDA (node/web3 no disponible). Calculalo luego con pnpm run pda:global." -ForegroundColor Yellow
}

Write-Host "[9/9] Creando SPL Token OXIDE (Token-2022) despues de deploy..." -ForegroundColor Green

# Crear mint Token-2022 SIN mintear nada
$mintOutput = spl-token create-token --token-program TokenzQdBnEtFd5Ki41mKq3d8bHPG8LML7jn1RybmZ8u --decimals 6 2>&1
$mintAddress = ($mintOutput | Select-String -Pattern "[1-9A-HJ-NP-Z]{32,44}" | Select-Object -First 1).ToString().Trim()

Write-Host "Mint Address: $mintAddress" -ForegroundColor Cyan

if ([string]::IsNullOrEmpty($mintAddress)) {
    Write-Host "[!] No se pudo crear el mint (verificar conexion)" -ForegroundColor Yellow
} else {
    # Crear token account para el creador
    Write-Host "Creando token account..." -ForegroundColor Yellow
    spl-token create-account $mintAddress
    Write-Host "[OK] Token account creado" -ForegroundColor Green
}

Write-Host ""

# ================================================================
# RESUMEN FINAL
# ================================================================
Write-Host "="*70 -ForegroundColor Green
Write-Host "  SETUP + DEPLOY COMPLETADOS" -ForegroundColor Green
Write-Host "="*70 -ForegroundColor Green

Write-Host @"

    INFO CLAVE (guardar):
        - Program OXIDE:           $programId
        - Program oxide_transfer_hook: $hookId
        - Mint (Token-2022):       $mintAddress
        - Network:                 $Network
        - Global PDA:              $globalPda

    SECUENCIA DE INICIALIZACION (post-deploy):
        A) spl-token authorize $mintAddress mint <GLOBAL_PDA>
        B) spl-token initialize-mint-extension $mintAddress transferHook $hookId
        C) initialize_global (program OXIDE) apuntando a mint y authority
        D) verify_mint_authority (critico) para habilitar withdraw
        E) initialize_extra_account_meta_list (program hook) con programId OXIDE
        F) Tracking inicial se crea en primer withdraw/deposit

    ESTADO ESPERADO INICIAL:
        - Creator interno: 1,000,000,000,000 OXD staked
        - Circulante SPL:  0 OXD
        - Mint authority:  GlobalState PDA (transferir en paso A)
        - Transfer hook:   $hookId

"@ -ForegroundColor White

# Guardar configuracion
$config = @{
    programId = $programId
    hookId = $hookId
    mintAddress = $mintAddress
    network = $Network
    createdAt = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
} | ConvertTo-Json

Set-Content "oxide_config.json" $config
Write-Host "[OK] Configuracion guardada en oxide_config.json`n" -ForegroundColor Cyan

Write-Host "="*70 -ForegroundColor Green
Write-Host "  Proximo paso: Ejecutar secuencia [PASO A-G] en orden" -ForegroundColor Yellow
Write-Host "="*70 -ForegroundColor Green
Write-Host ""
