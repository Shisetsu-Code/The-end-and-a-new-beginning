# Evidencia necesaria para ampliar la cobertura

Para retomar, empezar por [HANDOFF.md](../../docs/HANDOFF.md). Este documento identifica contratos/materiales aún necesarios; no es una lista de tareas inconclusas de la entrega `f550c02`. Los ZIP ya recibidos y analizados se identifican por nombre/hash en [PROVENANCE.md](PROVENANCE.md), aunque no estén en Git.

Revisión del repositorio, el traspaso del 14 de noviembre, la entrega inicial y los paquetes ampliados `ENTREGA-CLAUDE(2).zip`, `evidencia-claude-20261115.zip` y `Playtech evidence(1).zip` del 15 de noviembre de 2026. El ZIP independiente de evidencia es un subconjunto idéntico del paquete ampliado; no se contabiliza dos veces. La infraestructura admite todos los proveedores con cupos independientes por salida. Los modos que tienen un protocolo implementado se enumeran en [COVERAGE.md](COVERAGE.md); cada combinación pendiente está en [coverage.json](coverage.json).

La nueva entrega contiene clientes que llaman a demos oficiales y también simuladores locales. Se portaron los comandos de los clientes cuando sus parámetros y continuaciones están documentados. Las respuestas fabricadas por un simulador, los conteos históricos y un indicador de conciliación no establecen por sí solos un protocolo nativo.

## Qué se recuperó de la nueva entrega

| Proveedor | Material aprovechado y alcance |
|---|---|
| Mascot | Capturas de init y request/response: 22 compras y nueve modos normales de keno, bingo, ruleta, Plinball y videopóquer. Quedan implementados los 211 modos identificados de 126 juegos. La descripción anterior de «31 juegos sin binding» era incorrecta: eran 31 modos. |
| Amusnet | `mocks/amusnet/harvest.py`: 7 Fire Dice y Ocean Dice, normal y compra, con continuaciones MOVE/BONUS_PICK. Quedan implementados los 763 modos identificados. |
| 1spin4win | Inventario y `harvest.py`: siete juegos, 17 modos adicionales, líneas tomadas del init actual y selectores exactos de compra/ante. Queda un antebet sin selector. |
| Kendoo | Códigos nativos de 33 juegos, enlace oficial e init nuevos, menús actuales y continuaciones de `compare.py`. Se incorporan normales, los dos antes documentados y las compras de Coin Dragon; 40 modos en total. |
| Betsoft | `harvest.py` contiene PLACEBET y variantes legacy de compra/boost, además de DOBONUS/COLLAPSE. La selección utiliza la oferta nativa actual y conserva los intentos históricamente fallidos que tienen parámetros conocidos. Quedan 380 modos implementados. |
| IronDog | Inventario de 17 IDs y cliente de IntegratedSlots con gateway cifrado, oferta actual, token y `betRecord`: 61 modos, más los tres Morphic anteriores. Se agregan al catálogo 22 pares nativos que éste no enumeraba: cuatro juegos nuevos y dos antebets de juegos existentes, con sus IDs exactos y sin aliases. |
| RubyPlay | Capturas completas de 179 juegos, 255 modos, continuaciones y elecciones observadas; binarios íntegros y controles HTTP exactos. Ver límites concretos debajo. |

Estos conectores comparten el mismo planificador, WireGuard y formato de captura. No requieren la suite de mocks ni los runners Python al ejecutarse.

En Mascot, los otros tres IDs del registro de 129 juegos son `baccarat`, `blackjack` y `holdem`. Ya figuran en el inventario original sin modos. Sus trazas sólo incluyen init y apuestas/continuaciones rechazadas con `WRONG_PARAMS`; no definen una entrada aceptada y un flujo nativo. Los 211 modos implementados corresponden a los 126 juegos con contrato reconstruible.

## RubyPlay

Hay soporte para **179 juegos y 255 de 262 modos**. Las 179 capturas incluyen 432 rondas cerradas, dos rechazadas y 1491 pares aceptados de solicitud/respuesta. Los binarios base64 y `sessionData` permanecen completos. La ampliación aporta 244 modos sobre los 11 anteriores y corrige el init que legítimamente omite `sessionData`.

Se implementan 179 normales, 51 compras freespin, 17 respin, 4 minispin y 4 select. Las elecciones y continuaciones se restringen a controles observados por juego/modo; una rama desconocida se conserva como parcial. El normal de African Cats mantiene su decodificador específico. No hay decodificador universal de los binarios restantes; esa limitación no impide conservarlos ni reproducirlos opacamente.

Quedan **siete modos**:

