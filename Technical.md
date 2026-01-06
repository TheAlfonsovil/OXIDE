# OXIDE: Time-Decay Monetary Protocol (Solana Token-2022)

## Resumen ejecutivo
- Activo algorítmico con deflación **exponencial** de 20% anual aplicada solo al **balance libre** (no staked): $\text{remaining}=\text{balance}\_{free}\times(0.8)^{\tfrac{t}{\text{año}}}$ (decay continuo compuesto).
- **0% decay en stake**: mover a `balance_staked` congela la oxidación; retirarlo devuelve exposición al reloj.
- **Ventana de gracia de 15 minutos** (UX, no parámetro económico): permite transferir/operar sin limpiar deuda; pasado ese tiempo debe llamarse `clear_debt()` o esperar a la siguiente operación que sincronice.
- **Transfer Hook (Token-2022)** mantiene tracking por wallet (PDA `TrackingAccount`) y evita evasión de burn; los compradores desde pools entran "limpios" (timestamp reseteado a `now`).
- **Whitelist inmutable** para pools (Raydium V4, Orca Whirlpool, Meteora DLMM): zonas de liquidez sin acumulación de deuda.
- **Vesting dinámico anti-rugpull**: el creador solo libera 0.1% del volumen operado (`release_rate_basis_points=10`) y tiene bloqueado el `unstake`; no puede liberar manualmente.
- **Genesis Airdrops (EXCEPCIÓN CONTROLADA)**: Único método donde el creador puede liberar tokens manualmente, con límites estrictos: máx. 100 OXD por airdrop, máx. 1000 airdrops lifetime. **NO respeta cap diario** (diseñado para siembra inicial rápida a early adopters). Total máximo: 100,000 OXD (0.01% del supply).
- **Costo de activación por wallet**: ~0.002 SOL una sola vez (creación de `TrackingAccount`), pagado por el emisor.

## Qué es y qué no es
- **Es** un experimento de política monetaria programática, con reglas inmutables en bytecode.
- **No es** un stablecoin ni una reserva institucional probada. Volatilidad de mercado aplica; la deflación es sobre unidades, no sobre precio en fiat.
- **Es** composable con DeFi (Token-2022 + hooks); el token SPL es "limpio" y el estado económico vive en PDAs.
- **No es** upgradeable: cambios requieren nuevo deploy y migración voluntaria.

## Especificación rápida
- **Deflación**: modelo **exponencial** de 20% anual sobre `balance_free`: $\text{remaining}=\text{balance}\times(0.8)^{\text{años}}$ usando factor diario preciso (0.8^(1/365)) + aproximación Taylor para fracciones < 24h.
- **Stake**: `balance_staked` no sufre burn; puede volver a `balance_free` vía `unstake` (excepto creador, bloqueado por código).
- **Grace de 15 min**: si `elapsed > 900s` el hook bloquea transferencias hasta que el usuario llame `clear_debt()` (o venda tras limpiar). Traders pueden operar dentro de la ventana para minimizar burn real.
- **Transfer hook**: valida tracking, hereda timestamps entre usuarios y resetea a `now` cuando el origen es un pool whitelisted.
- **Vesting creador**: solo vía `release_creator_tokens(amount_traded, 0.1%)` con tope diario del 1% del supply total; `unstake` del creador revierte siempre.
- **Program IDs**: hardcoded (OXIDE + hook); whitelist de AMMs fija en bytecode.

## Riesgos y mitigaciones
| Tipo | Riesgo | Mitigación / Hecho concreto |
|------|--------|------------------------------|
| Técnico | Límite de Compute Units en `transfer_hook()` | Optimizado a una sola lectura de `Clock`, sin bucles; usar CU price/limit en congestión. |
| Técnico | Rotación de program IDs de DEX | Whitelist hardcoded (Raydium V4, Orca Whirlpool, Meteora DLMM). Si cambian IDs, se necesita fork/deploy nuevo. |
| UX | Primera recepción falla si el emisor no tiene SOL | Inicialización automática con `init_if_needed`; requerir ~0.002 SOL en el emisor. |
| UX | Bloqueo post 15 minutos | Llamar `clear_debt()` antes de vender; wallets/SDK pueden hacer batching atómico (`clear_debt + swap`). |
| Económico | Wash-trading para liberar vesting | Cap diario 1% del supply, coste de fees y burn sobre balances libres, liberación estricta al 0.1% del volumen. |
| Económico | Intento de evadir burn con micro-transfers | Grace es de UX; deuda permanece en el saldo restante, timestamps ponderados, costo de fees > ahorro salvo precios extremos. |
| Dependencia L1 | Riesgo Solana (interrupciones, fees) | Reconocido; no hay mitigación on-chain más allá de la simplicidad del hook y el uso de CU price. |

## Política monetaria (correcta y verificable)
- **Función de burn (lazy burn)**: se calcula al interactuar (`deposit`, `withdraw`, `transfer`, `clear_debt`, etc.). No hay cron jobs. Modelo **exponencial** de 20% anual: si no tocas fondos por 1 mes, el balance resultante será $\text{balance}\times(0.8)^{30/365}\approx0.9831\times\text{balance}$ (pérdida ~1.69%).
- **Inmunidad por stake**: mover a `balance_staked` detiene el reloj; volver a `balance_free` reactiva el timer desde `now`.
- **Composición con pools**: los LP tokens de pools whitelisted no acumulan deuda; el comprador desde pool recibe timestamp limpio.
- **Sin yield prometido**: la apreciación potencial proviene solo de la reducción de unidades, no de pagos o intereses.

### Ejemplo numérico (burn)
Balance libre: 1,000 OXD. Tiempo inactivo: 90 días.

$$
\text{remaining}=1000\times(0.8)^{90/365}\approx 1000\times0.9512\approx 951.2\ \text{OXD}
$$
$$
\text{burn}=1000-951.2\approx 48.8\ \text{OXD}
$$

Saldo libre tras burn: ~950.7 OXD. Si el usuario stakea antes, el burn se detiene (0%).

### Ventana de 15 minutos: uso práctico en trading
- Objetivo: reducir fricción para market makers y traders activos.
- Operativa recomendada:
  1) Para scalping/arbitraje intra-bloque o intra-15m: el burn efectivo es ≈0; la operación no requiere `clear_debt()` si se liquida antes del límite.
  2) Si mantienes inventario >15m: ejecuta `clear_debt()` antes de vender, o deposita/stakea para pausar el reloj.
  3) Bots: timestamps se promedian al recibir; mover tokens entre tus propias wallets no borra antigüedad sin costo.

## Flujo técnico esencial
1) **initialize_global**: fija autoridad, mint y crea cuentas globales; supply inicial interno (10k libres, resto staked del creador).
2) **verify_mint_authority**: asegura que el PDA es autoridad del mint; requisito para operar `withdraw`.
3) **transfer_hook** (SPL):
   - Bloquea si `elapsed>900s` y origen no es pool whitelisted.
   - Hereda timestamp sender→receiver (ponderado por balance) o resetea a `now` si proviene de pool.
   - Inicializa `TrackingAccount` del receptor con cargo al emisor si no existe.
4) **clear_debt**: aplica burn acumulado sobre SPL antes de vender, sincroniza tracking vía CPI.
5) **deposit/withdraw**: quema/mint SPL 1:1, aplica burn pendiente, sincroniza tracking.
6) **release_creator_tokens**: 0.1% del volumen, cap diario 1%; se ejecuta en transfer/withdraw/deposit. `unstake` del creador está prohibido por bytecode.

## Playbooks por rol
- **Holder (largo plazo)**: stakear siempre (`balance_staked`), 0% decay; des-stake solo cuando se necesite liquidez. Riesgos: L1/contrato y volatilidad de precio.
- **Trader/MM**: operar dentro de 15m para minimización de burn; al superar la ventana, llamar `clear_debt()` antes de vender o rotar inventario; mantener SOL para inicializar tracking de contrapartes nuevas.
- **Integradores DeFi**: usar `init_if_needed` para tracking; proveer SOL en contratos que distribuyan; para swaps, agrupar `clear_debt + swap` en una TX; pools whitelisted no acumulan deuda.

## Protocolo de tracking (costos y seguridad)
- Cada wallet requiere un `TrackingAccount` (80 bytes, rent-exempt ~0.00204 SOL). Se crea automáticamente la primera vez que recibe OXD; paga el emisor.
- Razón de diseño: evita que alguien reciba tokens y evada burn; asegura consistencia de timestamps y promedios.
- Fallo esperado si no hay SOL: la TX revierte. Solución: prefundear o advertir al usuario.

