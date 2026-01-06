# OXIDE: Time-Decay Monetary Protocol (Solana Token-2022)

Whitepaper institucional: preciso, sin hype, orientado a comités de riesgo, auditores y fondos regulados. No se alteran parámetros económicos ni código.

---

## Parámetros duros (cheatsheet)
- Supply inicial: 1,000,000,000,000,000 OXD (6 decimales); 10k libres, resto staked del creador.
- Función de burn (lazy): $\text{burn}=\text{balance}\_{free}\times0.20\times\tfrac{t}{\text{año}}$ aplicada al interactuar; fracciones acumuladas en `burn_fraction_remainder` (u128).
- Grace técnica: 15 minutos (`elapsed<=900s`); fuera de ventana, `clear_debt()` antes de transferir.
- Stake: 0% decay; volver a libre reinicia reloj en `now`.
- Vesting creador: 0.1% del volumen con cap diario 1% del supply; `unstake` del creador prohibido.
- Whitelist fija: pools Raydium V4, Orca Whirlpool, Meteora DLMM como zona franca.
- Coste de activación: `TrackingAccount` por wallet (~0.002 SOL), pagado por el emisor en la primera recepción.
- Inmutabilidad: sin multisig ni upgrade; cambios requieren nuevo deploy.

---

## Executive Summary (≤1 página)
- **Tesis**: OXIDE es un activo no rendidor cuyo suministro efectivo se contrae exponencialmente sobre saldos libres (decay compuesto 20% anual) y se preserva en stake (0% burn). Incentiva compromiso activo de capital y penaliza ociosidad sin depender de decisiones humanas.
- **Posicionamiento**: instrumento monetario programático, sin yield, sin governance, sin upgrade, no estable. Diseñado para holders de largo plazo y operadores que limpien deuda o actúen dentro de 15m.
- **Mecánica clave**: Token-2022 con Transfer Hook inmutable; tracking por wallet; herencia ponderada de antigüedad; pools whitelisted (Raydium V4, Orca Whirlpool, Meteora DLMM) como zona franca; `clear_debt` requerido si `elapsed>900s`.
- **Vesting anti-rugpull**: creador libera solo 0.1% del volumen con cap diario 1% del supply; `unstake` del creador prohibido; sin multisig ni governance que altere reglas.
- **Seguridad**: hook con una sola lectura de `Clock`; dualidad `UserAccount`/`TrackingAccount` evita desincronías; whitelist hardcoded; riesgos explícitos (dependencia L1, rotación de IDs de DEX, falta de SOL del emisor para activar tracking).
- **Regulatorio (resumen)**: no hay expectation of profit from efforts of others; sin promesa de precio ni yield; burn programático y no discrecional; creador sin capacidad de upgrade.

---

## 1. Tesis monetaria (macro)
- **Problema que aborda**: liquidez ociosa y ausencia de mecanismos programáticos que penalicen acaparamiento improductivo sin inflación discrecional ni reparto de yield centralizado.
- **Herramienta**: decay exponencial (20% anual compuesto) sobre saldos libres; favorece uso activo (stake/operativa) o aceptación de erosión continua por inactividad. No depende de oráculos ni decisiones humanas.
- **Contexto de sentido**: entornos de liquidez abundante que buscan disciplina de stock; carteras que prefieren activos no rendidores con escasez endógena; escenarios de control de velocidad sin autoridad monetaria.
- **Por qué decay exponencial y no inflación/rebase**: aplica solo a capital ocioso, es continuo y compuesto, es simétrico (creador incluido) y no requiere política discrecional.

---

## 2. Posicionamiento del activo
- **Qué es**: instrumento monetario no rendidor, supply-constricting sobre libres, refugio en stake, reglas inmutables.
- **Frase institucional**: "OXIDE is a non-yielding, supply-constricting monetary instrument optimized for long-term holders willing to actively commit capital; rules are immutable and enforcement is on-chain."

### Qué OXIDE NO es (non-goals)
- No es stablecoin.
- No es governance token.
- No es producto de yield ni promesa de retorno.
- No es utility/access token.
- No está optimizado para HFT ni retail de corto plazo.
- No tiene upgrade path ni parámetros modificables post-deploy.