- `rp_120 / buy_feature:freespin` y `rp_171 / buy_feature:freespin`: se conoce el comando de entrada, pero las capturas terminaron en rechazo `M-0` sin respuesta aceptada de compra ni continuación. Permanecen disponibles como evidencia parcial en el importador.
- Cinco compras heredadas `ruby_buy` de `rp_65`, `rp_57`, `rp_37`, `rp_26` y `rp_29`: todavía faltan sus contratos nativos completos.

La evidencia nueva se encuentra en `ENTREGA-CLAUDE/2-evidencia-claude/rubyplay/capture/rp_*.json`. No se vuelven a pedir como ausentes esos 179 archivos ya entregados. Para interpretar semántica interna adicional siguen faltando los módulos originales `native_bundle.cjs`, `native_choices.cjs`, `native_contracts.json` y sus fuentes exactas; no son requisitos de exportación/replay de los cuerpos existentes.

## Playtech

El paquete `Playtech evidence(1).zip` sí entrega `playtech_engine/`, los bindings de comandos, 32 modalidades de tres recibos y siete flujos con 47 intercambios de apuesta/continuación. El adaptador Rust incorpora los contratos GPAS que pueden vincularse al inventario, a la fuente actual y a un constructor implementado. La cifra exacta vigente figura en [COVERAGE.md](COVERAGE.md).

Ya están portadas la resolución de fuentes y configuración por ámbito, solicitudes modernas/legacy, selección de costes y multiplicadores legales, continuación nativa, elecciones y respuestas comprimidas. Las reglas financieras y bloqueos globales históricos no se trasladan.

Persisten estos límites:

- **NGM:** faltan loader, registro/perfiles y motores/recursos referenciados, como `ngm-families/profile-loader.cjs`, `profile-worker.cjs` y `profiles/registry.json`. Los conteos del historial no sustituyen esos archivos.
- **GLU6:** el bridge exacto y la validación de sus pins/versiones no están incluidos.
- **PSI/Extreme Fruits:** faltan las fuentes originales y SDK exactos que requiere el productor; la entrada declara explícitamente que no está completa.
- **Venetian Masks/Ferox:** hay un selector y hash, pero el constructor de la familia no está demostrado para ese juego. El caso especial de Cash Reveal no permite extrapolarlo.
- **Modos y variantes restantes:** UUID no coincidentes, productores incompletos y dominios de elección/continuación ausentes deben conservarse como pendientes.

Las siete trazas sirven para probar 47 comandos enviados, incluidas 25 continuaciones. **Sus respuestas no sirven como banco de replay completo:** los 19 payloads zlib de Age of the Dogs fueron redactados y los 28 playStack de los otros juegos fueron abreviados. Los 986 recibos describen pasos; no contienen 986 cuerpos completos. El runner guarda íntegros los cuerpos de las capturas nuevas.

## Kalamba

Quedan implementados **los 66 modos normales identificados**, mediante el constructor de `kalamba_client.py` y las respuestas de apertura/giro en `kalamba-log.jsonl`. Los 65 pendientes anteriores tienen la misma secuencia nativa revisada y cierre BaseGame. El log no conserva todas las solicitudes de giro; los comandos se vinculan al cliente entregado.

Las compras y otras ramas no incluidas en ese inventario necesitan sus comandos completos de entrada/continuación. Los ciclos de free spins observados en Agent51 no aportan por sí solos los argumentos de compra. Un giro que derive a una continuación desconocida se guarda parcial; no se inventa la acción siguiente.

## Otros modos pendientes