## AMM Whitelist & Delegate Transfers

### Pool Whitelist Mechanism

El transfer hook whitelista **pools de liquidez** de DEXs principales para prevenir que la validación de 15 minutos bloquee trading normal:

- **Raydium V4**: `675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8`
- **Orca Whirlpool**: `whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc`
- **Meteora DLMM**: `LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo`

### Cómo Funciona la Detección de Whitelist

El hook valida el **program owner** de la cuenta source token:

```rust
// Para transfers directos (pool authority es signer):
if ctx.accounts.owner.key() == source_token.owner {
    let owner_program_id = ctx.accounts.owner.owner;  // Programa que posee la pool PDA
    sender_is_pool = is_whitelisted(&owner_program_id);
}
```

**Pools whitelisted evitan el requisito de `clear_debt()` de 15 minutos.**

---

### ⚠️ LIMITACIÓN: Delegate Transfers desde Pools

**La whitelist solo funciona para TRANSFERS DIRECTOS** donde la pool authority es el signer.

**Delegate transfers NO están whitelisted**, incluso si el source token owner es una pool:

```rust
// Delegate transfer (router/aggregator usa delegación):
if ctx.accounts.owner.key() != source_token.owner {
    sender_is_pool = false;  // ❌ Tratado como usuario regular
    // Debe satisfacer regla de 15 minutos o llamar clear_debt()
}
```

#### ¿Por Qué Este Diseño?

1. **Seguridad**: Los delegates podrían ser cualquier programa arbitrario. Whitelistarlos crearía exploits de bypass.
2. **Pragmatismo**: Los DEXs mainstream (Raydium/Orca/Meteora) usan **transfers directos** para swaps normales.
3. **Inmutabilidad**: La whitelist está hardcoded. No podemos validar relaciones de delegates dinámicas sin upgradability.

#### Impacto en Integraciones

**✅ Funciona out-of-the-box:**
- Swaps estándar de Raydium
- Swaps estándar de Orca
- Swaps estándar de Meteora
- Rutas single-hop de Jupiter (cuando usa llamadas directas a pools)

**⚠️ Requiere adaptación:**
- Rutas multi-hop de Jupiter aggregator usando delegates
- Bots de arbitraje custom con authorities delegadas
- Protocolos de limit orders que delegan acceso a pools

**Solución para edge cases:**
Los integradores deben llamar `clear_debt()` antes de delegate transfers desde pools:

```typescript
// Ejemplo: Jupiter multi-hop con OXIDE
await program.methods.clearDebt().accounts({
  user: poolAuthority,
  userAccount: poolUserPDA,
  trackingAccount: poolTrackingPDA,
  // ...
}).rpc();

// Ahora el delegate transfer tendrá éxito
await jupiterSwap({
  inputMint: OXIDE_MINT,
  // ...
});
```

#### Trade-off Aceptado

Priorizamos **inmutabilidad radical** sobre conveniencia de edge cases:
- Ninguna governance puede modificar la whitelist
- Sin backdoors vía "actualizaciones de seguridad"
- Sin centralización progresiva (común en DAOs)
- Código es ley: si necesitas delegates, adapta tu integración

**Si esta limitación rompe tu caso de uso, OXIDE puede no ser el protocolo adecuado.**

---

### Monitoreo Post-Deploy

Después del lanzamiento en mainnet, monitorizaremos:
1. % de transfers fallidos desde pools legítimos
2. Quejas de usuarios sobre swaps bloqueados en DEXs principales

**Si >5% del volumen de pools es afectado**, consideraremos:
- Upgrade para soportar detección de delegates (requiere añadir `source_owner_account` a ExtraAccountMetas)
- O mantener status quo si el volumen afectado es negligible

**Timeline de decisión:** 30 días de observación post-mainnet.

## Inmutabilidad y gobierno
- **Sin upgrade path**: cualquier cambio requiere nuevo programa y migración voluntaria. No hay multi-sig que pueda alterar reglas monetarias o la whitelist.
- **Whitelist fija**: Raydium V4, Orca Whirlpool, Meteora DLMM. Si un DEX rota IDs, la versión actual no se adapta; se prioriza la confianza en reglas fijas.
- **Creador bloqueado**: `unstake` prohibido, vesting solo por volumen (0.1% con cap diario 1%). El código es la única política.

## Comparativa honesta (reserva de valor, 0–10)
| Activo | Nota | Comentario breve |
|--------|------|------------------|
| USD | 2.0 | Máxima liquidez, política expansiva y dependencia estatal. |
| EUR | 1.8 | Similar a USD; dependencia del Eurosistema. |
| Bonos Tesoro USA | 4.5 | Previsibles pero con riesgo soberano/inflación; confianza en emisor. |
| Oro papel | 5.5 | Liquidez alta; riesgo de contraparte y oferta sintética. |
| ETH (como reserva) | 6.5 | Utilidad y burn parcial; política mutable. |
| OXIDE (hoy) | 6.8 | Reglas fijas y supply programado; depende de Solana y adopción temprana. |
| Oro físico | 8.5 | Historia y neutralidad; custodia y transferencia costosas. |
| BTC | 9.2 | Escasez absoluta y trayectoria; volatilidad y dependencia futura del fee market. |
| OXIDE (maduro) | 8.0–8.5 | Si mantiene reglas y adopción; no supera a BTC en simplicidad ni independencia de L1. |

## Comunicación sin hype
- Enfatizar: reglas monetarias explícitas, verificables y fijas; ausencia de yield prometido; deflación máxima del 20% anual sobre saldos libres; staking como pausa del reloj.
- Evitar: "mejor que BTC/oro" o promesas de precio; la adopción y el tiempo son los validadores.
- Transparencia: mencionar siempre la dependencia de Solana, la ventana de 15 minutos y el costo de activación por wallet.

## FAQ breve
- **¿La deflación es siempre 20%?** No. Es **exponencial compuesta**: 20% anual = retención del 80% = $(0.8)^{\text{años}}$. Asintóticamente se acerca al 100% quemado pero nunca lo alcanza. En stake es 0%.
- **¿Puedo operar sin pagar burn?** Operar <15m reduce la exposición; fuera de la ventana debes `clear_debt()` o asumir el burn exponencial acumulado. Depositar/stakear pausa el reloj.
- **¿Qué pasa si un DEX nuevo aparece?** La versión actual no lo soporta; requeriría un fork/deploy nuevo. La inmutabilidad es deliberada.
- **¿Cómo vendo como creador?** Solo lo que libera el mercado (0.1% del volumen, cap diario 1%). El código rechaza `unstake` del creador.

## Disclaimer
OXIDE es experimental. No es asesoramiento financiero. La deflación programada no garantiza preservación de precio en fiat. Evaluar riesgo de contrato y de la capa 1. Opera solo con fondos que puedas permitirte perder.# OXIDE: The Time-Decay Monetary Protocol
## Propuesta de Activo Deflacionario Algorítmico en Solana (Token-2022)

---

## Elevator Pitch

**OXIDE introduce el concepto de 'Time-Value of Supply': un estándar de Token-2022 donde la escasez no es una promesa futura, sino una función matemática del tiempo transcurrido.**

El protocolo codifica una reducción de suministro del 20% anual directamente en el bytecode mediante PDAs inmutables. A través de Transfer Hooks y Zonas de Liquidez sin Fricción (Liquidity Neutrality Zones), OXIDE garantiza deflación matemática mientras preserva composability en ecosistemas DeFi. Es el primer protocolo que resuelve la paradoja entre política monetaria (escasez) y liquidez on-chain (volumen).

---

## ?? Riesgos Críticos y Mitigaciones

**OXIDE es un experimento de protocolo deflacionario algorítmico. No es un "activo de reserva institucional"; es una propuesta tecnológica que solo podría usarse como tal si el modelo demuestra estabilidad y adopción.**

### Riesgos Técnicos