---

## 3. Diseño monetario (time-decay correcto)
- **Ámbito del burn**: solo `balance_free`; excluye `balance_staked` y saldos en pools whitelisted.
- **Decay exponencial (lazy burn)**: $\text{remaining}=\text{balance}\_{free}\times(0.8)^{\text{años}}$; p.ej., 30 días inactivo: $(0.8)^{30/365}\approx0.9831$ = pérdida ~1.69%.
- **Precisión**: factor diario (0.8^(1/365)) + Taylor con ln(0.8) para fracciones < 24h; sin exploits por timing.
- **Sin yield**: no hay reparto de fees ni intereses; la posible apreciación deriva de reducción de unidades.
- **Refugio**: stake = 0% burn; al volver a libre, reloj reinicia en `now`.

---

## 4. Transfer Hook, tracking y ventana de gracia (15m)
- **Validación**: cada transfer SPL pasa por `transfer_hook`; si emisor no-pool y `elapsed>900s`, revierte con `DebtNotCleared` hasta `clear_debt()`.
- **Herencia de antigüedad**: receptor hereda timestamp ponderado; evita lavado de tokens viejos.
- **Zona franca**: compras desde Raydium V4 / Orca Whirlpool / Meteora DLMM asignan timestamp `now` al comprador; si sender es pool whitelisted, no se valida `elapsed`.
- **Inicialización**: `TrackingAccount` en primera recepción; costo ~0.002 SOL, pagado por el emisor; la TX falla si no tiene SOL.
- **Racional 15m**: reduce fricción operativa sin eliminar deuda económica (sigue acumulando sobre saldo libre).
- **⚠️ Limitación delegates**: La whitelist solo funciona para **transfers directos** donde la pool authority es signer. Delegate transfers (ej: Jupiter multi-hop, bots custom) NO están whitelisted y deben llamar `clear_debt()` antes de transferir desde pools. Los DEXs mainstream (Raydium/Orca/Meteora) usan transfers directos para swaps normales. Ver [TECHNICAL.md](TECHNICAL.md#amm-whitelist--delegate-transfers) para detalles.

---

## 5. Stake, depósitos y retiros
- **Stake**: mueve de libre a staked; burn = 0 mientras esté staked.
- **Unstake**: vuelve a libre y reinicia reloj; prohibido para el creador.
- **Deposit (SPL→interno)**: quema SPL 1:1, aplica burn pendiente, sincroniza tracking.
- **Withdraw (interno→SPL)**: aplica burn pendiente, mintea SPL 1:1, sincroniza tracking.
- **clear_debt**: aplica burn pendiente a SPL antes de vender fuera de ventana.

---

## 6. Vesting del creador e inmutabilidad
- **Liberación**: `tokens_to_release = amount_traded × 0.1%`, cap diario 1% del supply total, no más que el balance staked restante.
- **Bloqueo de unstake**: cualquier `unstake` del creador revierte (anti-rugpull).
- **Génesis airdrops (EXCEPCIÓN CONTROLADA)**: 
  - **Único método** donde el creador puede liberar tokens manualmente
  - Límites de seguridad: máx. 100 OXD por airdrop, máx. 1000 airdrops lifetime
  - **NO respeta cap diario** (diseñado para siembra inicial rápida a early adopters)
  - Total máximo posible: 100,000 OXD (0.01% del supply inicial)
  - Propósito: Distribuir a early adopters en la fase genesis sin esperar volumen de trading
  - Consume `balance_staked` del creador; contador público `genesis_airdrops_given`
- **Inmutabilidad**: sin multisig ni governance; whitelist hardcoded; cambios implican nuevo despliegue y migración voluntaria.

---

## 7. Seguridad: amenazas y mitigaciones (vista auditoría)
| Categoría | Riesgo | Mitigación | Evidencia/diseño |
|-----------|--------|------------|------------------|
| Técnica | Exceso de CU en `transfer_hook` | Una `Clock`, sin bucles; CU price/limit ajustable | Hook minimalista |
| Técnica | Rotación de IDs de DEX whitelisted | Whitelist hardcoded; cambios requieren nuevo deploy | Política "código como ley" |
| Técnica | Desincronía SPL ↔ tracking | `clear_debt`/`withdraw` sincronizan; hook hereda timestamps ponderados | Dualidad `UserAccount` + `TrackingAccount` |
| Operacional | Emisor sin SOL para activar receptor | `init_if_needed` cobra al emisor; sin SOL la TX revierte; pre-chequeo en frontends | Costo ~0.002 SOL |
| Operacional | Bloqueo >15m | Ruta `clear_debt + swap` atómica; mensajes claros | Error `DebtNotCleared` |
| Económico | Wash-trading para vesting | Cap diario 1% supply; liberación 0.1% volumen; burn y fees encarecen | Cap on-chain |
| Económico | "Reset" vía micro-transfers | Ponderación de timestamp; ventana no borra deuda; fees > ahorro | Implementado en hook |
| Gobernanza | Cambios arbitrarios | Sin upgrade authority ni governance | Reglas fijas en bytecode |

---

## 8. Modelo económico y escenarios
- **Supply inicial**: 1,000,000,000,000,000 OXD (6 decimales); 10k libres, resto staked del creador.
- **Deflación máxima**: 20% anual sobre `balance_free`; 0% en stake/pools whitelisted.
- **Vesting**: 0.1% del volumen con cap diario 1% supply; visible en `total_tokens_released`.
- **Dinámica**: actividad libera supply; inactividad reduce libres; stake preserva unidades.

### Sensibilidad (intuición)
| Escenario | Volumen semanal | Liberación semanal al creador | Burn anual sobre 1M OXD libres |
|-----------|-----------------|-------------------------------|--------------------------------|
| Bajo | 1M OXD | ~1,000 OXD | ~200k OXD (20%) |
| Medio | 10M OXD | ~10,000 OXD | ~200k OXD (20%) |
| Alto | 100M OXD | ~100,000 OXD (cap lejano) | ~200k OXD (20%) |
| Muy alto | 1B OXD | Cap diario puede activar (≤1% supply/día) | ~200k OXD (20%) |

---

## 9. Externalidades esperadas (2–3 años)
- **Supervivencia de holders**: prevalecen quienes stakean o rotan inventario con disciplina; saldos libres pasivos se erosionan.
- **Comportamiento penalizado**: acaparamiento ocioso en libre; intentos de reset via micro-transfers.
- **Mercado emergente**: mayor proporción del supply en stake o pools whitelisted; flotante libre efectivo cae si la actividad baja.
- **Quién debería evitar OXIDE**: quienes buscan precio estable, yield, o liquidez sin fricción; HFT puro.

---

## 10. Operativa e integración DeFi
- **Wallets/frontends**: detectar receptor sin tracking y mostrar costo estimado; toggle "clear_debt automático" al vender.
- **DEX/agregadores**: empaquetar `clear_debt + swap` si `elapsed>900s`; respetar whitelist (Raydium V4, Orca Whirlpool, Meteora DLMM).
- **Lending/derivados**: colateral en stake para 0% decay; si se retira como SPL, considerar timestamp heredado.
- **Market makers/bots**: cachear contrapartes para minimizar activaciones; rotar wallets no limpia antigüedad.
- **Bridges/custodios**: los tokens quedan lockeados en Solana con su `TrackingAccount`; el tiempo sigue contando. Al redimir, el custodio debe ejecutar `clear_debt()` (y pagar SOL) antes de devolver; el receptor recibe el saldo post-burn. Documentar este flujo; sin SOL o sin `clear_debt` la redención revierte.
- **Perfiles y patrones**:
	- Bots/MM intra-15m: operar desde pools whitelisted para entrar limpios; si inmovilizan >15m, `clear_debt` antes de mover inventario; prefundear SOL para inicializar contrapartes nuevas.
	- DCA recurrente: cada compra desde pool entra con timestamp `now`; stake inmediato para 0% decay mientras acumulas; al vender tras >15m, `clear_debt` primero.
	- Holder largo: mantener en stake; si pasas a libre y superas 15m, limpia deuda antes de vender o re-stakear.
	- Custodios/CEX: programar `clear_debt` periódico sobre la cuenta de custodia; mantener SOL para redenciones y para activar tracking de destinatarios.

---

## 11. Costos, performance y monitoreo
- **Activación**: `TrackingAccount` ~0.00204 SOL, pagado por el emisor en primera recepción.
- **Overhead**: similar a crear un ATA Token-2022; sin rent extra recurrente.
- **CU/fees**: hook con una `Clock`, sin bucles; ajustar CU price/limit en congestión.
- **Operación recomendada**: `clear_debt + swap` atómico.
- **Observabilidad mínima**: `DebtNotCleared`, `TrackingNotInitialized`, CU medio hook, `total_tokens_released`, `genesis_airdrops_given`.

---

## 12. Riesgo regulatorio y cumplimiento
- No hay expectation of profit from the efforts of others: el protocolo no distribuye rendimientos ni fees.
- Burn/decay es programático y no discrecional; no existe autoridad que ajuste parámetros.
- El creador no puede upgrade, cambiar whitelist ni unstakear manualmente.
- No es stablecoin, deuda, equity ni governance.
- Comunicación recomendada: reglas fijas, ausencia de promesas de precio/retorno, carácter experimental.

---

## 13. Guía de pruebas y auditoría técnica
- **Unit**: burn con `burn_fraction_remainder`; herencia ponderada; bloqueo >15m; init tracking.
- **Integration**: `clear_debt + swap` atómico; vesting con cap diario; génesis airdrops (≤1000); `unstake` del creador revertido.
- **Property-based**: no-negatividad de balances; invariantes de supply interno tras burn/vesting; monotonicidad de `total_tokens_released`.
- **Fuzzing (hook)**: pool/no-pool, `elapsed` variable, falta de SOL para tracking.
- **Manual mainnet-beta**: CU en congestión; UX ante `DebtNotCleared`; latencia de `Clock` (~400ms) suficiente para cálculos anuales.

---

## 14. Despliegue y operación segura (resumen)
1) Crear keypairs y fundear SOL (incluida rent de PDAs).
2) Desplegar `oxide`; transferir mint authority al PDA; ejecutar `initialize_global` (10k libres, resto staked creador).
3) Desplegar `oxide_transfer_hook`; correr `initialize_extra_account_meta_list` con whitelist hardcoded.
4) Ejecutar `verify_mint_authority` (requisito para `withdraw`).
5) Aplicar extensión TransferHook al mint Token-2022.
6) Crear pool inicial en DEX whitelisted con los 10k libres.
7) Opcional: génesis airdrops (≤1000) desde `balance_staked` del creador.
8) Publicar IDs de programa/mint y congelar upgrade authority (no-upgrade policy).

---

## 15. FAQ breve
- **¿La deflación es siempre 20%?** No, es un máximo anual proporcional sobre `balance_free`; en stake es 0%.
- **¿Puedo operar sin burn?** <15m minimiza exposición; fuera de ventana, `clear_debt()` o asumir burn proporcional. Stake/depósito pausan el reloj.
- **¿Quién paga la activación?** El emisor, para asegurar tracking desde la primera recepción y prevenir evasión.
- **¿Qué pasa si aparece un DEX nuevo?** No está soportado; requeriría nuevo programa. Whitelist es fija.
- **¿Cómo vende el creador?** Solo lo que libera el mercado (0.1% volumen, cap diario 1% supply); `unstake` del creador revierte siempre.
- **¿Precio de mercado afecta el burn?** No, el burn se calcula sobre unidades.
- **¿Se puede pausar el burn globalmente?** No; reglas fijadas en bytecode.

---

## 16. Disclaimer y límites
OXIDE es experimental. No es asesoramiento financiero. La deflación programada no garantiza preservación de precio en fiat. Existe riesgo de contrato y de la capa 1. Opera solo con fondos que puedas permitirte perder.