| Proveedor | Pendientes | Límite concreto / archivos que recuperar |
|---|---:|---|
| Betsoft | 194 | Familias modernas de 1017/1052, continuadores nativos de los estados de 224/719, gamble/cartas y otras variantes. En 692/818 el cliente histórico cerraba por STATE ausente más BALANCE; no hay captura que pruebe otra señal de cierre. Buscar `betsoft-private/init-*.json`, `betsoft-private/results/`, `client-548.js`, `modern_audit.py` y capturas modernas. El directorio `mocks/betsoft/` ya fue entregado y aprovechado. |
| IronDog | 337 | Launchers y tipos nativos para los IDs restantes, modos no documentados de las familias conocidas y continuadores. Faltan `mocks/irondog/capture/`, el audit de modos `iron-dog-profile-mode-audit-20261104.json` y los perfiles/resultados que cita. El nuevo `build_bank.py` e inventario sí están incluidos; no sustituyen sus archivos de entrada. |
| Kendoo | 36 | Mapeos de compras fuera de Coin Dragon: relación entre modo y `selected_mode` / `buy_spin_type`, y elecciones si las hay. Buscar `kendoo-chat4/catalog.json`, `live/games/`, `normalizer.py` y colectores/contratos de esa familia. Los 33 códigos de juego ya se recuperaron. |
| Microgaming | 48 | Los mismos colectores/catálogos, con `ProductID`, contexto de lanzamiento y comandos nativos de compra. La entrega declara `evidencia: []`; module/client y `buy_declared` no especifican las apuestas. |
| Evoplay | 489 | Claves `native:<hash>` de 20 juegos sin payload ejecutable. Buscar `demo-dashboard/evoplay`, `collection_full_playtech.py` y `playtech_family_replay.py`. No apareció un cliente nuevo que defina esas claves. |
| Spinomenal | 238 | Contratos de ramas observadas, tower, wheel y prerrequisitos. El nuevo `harvest.py` importa `spinomenal-private/templates-private.json`, ausente. Recuperar ese archivo, `client.py`, resultados y `collection_full_misc.py`. |
| NetEnt | 67 | Fuentes/capturas de 39 juegos. El nuevo `build_bank.py` lee `provider-work/netent-root/live/games/*.json` y `catalog-current.json`, ausentes, además de las referencias anteriores a `netent-private-root/campaign/` y `collection_full_node.py`. Los contratos GECO/Horizon/RedTiger reconstruibles ya están portados. |
| BGaming | 21 | Cuatro contratos RPC preparados y un launcher/init válido de PrincessOfSky; detalle abajo. El cliente legacy v2 del nuevo ZIP ya estaba cubierto. |
| Pragmatic | 22 | Veinte compras de tickets de scratchcards y dos `fsp-0`. El mock nuevo cubre `pur-*` / `bl-*`, sin definir esos comandos. Recuperar `collection_full_existing.py` y los contratos de entrada. |
| Habanero | 2 | `LaughingBuddha` y `ZeusDeluxe`, aliases de superbet sin selector nativo demostrado. Una respuesta aceptada con un campo `superBet=1` no demuestra que ese campo cambie el modo; falta el contrato del cliente/servidor que lo vincule. |
| 1spin4win | 1 | `LuckyFoxilianHoldAndWin` / `antebet`: el nuevo inventario no exporta su par selector. Normal y compra de ese juego ya se incorporaron. |
| RedTiger | 1 | Modo ambiguo `101`: el simulador no aporta un selector nativo adicional que lo resuelva. |

Los motivos de cada par juego/modo se conservan en `plan --details`. Un rechazo histórico no excluye un contrato conocido; los datos ausentes no aparecen al cambiar `ready`, `campaign_eligible` ni las antiguas verificaciones financieras.

### Los 21 modos pendientes de BGaming

Veinte requieren estos archivos preparados, además de las fuentes originales a las que apuntan:

```text
provider-work/bgaming-chat4/protocols/KeepersOfTheSecret7rst.rpc-extra-prepared.json
provider-work/bgaming-chat4/protocols/GrandPatron7rst.rpc-extra-prepared.json
provider-work/bgaming-chat4/protocols/ClashofGodsAnubisvsHades.rpc-extra-prepared.json
provider-work/bgaming-chat4/protocols/AztecsClawWildDice.rpc-extra-prepared.json
```

Son 2, 5, 12 y 1 modos respectivamente. Esos contratos contienen `fresh_version_guard`, `fresh_source_pins` y `metadata_source_routes`; las fuentes se referencian bajo `bgaming-private-chat4/research/frontends/<GID>/`. El parser entregado `rpc_source_pins.py` usa esos datos para seleccionar/verificar clientes.

El normal de **PrincessOfSky** sólo conserva URL y un resultado 404. Falta un launcher/init/cliente que pruebe entrada y continuación; el bloqueo es ese contrato ausente, no el fracaso histórico.

## Alcance de Games Global y Yggdrasil

Games Global tiene los dos modos suplementarios ya publicados para Gold Blitz Extreme y 12 Masks. Su «2/2» se refiere a esos selectores identificados, no a la totalidad de su catálogo. El nuevo contrato de GoldBlitzUltimate menciona `PlayBonus1x3`, Double/Guarantee y `FreeSpinOption1..3`, pero remite a respuestas completas y controles XML no entregados, bajo `Tester-Spin-Rust/data/providers/games_global/GoldBlitzUltimate/tests/`. Faltan esos cuerpos y parámetros de etapa para incorporar nuevas acciones.

Yggdrasil conserva sus 301 selectores ya implementados. El CSV de rondas de esta entrega coincide byte a byte con el utilizado antes; no se contabiliza otra vez. Los 18 rechazos históricos con parámetros completos siguen incluidos.

## Cómo ampliar desde aquí

Recuperar los archivos señalados del workspace original conserva el trabajo ya realizado. Con un init, una entrada y las continuaciones documentadas se puede ampliar un adaptador sin modificar el planificador ni volver a introducir comprobaciones de saldo. [PROVENANCE.md](PROVENANCE.md) identifica qué fuentes sustentan los bindings actuales.