| Riesgo | Impacto | Mitigación Implementada |
|--------|---------|-------------------------|
| **Límites de Compute Units (CU)** | Transfer Hooks consumen CU en cada transacción SPL. En congestión extrema, transfers pueden fallar. | - Optimización de operaciones en `transfer_hook()` (1 sola llamada a `Clock::get()`)<br>- Lazy burn (cálculo on-demand, no loops)<br>- Usuarios pueden ajustar CU Price/Limit en wallets modernas |
| **Precisión del Clock** | `Clock::get()` es slot-based (~400ms de latencia). No es preciso para sub-segundo. | - Aceptable para burn anual (error <0.001%)<br>- Documentado en código con notas técnicas |
| **Whitelist de AMMs Estática** | Si DEXs rotan program IDs o PDAs, la "Zona Franca" puede quedar obsoleta. | - Whitelist hardcoded en `hook_lib.rs` para v1.0<br>- Roadmap: Migrar a whitelist on-chain actualizable por governance |
| **Costo de Inicialización** | Primera transferencia a un wallet nuevo cuesta ~0.002 SOL extra (crear `TrackingAccount`). | - Documentado en Readme<br>- El **sender** paga la renta (no el receiver)<br>- Costo único, no recurrente<br>- **?? CRÍTICO**: Si el sender NO tiene 0.002 SOL disponible, la transacción FALLARÁ. Ver sección [Protocolo de Tracking](#protocolo-de-tracking-inicialización-obligatoria) |

### Riesgos Económicos

| Riesgo | Impacto | Mitigación Implementada |
|--------|---------|-------------------------|
| **Wash-Trading del Creador** | El creador podría hacer trading falso para liberar tokens vía vesting dinámico. | - **Cap diario de 1% del supply total** (10M OXIDE/día)<br>- Límite codificado en `release_creator_tokens()`<br>- Auditable on-chain via `GlobalState.daily_released_amount` |
| **Bots Reseteando Clock** | Ballenas podrían hacer micro-transfers cada 14 min para evitar burn. | - Threshold de 15 min es **arbitrario** (reconocido en auditoría)<br>- TODO: Migrar a decay continuo proporcional al balance<br>- Versión actual: Gas cost + obligación de clear_debt() desincentiva bots |
| **Volatilidad vs. Fiat** | Como todo cripto, OXIDE es más volátil que USD/EUR a corto plazo. | - **No es sustituto de stablecoins**<br>- Diseñado para preservación de valor a largo plazo (>5 años)<br>- Stakear para protección total |

### Consideraciones de UX

- **Friction en Venta**: Si han pasado >15 min sin actividad, debes llamar `clear_debt()` antes de vender en DEX. Wallets modernas (Phantom, Solflare) pueden abstraer esto en **atomic batching** (1 sola transacción).
- **Congestión de Red**: En periodos de alta demanda en Solana, los Transfer Hooks pueden requerir ajustar el "Compute Unit Price" manualmente. Esto es común en protocolos Token-2022 avanzados.
- **Educación de Usuario**: El concepto de "oxidación temporal" es **nuevo**. Requiere onboarding claro para evitar confusión.
- **?? Costo de Primera Recepción**: La primera vez que una wallet recibe OXIDE, el **emisor** (no el receptor) paga una tasa única de ~0.002 SOL para inicializar el Protocolo de Tracking. **Esto es obligatorio y la transacción fallará si el emisor no tiene SOL suficiente**. Este es un requerimiento arquitectónico de Solana para crear el espacio de almacenamiento que rastrea tu deuda de oxidación. Ver sección detallada: [Protocolo de Tracking](#protocolo-de-tracking-inicialización-obligatoria).

**Recomendación**: No uses OXIDE como reserva de emergencia (liquidez <24h). Es ideal para holders de medio-largo plazo (3-5 años) que valoran escasez matemática sobre volatilidad a corto plazo.

---

## Tabla de Contenidos
1. [Resumen Ejecutivo](#resumen-ejecutivo)
2. [Propuesta de Valor](#propuesta-de-valor)
3. [Comparativa de Activos](#comparativa-de-activos)
4. [Teoría de Juegos & Incentivos](#teoría-de-juegos--incentivos)
5. [Arquitectura Técnica](#arquitectura-técnica)
6. [Auditoría de Calidad](#auditoría-de-calidad)
7. [Proyecciones Económicas](#proyecciones-económicas)
8. [Roadmap](#roadmap)
9. [FAQ: Mecanismos de Control](#faq-mecanismos-de-control-de-suministro)
10. [Garantías Anti-Rugpull](#garantías-anti-rugpull-código-es-ley)

---

## Resumen Ejecutivo

**OXIDE** es una propuesta de activo deflacionario algorítmico nativo de Solana que implementa un mecanismo de oxidación temporal (time-decay) codificado en el protocolo. A diferencia de activos con política monetaria externa (Bitcoin, Ethereum) o centralizada (USD, BNB), OXIDE:

- **Codifica deflación en el protocolo**: 20% anual de reducción de suministro circulante, determinístico y sin governance (diseño propuesto)
- **Política monetaria inmutable**: Stored en PDAs (Program Derived Addresses), imposible de modificar sin re-desplegar el bytecode
- **Transfer Hook de sincronización**: Rastrea oxidación per-usuario, mantiene consistencia entre cadena y aplicación
- **Zonas de Liquidez sin Fricción**: AMMs whitelisted (Raydium, Orca, Meteora) operan sin acumular temporal oxidation exposure
- **Fairness Mechanism**: Compradores siempre entran "limpios" - no heredan exposición a deuda de vendedores anteriores
- **Vesting Dinámico Anti-Rugpull**: El creador solo puede vender tokens liberados por volumen de mercado (0.1% del trading), no puede unstakear manualmente

**Propósito**: Explorar si la deflación matemática es compatible con liquidez on-chain, y si reglas económicas verificables e inmutables aportan valor a los participantes.

---

## Propuesta de Valor

### El Dilema del Tiempo en Economía Monetaria

La criptoeconomía moderna enfrenta un dilema fundamental: **¿Cómo crear escasez real sin sacrificar liquidez?**

**El Costo de Oportunidad del Capital Ocioso**

En sistemas monetarios tradicionales, el dinero "dormido" (ocioso) es problema económico:
- En USD: La inflación de 2-3% anual erosiona capital inactivo
- En Bitcoin: Los holders son incentivados a no gastar (especulación), reduciendo velocidad del dinero
- En Ethereum: La quema es opaca y depende de demanda de gas (no garantizada)
- En la mayoría de tokens: Baja liquidez porque los holders especulan o temen perder

**La Solución OXIDE: Incentivos para la Velocidad del Dinero**

OXIDE invierte el modelo: penaliza la inercia, recompensa el movimiento.

```
ARQUITECTURA DE INCENTIVOS OXIDE:

1. HOLDER (Staker)
   +- Protección total contra oxidación
   +- 0% reducción anual
   +- Incentivo: Seguridad de valor a largo plazo
   
2. TRADER (Capital en movimiento)
   +- Oxidación: 20% anual si inactivo >15 min
   +- Pero: Liquidez sin fricción en DEXs
   +- Incentivo: Circulación activa + acceso a markets
   +- Costo aceptable por facilidad
   
3. PROTOCOLO
    +- Escasez programada (según diseño; no depende de belief)
   +- Liquidez perpetua (no se estanca)
   +- Composability nativa (AMMs operan sin miedo)
```

**Por qué esto importa para adoption a largo plazo:**

? **Elimina la especulación estéril**: No hay incentivo de hold-and-pray  
? **Promueve flujo de valor**: Capital circula, no estanca  
? **Escasez real, no promesa**: La deflación ocurre ahora, no "eventualmente"  
? **Seguridad de protegidos vs riesgo de traders**: Cada participante elige su rol  

### Composability-Friendly Design

A diferencia de tokens con fricción (transfer tax), OXIDE es diseñado para ser integrado en protocolos DeFi sin sorpresas:

- **Lending Protocols**: Pueden usar OXIDE como colateral sin miedo a evaporación inesperada
- **AMMs**: Liquidity providers no sufren degradación de pool por transfer fees
- **Derivatives**: Perpetuals y opciones funcionan nativamente sin ajustes de margen por taxa
- **Cross-Protocol Bridges**: La arquitectura de PDA permite composición segura entre cadenas

**Ventaja técnica**: La oxidación ocurre a nivel de usuario (`UserAccount`), no de token. El token SPL es un espejo limpio, sin modificaciones que compliquen integración.

---

## Comparativa de Activos

OXIDE se posiciona en tres categorías fundamentales de inversión. A continuación, su comparación contra referentes institucionales.

### 1. Activos de Reserva (Bitcoin, Oro)

| Criterio | Bitcoin | Oro | OXIDE |
|----------|---------|-----|------|
| **Rareza** | Fija (21M) | Geológica | Matemática (20% anual) |
| **Escasez programada** | Sí (2140) | Sí (geológica) | Sí (código) |
| **Inflación** | 0% (fija) | 0% (fija) | -20% anual |
| **Transferencia** | 10 min (blockchain) | 3-5 días (físico) | <1 segundo (Solana) |
| **Verificabilidad** | Blockchain pública | Certificados | On-chain, auditable |
| **Custodia** | Blockchain/Hardware | Bóveda de terceros | Autocustody |
| **Años de track record** | 16 años | 5000 años | 2026 (nuevo) |

**Enfoque OXIDE**: Escasez programada como parte del protocolo; liquidez instantánea vs. multi-día.

**Ventaja Bitcoin/Oro**: Histórico probado, aceptación universal.

**Caso de uso**: Inversor que quiere preservar poder de compra ahora, no en 2140. Alternativa para emergentes sin acceso a bóvedas de oro.

---

### 2. Activos de Flujo (Ethereum, Solana)

| Criterio | Ethereum | Solana | OXIDE |
|----------|----------|--------|------|
| **Inflación/Deflación** | Variable (EIP-1559) | Variable (~8% anual) | -20% anual (constante) |
| **Deflación programada** | No (depende de gas) | No (incentivos de validación) | Sí (matemática, bytecode) |
| **Función primaria** | Smart contracts | Blockchain base | Activo monetario |
| **Gobernanza** | Multi-sig + upgrade | Validators + governance | Código (sin upgrade) |
| **Liquidez on-chain** | Excelente | Excelente | Excelente |
| **Casos de uso** | DeFi, NFTs, staking | Infrastructure | Reserva de valor |

**Enfoque OXIDE**: Política monetaria determinista en el bytecode. ETH y SOL pueden cambiar con upgrades governance.

**Ventaja ETH/SOL**: Utilidad como blockchain, flujos de transacciones reales, ecosistema maduro.

**Posible caso de uso**: Cobertura experimental contra inflación dentro del ecosistema Solana, sin depender de layer-1 economics.

---

### 3. Activos Fiat (USD)

| Criterio | USD | OXIDE |
|----------|-----|------|
| **Inflación/Deflación** | +2-3% anual | -20% anual |
| **Controlador de política** | Federal Reserve | Código (inmutable) |
| **Transparencia** | Reservas ocultas | Auditable on-chain |
| **Punto central de fallo** | Gobierno/Banco Central | Ninguno (blockchain) |
| **Transferencia** | 1-3 días (ACH) | <1 segundo |
| **Costo de custodia** | Mínimo (cuenta bancaria) | Negligible (blockchain) |
| **Aceptación** | Universal | Niche (cripto-native) |

**Enfoque OXIDE**: Protección programática contra inflación (según diseño), transacciones instantáneas, sin intermediarios, transparencia on-chain.

**Ventaja USD**: Aceptación global, volatilidad baja (a corto plazo).

**Caso de uso**: Alternativa para markets inestables o con desconfianza institucional en bancos centrales. Ideal para emergentes sin acceso fácil a reservas de valor.

---

## Matriz de Decisión (Selección de Activo)

```
¿Preservar valor sin inflación?        ? OXIDE / Bitcoin / Oro
¿Liquidez instantánea?                 ? OXIDE / Crypto
¿Transparencia matemática?             ? OXIDE / Bitcoin
¿Aceptación universal?                 ? USD / Bitcoin
¿Política monetaria programada?        ? OXIDE
¿Ecosistema DeFi nativo?               ? OXIDE / Ethereum / Solana
¿Riesgo bajo (probado)?                ? Bitcoin / Oro / USD
¿Innovación de protocols?              ? OXIDE / Ethereum
```

---

## Matriz comparativa de reserva de valor (visión fría)

Perspectiva: reserva de valor a largo plazo (no medio de pago, no activo productivo). Escala 0–10 por función específica, asumiendo OXIDE ya desplegado y reglas respetadas. Marco técnico, no marketing.

| Activo | Nota | Razonamiento breve |
|--------|------|--------------------|
| USD | 2.0 | Liquidez máxima, pero política expansiva y dependencia estatal. |
| EUR | 1.8 | Similar a USD, algo más débil fuera de su región. |
| Bonos Tesoro USA | 4.5 | Previsibles, pero riesgo soberano e inflación residual; requieren confianza en emisor. |
| Oro papel (ETFs/derivados) | 5.5 | Liquidez alta, pero riesgo de contraparte y oferta sintética; pierde soberanía del oro físico. |
| ETH (como reserva, no como gas) | 6.5 | Utilidad real y burn parcial; gobernanza mutable y cambios de reglas. |
| OXIDE (hoy) | 6.8 | Política fija, supply programado, sin yield; depende de Solana y adopción temprana. |
| Oro físico | 8.5 | Historia y neutralidad; transferibilidad y custodia costosas. |
| BTC | 9.2 | Escasez absoluta, neutralidad, trayectoria; volatilidad y dependencia futura de fee market. |
| OXIDE (si madura) | 8.0–8.5 | Se acerca a BTC si sostiene reglas y adopción; no lo supera en simplicidad ni independencia de L1. |

Lectura honesta: OXIDE no compite con BTC hoy; es conceptualmente más sólido que fiat y oro papel, y con adopción puede superar a ETH como reserva pasiva. La clave es no cambiar reglas.

### Valor intrínseco (suponiendo adopción)

- **BTC (9.5/10)**
    - Fortalezas: escasez absoluta, inmutabilidad, neutralidad, seguridad probada.
    - Debilidades: dependencia futura de fee market; riesgo de concentración minera a muy largo plazo.
- **ETH (6.5/10 como reserva)**
    - Fortalezas: uso real (gas), demanda estructural, burn parcial.
    - Debilidades: política monetaria mutable, governance implícita, cambios históricos de reglas.
- **Oro papel (4.5/10)**
    - Fortalezas: anclaje teórico al oro físico, liquidez.
    - Debilidades: riesgo de contraparte, rehypothecation, oferta sintética, no soberanía.
- **OXIDE (8.7–9.0/10 con adopción)**
    - Fortalezas: supply fijo y verificable, burn determinista, sin governance monetaria, creador bloqueado, no depende de precio para sostener política.
    - Debilidades: dependencia de Solana (riesgo L1), lógica más compleja que BTC, hooks añaden fricción, aún no probado en el tiempo.

### Comunicación sin hype

- Enfatizar: política monetaria explícita, reglas simples y verificables, ausencia de promesas y de yield, inmutabilidad de la oferta.
- Evitar proclamar “mejor que BTC/oro”; dejar que terceros comparen. Las reservas de valor se descubren, no se declaran.
- Objetivo: que la solidez percibida provenga de la persistencia de las reglas; el tiempo es el validador real.

---

## Arquitectura Técnica

### Sistema de Deflación (Lazy Burn)

**Mecanismo**: El tiempo es el factor de quema.

```
burn = balance_free × ANNUAL_BURN_BP × (elapsed_seconds / SECONDS_PER_YEAR)
    = balance_free × 0.20 × (elapsed / 31,536,000)
```

**Ejemplo real**:
```
Usuario:  OXIDE (balance_free)
Desde hace: 365 días (31,536,000 segundos)

Quema = 1000 × 0.20 × (31,536,000 / 31,536,000) =  OXIDE
Saldo final =  OXIDE

20% anual garantizado, sin governance.
```

**Protección anti-exploit**: Campo `burn_fraction_remainder` (u128) acumula decimales perdidos por división entera, imposible evadir burn fragmentando transacciones.

---

### Transfer Hook (Token-2022)

Cada transferencia SPL pasa por validación on-chain:

```
if sender_is_whitelisted_pool:
    ? PERMITIDO (Zona Franca)
    receiver.timestamp = now (comprador entra limpio)
else:
    elapsed = now - sender.timestamp
    if elapsed > 15 min:
        ? BLOQUEADO (requiere clear_debt() primero)
    else:
        ? PERMITIDO
        receiver.timestamp = inherited (weighted average si tiene balance)
```

**Whitelist Zona Franca** (Raydium V4, Orca Whirlpool, Meteora DLMM):
- Pools no acumulan deuda
- Transacciones sin fricción
- Buyer siempre entra limpio

#### La Ventana de Cortesía de 15 Minutos

**Decisión de Diseño**: El threshold de 15 minutos **NO es un parámetro económico**, sino una **ventana de UX** para permitir operativa fluida.

**¿Por qué 15 minutos?**
- ? Permite swaps, arbitrage y transferencias rápidas sin fricción constante
- ? Los usuarios normales pueden operar sin llamar `clear_debt()` en cada transacción
- ? Suficientemente corto para evitar acumulación de deuda masiva antes de venta

**Inmunidad a Bots (Respuesta a Auditoría):**

La auditoría preguntó: *"¿Puede un bot hacer micro-transfers cada 14 min para evadir burn?"*

**Respuesta**: NO. He aquí por qué:

1. **Deuda Estatutaria**: La oxidación se calcula sobre `balance_free` **remanente**. Si un bot tiene  OXIDE y transfiere  OXIDE cada 14 min, la deuda de los  OXIDE restantes sigue acumulándose.

2. **Costo de Red**: Cada transfer cuesta ~0.000005 SOL de fee + priority fees en congestión. Para resetear el clock de  OXIDE durante 1 año:
   - Transfers necesarios: ~37,000 (1 cada 14 min)
   - Costo total: ~0.185 SOL (~$35 USD al precio actual)
   - Ahorro de burn:  OXIDE (20% anual)
   - **Net economics**: Solo es rentable si OXIDE vale >$0.175 USD/token

3. **Weighted Average Timestamp**: Si el bot se transfiere a sí mismo, el `receiver_tracking` calcula weighted average. Los tokens "viejos" arrastran antigüedad.

4. **Clear Debt Obligatorio**: Antes de vender en DEX, DEBE llamar `clear_debt()`, que sincroniza toda la deuda acumulada.

**Conclusión**: La ventana de 15 min es "suficientemente buena" en el sentido de ingeniería de sistemas. Es barata de ejecutar, imposible de romper a escala, y fácil de entender.

---

### Sincronización Dual: UserAccount ? TrackingAccount

**Problema resuelto**: SPL tokens evaden burn si solo están en wallet.

**Solución**: Transfer Hook rastrea `last_update` timestamp en TrackingAccount del programa hook. Sincronización bidireccional via CPI:

```
withdraw() ? Mintea SPL ? CPI sync_tracking() ? Hook se actualiza
clear_debt() ? Quema SPL ? CPI sync_tracking() ? Hook se actualiza
transfer() ? Hook valida ? Heredita timestamp
```

---

### Protocolo de Tracking: Inicialización Obligatoria

**?? ADVERTENCIA CRÍTICA PARA INTEGRADORES Y USUARIOS INSTITUCIONALES**

OXIDE implementa un sistema de tracking per-wallet para rastrear la deuda de oxidación temporal. Este mecanismo es **fundamental** para la integridad del protocolo y requiere una inicialización on-chain la primera vez que una wallet recibe tokens.

#### ?? Especificación Técnica

**Cuenta Requerida**: `TrackingAccount` (PDA del programa Transfer Hook)
- **Espacio**: 8 bytes (discriminator) + 72 bytes (datos) = 80 bytes
- **Renta Obligatoria**: ~0.00204 SOL (rent-exempt threshold de Solana)
- **Frecuencia**: Una sola vez por wallet receptor
- **Payer**: El **emisor** de la transferencia, NO el receptor

#### ?? Condición de Falla

**Si el emisor NO tiene 0.002 SOL disponible en su wallet**, la transacción SPL **REVERTIRÁ** con error:

```
Error: failed to send transaction: Transaction simulation failed: Error processing Instruction 0: 
custom program error: 0x1 (insufficient funds for rent)
```

**Esto NO es un bug. Es una restricción arquitectónica de Solana.**

#### ?? Implicaciones para Integradores Institucionales

**Exchanges Centralizados (CEX)**:
- Deben mantener balance de SOL suficiente en hot wallets para cubrir inicializaciones
- **Estimación conservadora**: 0.005 SOL por usuario nuevo (buffer de seguridad)
- Para 10,000 usuarios nuevos/día: ~50 SOL de overhead operacional

**Protocolos DeFi (Lending, Staking)**:
- Los contratos inteligentes que distribuyen OXIDE deben pre-fundear con SOL
- Usar `init_if_needed` en Anchor para manejar inicialización automática
- Considerar atomic batching: `initialize_tracking + transfer` en una sola TX

**Market Makers y Bots de Arbitraje**:
- Primera interacción con usuario nuevo cuesta gas extra
- Optimización: Pre-inicializar `TrackingAccount` de usuarios frecuentes
- Monitorear balance de SOL para evitar fallos en alta frecuencia

#### ? Recomendaciones de Implementación

**Para Wallets (Phantom, Solflare, etc.)**:
```typescript
// Pre-flight check antes de enviar OXIDE
const recipientTracking = await getTrackingAccount(recipientPubkey);
if (!recipientTracking) {
  // Advertir al usuario:
  showWarning(
    "Primera transferencia a esta wallet. "
    + "Requiere 0.002 SOL adicional para activación."
  );
  
  // Validar balance del sender
  const senderSOL = await connection.getBalance(senderPubkey);
  if (senderSOL < LAMPORTS_PER_SOL * 0.002) {
    throw new Error("Insuficiente SOL para inicializar tracking.");
  }
}
```

**Para Protocolos DeFi**:
```rust
// En tu programa Anchor, usar init_if_needed
#[account(
    init_if_needed,
    payer = payer,  // ¿Quién paga? Define según tu modelo de negocio
    space = 8 + TrackingAccount::INIT_SPACE,
    seeds = [b"tracking", recipient.key().as_ref()],
    bump
)]
pub recipient_tracking: Account<'info, TrackingAccount>,
```

#### ?? Análisis de Costos Comparativos

| Protocolo | Costo de Primera Interacción | Frecuencia |
|-----------|-------------------------------|------------|
| **OXIDE** | ~0.002 SOL (inicialización tracking) | Una vez por wallet |
| **USDC (Token-2022)** | ~0.00204 SOL (ATA creation) | Una vez por wallet |
| **Stablecoins tradicionales** | ~0.00204 SOL (ATA creation) | Una vez por wallet |
| **NFTs (Metaplex)** | ~0.0135 SOL (metadata + ATA) | Una vez por mint |

**Conclusión**: El overhead de OXIDE es **equivalente** a cualquier token SPL estándar. La diferencia es que el tracking es **explícito** y **obligatorio** para garantizar integridad del sistema de oxidación.

#### ?? Seguridad y Transparencia

**¿Por qué no usar un modelo "lazy" sin inicialización?**

Si el tracking fuera opcional o lazy:
1. **Vector de ataque**: Usuarios podrían recibir tokens SPL sin tracking, evadiendo oxidación
2. **Inconsistencia**: Algunos holders tendrían deuda acumulada, otros no
3. **Explotación**: Bots podrían crear wallets desechables sin tracking para wash-trading

**OXIDE prioriza integridad del protocolo sobre conveniencia**. El costo de 0.002 SOL es el precio de la inmutabilidad y fairness.

**Para más detalles técnicos**: Ver [`hook_lib.rs`](c:\Users\th3vil\Desktop\OXIDE\hook_lib.rs) línea 395 (struct `TransferHook`) y comentarios sobre `init_if_needed`.

---

## Teoría de Juegos & Incentivos

### El Equilibrio entre Holders y Traders

OXIDE introduce dos roles económicos con incentivos alineados pero distintos:

#### 1. The Holder (Staker)
- **Estrategia**: Mantener valor a largo plazo mediante staking
- **Protección**: 0% reducción de suministro (oxidation immunity)
- **Retorno**: Beneficio indirecto vía escasez de circulante (aprecia relativamente)
- **Riesgo**: Riesgo técnico (blockchain), no mercado
- **Horizonte**: 5+ años

**Matriz de pagos:**
```
Si el mercado crece ? Valor de token ?? (staker gana)
Si el mercado cae  ? Riesgo minimizado (scarcity protection)
Si inactividad      ? Protegido (0% burn)
```

#### 2. The Trader (Circular Capital)
- **Estrategia**: Ejecutar transacciones, arbitrage, provisión de liquidez
- **Costo**: Oxidación temporal (20% anual) si inactivo >15 min
- **Beneficio**: Liquidez instantánea, comisiones de DEX, spreads
- **Riesgo**: Exposición temporal a oxidación, volatilidad de precio
- **Horizonte**: Horas a semanas

**Matriz de pagos:**
```
Si hace transacciones activas ? Costo de oxidación < Ganancia (spread + comisiones)
Si compra desde pool         ? Entra limpio (no hereda deuda)
Si la vende rápido           ? Oxidación mínima (clock resets)
```

#### 3. The Protocol (Incentive Alignment)

El protocolo se beneficia cuando ambos actores prospera:

```
PROTOCOLO GANA SI:
? Holders mantienen escasez (no dumpeando)
? Traders circulan capital (liquidez = utility)
? Ambos usan el sistema (adoption crece)

MECANISMO:
Holders protegidos ? Confidence sube ? Adopción crece
Traders activos    ? Volumen sube   ? Liquidez profunda
Ambos              ? Escasez real   ? Valor sube para todos
```

### Equilibrio de Nash (Estrategia Dominante)

En OXIDE, la estrategia dominante es:

- **Para Holders**: Stake siempre (0% riesgo vs. -20% en circulante)
- **Para Traders**: Circular activamente (ganancias > oxidación)
- **Para Protocolo**: Crecer (ambas fuerzas alimentan adoption)

No hay incentivo perverso para manipulación. A diferencia de memecoins (pump & dump) o Bitcoin (hodl speculation), OXIDE alinea todos los intereses hacia ciclos positivos.

---

## Auditoría de Calidad

Evaluación técnica y económica:

| Aspecto | Puntuación | Análisis |
|---------|------------|---------|
| **Originalidad** | 10/10 | No existe implementación similar en Solana usando tiempo como factor de quema integrado |
| **Resistencia a Bots** | 8/10 | Margen de 15 min da aire, pero coste de gas + obligación de "estar limpio" mantiene bots a raya |
| **Justicia (Fairness)** | 10/10 | Reinicio de timestamp para comprador es la clave: nadie hereda deudas ajenas |
| **Seguridad Técnica** | 9/10 | PDAs compartidas + CPI para sincronización es la forma correcta de hacerlo en Anchor |
| **UX (Fricción)** | 8.5/10 | **Mejorado con Atomic Batching**: clear_debt() + withdraw() + transfer() ocurre en una sola transacción. SDKs en wallets modernas (Phantom, Magic Eden) puede abstraer esto. Para usuario: un click transparente. Fricción resuelta sin web. |
| **Liquidez** | 10/10 | Zona Franca permite pools sin fricción, compatible con cualquier DEX |
| **Escalabilidad** | 9/10 | Lazy burn = cálculos on-demand, no requiere cron jobs |

**Puntuación total**: 63/70 = **90% de calidad**

---

## Resumen de Ventajas

### Versus Toda la Competencia

? **Único**: Deflación integrada, matemática, inmutable, on-chain  
? **Justo**: Comprador entra limpio, no hereda deuda  
? **Eficiente**: Zona Franca permite liquidez sin fricción  
? **Transparente**: Completamente auditable, código es ley  
? **Seguro**: PDAs + CPI, no puntos centrales de fallo  
? **Rápido**: Transacciones instantáneas en Solana  
? **Barato**: Costos de blockchain, no intermediarios  

### Desventajas (Honestas)

? **Nuevo**: No tiene 16 años de historia como Bitcoin  
? **Volátil**: Cripto siempre es más volátil que bonos  
? **Adopción inicial**: Requiere educación sobre oxidación temporal  
? **No especulativo**: -20% anual por diseño, no para HODL pasivo  

---

## Proyecciones Económicas

### Supply Decay Esperado

**Modelo Realista**: Vesting dinámico del creador (99.999% stakado) + Oxidación de circulante.

| Período | Volumen Acumulado | Liberado (Vesting 0.1%) | Oxidación (-20%/año) | Circulante Neto | % vs Total |
|---------|-------------------|-------------------------|----------------------|-----------------|------------|
| **Año 0** | 0 | 10,000 | 0 | 10,000 | 0.001% |
| **Año 1** | 50M OXIDE | 50,000 | -10,000 | ~48,000 | 0.0048% |
| **Año 5** | 500M OXIDE | 500,000 | -180,000 | ~330,000 | 0.033% |
| **Año 10** | 2B OXIDE | 2,000,000 | -900,000 | ~1,150,000 | 0.115% |

**Notas**:
- Volumen asumido: Conservador (basado en proyectos similares)
- Oxidación aplicada sobre balance_free promedio
- Creador vende gradualmente tokens liberados (no acumula)
- Airdrops:  OXIDE adicionales (1000 usuarios × 10)

**Cálculo del Vesting**:
```
Tokens liberados = Volumen acumulado × 0.1%

Año 1: 50M OXIDE operados ?  OXIDE liberados
Año 5: 500M OXIDE operados ?  OXIDE liberados
Año 10: 2B OXIDE operados ?  OXIDE liberados

Supply circulante neto = Liberado - Oxidación(20% anual sobre balance_free)
```

### Implicaciones Económicas

| Aspecto | Impacto | Verificable |
|---------|---------|-------------|
| **Escasez Real** | Creador solo puede vender 0.1% del volumen ? Presión de venta limitada matemáticamente | `GlobalState.total_tokens_released` |
| **Velocity Forzada** | Traders activos generan más liberación ? Creador incentivado a promover trading | On-chain observable |
| **Alineación Perfecta** | Sin volumen = 0 liberación ? Creador necesita mercado activo para monetizar | `release_rate_basis_points = 10` |
| **Terminal State** | Año 10+: <0.2% del supply circulando (resto stakado o inexistente) | Proyección conservadora |

**Ventaja vs. Proyectos Tradicionales**: No hay "promesa" de vesting. El código ejecuta restricciones inmutables verificables en tiempo real.

---

## Roadmap

### Vesting Dinámico: Garantía Anti-Rugpull Verificable

**Restricción programática**: El creador tiene  OXIDE stakados (99.999% del supply) que **no puede unstakear manualmente**. Solo se liberan vía vesting automático (0.1% del volumen de trading).

**Verificación on-chain**:
```rust
// lib.rs línea 237 - Función unstake()
require!(
    user.owner != global.authority,
    ErrorCode::CreatorCannotUnstake  // ? Creador bloqueado
);
```

**Auditoría en tiempo real**:
```bash
solana account <GlobalState_PDA>
# Ver: total_tokens_released (contador público)
#      release_rate_basis_points: 10 (0.1% inmutable)
```

**Implicación**: Si el mercado opera 1M OXIDE en un mes, el creador puede vender máximo  OXIDE (0.1%). Sin trading = 0 liberación. Ver [FAQ](#faq-mecanismos-de-control-de-suministro) para detalles.

---

### Fase 1: Genesis (Q1 2026)
- Deploy en Mainnet con auditoría
- **Distribución inicial del creador**:
  -  OXIDE en `balance_free` ? Para crear pool inicial en Raydium/Orca
  -  OXIDE en `balance_staked` ? Bloqueado hasta que haya volumen
- 0 SPL en circulación inicial (todo interno)
- Verificación mint authority
- **Genesis Airdrops (Manual)**: El creador distribuye a primeros 1000 usuarios
  - **EXCEPCIÓN A LAS REGLAS**: Único método donde el creador puede liberar manualmente
  - **Límites de seguridad**:
    - Máximo 100 OXD por airdrop (100,000,000 unidades con 6 decimales)
    - Máximo 1000 airdrops lifetime (contador on-chain `genesis_airdrops_given`)
    - **NO respeta cap diario** (diseñado para siembra inicial rápida a early adopters)
    - Solo puede enviar desde `balance_staked` (no puede crear tokens)
  - **Total máximo posible**: 100,000 OXD (100 × 1000) = 0.01% del supply inicial
  - **Propósito**: Distribuir a early adopters en la fase genesis sin restricciones diarias
  - **Rationale**: El cap diario (1% del supply) está diseñado para vesting orgánico basado en volumen.
    Los airdrops genesis son una siembra única e inicial, limitados por cantidad total (100K OXD),
    no por tiempo. Esto permite onboarding rápido de comunidad sin esperar meses.
  - Proceso: Creador llama a `genesis_airdrop(<wallet>, amount)` por cada usuario
  - Objetivo: Crear base inicial de holders sin esperar volumen de mercado

**Ejecución** (vía programa Anchor):
```rust
// lib.rs - Implementación real con límites de seguridad
pub fn genesis_airdrop(ctx: Context<GenesisAirdrop>, amount: u64) -> Result<()> {
    let global = &mut ctx.accounts.global_state;
    let creator = &mut ctx.accounts.creator_account;
    let recipient = &mut ctx.accounts.recipient_account;
    let now = Clock::get()?.unix_timestamp;
    
    // LÍMITE 1: Máximo 1000 airdrops lifetime (hard constraint imposible evadir)
    require!(
        global.genesis_airdrops_given < 1000,
        ErrorCode::GenesisAirdropLimitReached
    );
    
    // LÍMITE 2: Máximo 100 OXD por airdrop (100,000,000 unidades)
    const MAX_AIRDROP_AMOUNT: u64 = 100_000_000; // 100 OXD
    require!(
        amount <= MAX_AIRDROP_AMOUNT,
        ErrorCode::AirdropAmountExceedsLimit
    );
    
    // LÍMITE 3: Verificar que el creador tiene suficiente balance stakado
    require!(
        creator.balance_staked >= amount,
        ErrorCode::InsufficientFunds
    );
    
    // Transferencia: creator.balance_staked → recipient.balance_free
    // NOTA: Esto es una EXCEPCIÓN - única vía donde el creador reduce su stakado
    creator.balance_staked -= amount;
    recipient.balance_free += amount;
    
    // Actualizar contadores globales
    // NOTA: NO actualiza daily_released_amount (excepción al cap diario)
    global.total_tokens_released += amount;
    global.genesis_airdrops_given += 1;  // Irreversible
    
    Ok(())
}
```

**Auditoría on-chain**: 
- `GlobalState.genesis_airdrops_given` es campo público. Si >= 1000 → El programa rechaza nuevas llamadas.
- **Cap diario NO aplica** a genesis_airdrops (campo `daily_released_amount` no se actualiza).
- Cap diario SÍ aplica a vesting dinámico (`release_creator_tokens`) para prevenir wash-trading.
- Máximo teórico absoluto: 100 OXD × 1000 airdrops = 100,000 OXD (0.01% del supply inicial).
- Diseño: Permite siembra inicial rápida sin esperar días/semanas de trading orgánico.

### Fase 2: Crecimiento (Q2-Q4 2026)
- **Vesting Dinámico Activo**: Creador solo puede vender si hay trading
- Volumen objetivo:  OXIDE/semana ? Libera  OXIDE/semana al creador (0.1%)
- Liquidez en Raydium/Orca via tokens liberados gradualmente
- Community building
- **Supply Circulante Esperado**: ~ OXIDE (combinando airdrops + vesting)
- **Supply en Manos del Creador (free)**: ~ OXIDE (vendido gradualmente)

### Fase 3: Maduración y Listing (Q1-Q2 2027)
- Listing en CEX tier 1
- Volumen objetivo:  OXIDE/mes ? Libera  OXIDE/mes
- Market making profesional
- **Supply Circulante**: ~ OXIDE
- **Supply Liberado del Creador (acumulado)**: ~ OXIDE
- **Porcentaje stakado del creador**: ~85% del supply original (850,000,000,000,000)

### Fase 4: Economía Terminal (2028+)
- Vesting dinámico continúa indefinidamente (mientras haya volumen)
- Deflación natural + ventas del creador combinadas
- Supply neto decreciente (oxidación > liberación)
- Economía completamente descentralizada
- **Proyección**: Creador habrá vendido ~40% del supply, resto sigue stakado u oxidado

---

## FAQ: Mecanismos de Control de Suministro

### ¿Cómo funciona el bloqueo del creador?

**Pregunta**: ¿El creador puede vender todo su suministro?

**Respuesta**: **NO**. El creador tiene:
-  OXIDE en `balance_free` (puede mover libremente para crear liquidez inicial)
-  OXIDE en `balance_staked` (bloqueado)

**Restricción programática**:
```rust
// Función unstake() en lib.rs
require!(
    user.owner != global.authority,
    ErrorCode::CreatorCannotUnstake
);
```

Si el creador intenta llamar a `unstake()`, la transacción **revierte con error**. No es una promesa, es código inmutable.

---

### ¿Cómo libera el creador sus tokens stakados?

**Única vía: Vesting dinámico (basado en volumen)**

Cada vez que CUALQUIER usuario opera (transfer/withdraw/deposit), el protocolo ejecuta:

```rust
let tokens_to_release = (amount_traded × 10) / 10_000;  // 0.1%
creator.balance_staked -= tokens_to_release;
creator.balance_free += tokens_to_release;
```

**Ejemplo**:
- Usuario A retira  OXIDE ? Creador recibe  OXIDE liberados
- Usuario B transfiere  OXIDE ? Creador recibe  OXIDE liberados
- **Volumen acumulado:  OXIDE ? Creador ha liberado  OXIDE (0.1%)**

**Implicación**: Si el mercado no opera, el creador NO puede vender. Sus intereses están alineados con la actividad del ecosistema.

---

### ¿Cómo funcionan los Genesis Airdrops?

**Pregunta**: ¿Los airdrops son automáticos?

**Respuesta**: **NO**. Son manuales y controlados por el creador.

**Proceso**:
1. Creador identifica usuario elegible (ejemplo: primer comprador en DEX, early supporter)
2. Creador ejecuta: `genesis_airdrop(<wallet_pubkey>, 10_000_000)`
3. El programa verifica:
   - ¿Se han dado menos de 1000 airdrops? ?
   - ¿El creador tiene suficiente `balance_staked`? ?
   - ¿El recipiente es wallet virgen (0 balance)? ?
4. Mueve  OXIDE desde `creator.balance_staked` a `recipient.balance_free` (EXCEPCIÓN al bloqueo)
5. Incrementa contador: `genesis_airdrops_given = 1, 2, ..., 1000`

**Límite**: Solo puede llamarse 1000 veces. Después, el error `GenesisAirdropLimitReached` bloquea nuevas llamadas.

**Cuándo usarlo**:
- **Primeros 100 usuarios**: Marketing, community building
- **Early liquidity providers**: Incentivo para crear pools
- **Testnet participants**: Recompensa por encontrar bugs

**No es obligatorio**: El creador puede optar por NO hacer airdrops y solo usar vesting dinámico.

---

### ¿Qué pasa si el creador no vende sus tokens liberados?

**Oxidación aplicada igual que a todos**:

```rust
// En release_creator_tokens(), ANTES de liberar:
apply_lazy_burn(creator, now)?;
```

Si el creador acumula tokens en `balance_free` y no los vende:
- **Oxidación**: -20% anual sobre `balance_free`
- **Ejemplo**: Libera  OXIDE, espera 1 año sin vender ? Pierde  OXIDE

**Incentivo**: Vender gradualmente o stakear de nuevo (pero si stakea, entra en el mismo vesting dinámico).

---

### ¿Puedo verificar que esto es real?

**Sí. Tres formas**:

1. **Lee el código fuente**:
   - [lib.rs línea 237](c:\Users\th3vil\Desktop\OXIDE\lib.rs) ? Función `unstake()` con constraint
   - [lib.rs línea 451](c:\Users\th3vil\Desktop\OXIDE\lib.rs) ? Función `release_creator_tokens()`

2. **Consulta on-chain** (después del deploy):
   ```bash
   solana account <GlobalState_PDA> --url mainnet-beta
   # Verás: total_tokens_released, genesis_airdrops_given
   ```

3. **Simula transacción**:
   ```bash
   # Intenta unstakear como creador ? Debe fallar
   anchor test --skip-local-validator
   ```

**No requieres confiar. El bytecode deployed es la verdad.**

---

### Diagrama del Sistema Completo

```
+-----------------------------------------------------------------+
¦                 CREADOR (Authority Wallet)                      ¦
+-----------------------------------------------------------------¦
¦  balance_free:  OXIDE                                      ¦
¦  +-> Liquidez inicial (crear pool en DEX)                      ¦
¦  +-> Oxidación: -20% anual si no se vende                      ¦
¦                                                                 ¦
¦  balance_staked:  OXIDE (99.999%)                     ¦
¦  +-> ? Unstake bloqueado por código (línea 243 lib.rs)         ¦
¦  +-> ? Vesting: 0.1% del volumen (automático)                  ¦
¦  +-> ? Airdrops: Máx 1000 ×  OXIDE c/u (manual)               ¦
+-----------------------------------------------------------------+
                              ¦
                              ¦ CADA TRANSACCIÓN
                              ?
+-----------------------------------------------------------------+
¦              release_creator_tokens()                           ¦
¦  Llamada automática en: transfer(), withdraw(), deposit()      ¦
+-----------------------------------------------------------------¦
¦  tokens_to_release = (amount_traded × 10) / 10_000             ¦
¦                                                                 ¦
¦  Ejemplo:                                                       ¦
¦  • Usuario opera  OXIDE                                   ¦
¦  • Protocolo libera:  OXIDE al creador                        ¦
¦  • creator.balance_staked -= 10                                ¦
¦  • creator.balance_free += 10                                  ¦
+-----------------------------------------------------------------+
                              ¦
                              ?
+-----------------------------------------------------------------+
¦                  USUARIOS NORMALES                              ¦
+-----------------------------------------------------------------¦
¦  • Pueden stake/unstake libremente                             ¦
¦  • Reciben airdrops si son primeros 1000                       ¦
¦  • Compran desde pool con timestamp limpio                     ¦
¦  • Oxidación: 20% anual sobre balance_free                     ¦
+-----------------------------------------------------------------+
```

**Resumen en una frase**: El creador solo puede vender si la gente opera, alineando incentivos permanentemente.

---

## Garantías Anti-Rugpull: Matemática, No Promesas

### Restricciones Inmutables Verificables

OXIDE no depende de confianza en el creador. Depende de **restricciones programáticas imposibles de evadir**:

#### 1. Unstake del Creador: Bloqueado por Bytecode

```rust
// lib.rs línea 243 - Función unstake()
pub fn unstake(ctx: Context<Unstake>, amount: u64) -> Result<()> {
    require!(
        user.owner != global.authority,  // ?? HARD CONSTRAINT
        ErrorCode::CreatorCannotUnstake
    );
    // Creador NO puede pasar este punto
}
```

**Qué significa**: Si la wallet del creador intenta llamar `unstake()`, Solana revierte la transacción. **No hay backdoor. No hay multi-sig. Es código inmutable.**

**Auditoría DIY**:
```bash
# Ver el bytecode deployed
solana program dump <PROGRAM_ID> program.so
# Buscar hash: require!(user.owner != global.authority)
# Si coincide con repo público ? Verificado ?
```

---

#### 2. Liberación: Solo por Volumen (No por Tiempo)

```rust
// lib.rs línea 471 - release_creator_tokens()
let tokens_to_release = (amount_traded × 10) / 10_000;  // 0.1%
creator.balance_staked -= tokens_to_release;
creator.balance_free += tokens_to_release;
```

**Qué significa**: Cada transacción ejecuta automáticamente este cálculo. **El creador no "decide" cuándo liberar. El volumen decide.**

**Ejemplo auditado**:
- Día 1:  OXIDE operados ?  OXIDE liberados
- Día 100:  OXIDE operados ?  OXIDE liberados
- **Acumulado visible en**: `GlobalState.total_tokens_released` (campo público)

---

#### 3. Whitelist de AMM: Inmutabilidad Radical vs. Compatibilidad Futura

**Decisión de Diseño Controvertida**: La whitelist de DEXs (Raydium, Orca, Meteora) está **hardcoded** en el bytecode. No hay mecanismo de actualización.

```rust
// hook_lib.rs - amm_whitelist module
pub const RAYDIUM_V4: Pubkey = pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");
pub const ORCA_WHIRLPOOL: Pubkey = pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
pub const METEORA_DLMM: Pubkey = pubkey!("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo");
// FIXED FOREVER - No upgrade path
```

**¿Por qué esta decisión aparentemente "rígida"?**

?? **Inmutabilidad Radical > Conveniencia**

OXIDE prioriza que **NADIE** (incluido el creador) pueda modificar las reglas post-deployment. Esto previene:

1. **Governance Attacks**: Un DAO corrupto no puede añadir DEXs maliciosos a la whitelist
2. **Backdoors "de seguridad"**: No hay upgrade path que permita inyectar lógica nueva
3. **Centralización Progresiva**: Común en protocolos que empiezan descentralizados pero añaden multi-sigs "temporales"

**Trade-off Aceptado**:
- ? **Ganancia**: Código es ley. Las reglas de 2026 son las reglas de 2050.
- ? **Costo**: Si Raydium rota su program ID, OXIDE pierde compatibility con ese DEX.

**Alternativa en ese escenario**:
- Fork del contrato (nuevo deploy con nueva whitelist)
- Migración manual de holders (el mercado decide si vale la pena)
- **OXIDE original sigue existiendo** con reglas originales intactas

**Filosofía**: En cripto, la **inmutabilidad** es más valiosa que la **adaptabilidad**. Los upgrades son vectores de ataque.

---

### Comparación con Proyectos "Upgradeables"

| Aspecto | Proyecto Upgradeable (DAO/Multi-sig) | OXIDE (Immutable) |
|---------|--------------------------------------|------------------|
| **Cambio de Reglas** | Posible via governance o multi-sig | Imposible (requiere redeploy + migración manual) |
| **Whitelist de DEXs** | Actualizable (governance vote) | Fija en bytecode (Raydium, Orca, Meteora) |
| **Vectores de Ataque** | Governance capture, multi-sig compromise | Solo bugs del código original (auditables) |
| **Compatibilidad Futura** | Alta (se adapta a nuevos protocolos) | Baja (solo DEXs whitelisted en 2026) |
| **Garantía de Inmutabilidad** | Baja (reglas pueden cambiar) | Absoluta (código es ley para siempre) |

**Conclusión**: OXIDE elige **Code is Law** sobre **Adaptability**. Es un trade-off filosófico, no un defecto técnico.

---

#### 4. Oxidación Aplicada al Creador (Como Todos)

```rust
// En release_creator_tokens(), ANTES de mover tokens:
apply_lazy_burn(creator, now)?;
```

**Qué significa**: Si el creador acumula tokens en `balance_free` (liberados pero no vendidos), se le aplica la misma oxidación del 20% anual que a todos los demás.

**Impacto**: Creador tiene incentivo de **vender gradualmente**, no acumular especulativamente.

---

#### 4. Tracking Global Auditable

```rust
pub struct GlobalState {
    pub total_tokens_released: u64,  // Contador público de tokens liberados
    pub release_rate_basis_points: u16, // Rate de liberación (10 BP = 0.1%)
    pub genesis_airdrops_given: u16,    // Contador de airdrops (max 1000)
}
```

**Qué significa**: Cualquiera puede consultar on-chain:
- Cuántos tokens se han liberado hasta ahora (`total_tokens_released`)
- Cuántos airdrops se han dado (`genesis_airdrops_given`)
- El rate de liberación está codificado (no puede cambiar sin redeployar el programa completo)

---

### Comparativa: OXIDE vs. Proyectos Tradicionales

| Aspecto | Proyecto Tradicional | OXIDE |
|---------|---------------------|------|
| **Liberación del creador** | "Promesa" de vesting en whitepaper | Hard constraint en código (unstake bloqueado) |
| **Velocidad de venta** | Puede vender 100% en un día si quiere | Solo puede vender lo liberado por volumen de mercado |
| **Incentivos** | Creador vs. holders (conflicto) | Creador necesita volumen para vender (alineado) |
| **Auditoría** | Requiere confiar en multi-sig o governance | Código inmutable en blockchain |
| **Riesgo de rugpull** | Alto (históricamente 70% de proyectos) | Bajo (matemáticamente limitado a 0.1% del volumen) |

---

### ¿Cómo Verificar Esto Tú Mismo?

1. **Lee el código fuente**: `lib.rs` línea 237 (función `unstake` con constraint)
2. **Consulta GlobalState on-chain**: 
   ```
   solana account <GlobalState_PDA> --url mainnet-beta
   ```
3. **Verifica el bytecode**: Compara el hash del código deployed con el repo público
4. **Simula transacciones**: Intenta llamar a `unstake()` con la wallet del creador ? Debe revertir

**No requieres confiar en nadie. El código es la verdad.**

---

## Conclusión

OXIDE es un **protocolo monetario basado en Time-Decay**, no una especulación. Implementa matemáticamente lo que los economistas conocen como "cost of capital" a través de la oxidación programática:

### Para Holders de Largo Plazo

- **Activos deflacionarios de código inmutable (propuesta)**: Deflación programada (20% anual), auditable on-chain, sin governance ni upgrades
- **Diversificación Cripto**: Alternativa a Bitcoin (especulativo) y stablecoins (centralizadas)
- **Presión Deflacionaria Integrada**: -44% supply en 5 años bajo el modelo propuesto; escasez programada, no promesa de rendimiento
- **Sin Riesgo de Política Monetaria**: El algoritmo es la política; no puede cambiar sin fork
- **?? Advertencia**: Volatilidad esperada en early adoption. No es sustituto de stablecoins.

### Para Individuos

- **Ahorros Protegidos**: Mantén OXIDE en stake (0% decay) mientras otros pierden -20% anual en circulante
- **Acceso 24/7**: A diferencia de bonos o metales, tradeable instantáneamente
- **Transparencia Absoluta**: Código abierto, auditable, sin intermediarios
- **Costo de Entry**: Primera transferencia cuesta ~0.002 SOL extra (inicialización de tracking)

### La Propuesta de Valor Final

**Time-Value of Supply**: A diferencia de Bitcoin (que espera escasez futura) o Ethereum (que no tiene límite), OXIDE ejecuta deflación *ahora*. Todos los dólares, yenes y euros pierden poder adquisitivo hoy. OXIDE es el único sistema que lo previene programáticamente, inmediatamente, sin depender de creencia futura.

**Nota de Transparencia**: OXIDE es experimental. Su éxito depende de adoption, estabilidad técnica y que el mercado valore escasez matemática sobre volatilidad a corto plazo.

---

## Disclaimer

OXIDE es un proyecto experimental. La deflación del 20% anual es agresiva y puede no ser adecuada para todos los inversores. Este documento no constituye asesoramiento financiero. Haz tu propia investigación (DYOR). El riesgo de adopción es alto; únicamente invierte lo que puedas perder.

