# Evidencia necesaria para ampliar la cobertura

Revisión del repositorio, el traspaso del 14 de noviembre y `ENTREGA-CLAUDE.zip` / `CLAUDE-COMPLETO.md` del 15 de noviembre de 2026. La infraestructura admite todos los proveedores con cupos independientes por salida. Los modos que tienen un protocolo implementado se enumeran en [COVERAGE.md](COVERAGE.md); cada combinación pendiente está en [coverage.json](coverage.json).

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
| RubyPlay | Launcher demo, parejas de versión de protocolo/matemática, `funModeData` y `next_action` de nueve juegos. Hay 11 modos implementados, con el límite de interpretación del binario explicado debajo. |

Estos conectores comparten el mismo planificador, WireGuard y formato de captura. No requieren la suite de mocks ni los runners Python al ejecutarse.

En Mascot, los otros tres IDs del registro de 129 juegos son `baccarat`, `blackjack` y `holdem`. Ya figuran en el inventario original sin modos. Sus trazas sólo incluyen init y apuestas/continuaciones rechazadas con `WRONG_PARAMS`; no definen una entrada aceptada y un flujo nativo. Los 211 modos implementados corresponden a los 126 juegos con contrato reconstruible.

## RubyPlay

Hay soporte para **nueve juegos y 11 modos**: `rp_47` conserva su decodificador específico; se añaden normales de `rp_1`, `rp_102`, `rp_103`, `rp_106`, `rp_109`, `rp_112`, `rp_114` y `rp_222`, y las compras documentadas de `rp_102` / `rp_103`.

Los ocho juegos añadidos usan el cliente demo de `mocks/rubyplay/capture.py` y las versiones exactas de `4-analisis-otros-proveedores/rubyplay.md`. Cada intento crea una sesión nueva, conserva el binario completo y sigue únicamente `freespin`, `respin` y `minispin` cuando la respuesta los pide. Una acción de elección sin contrato, un cambio de versión o un control ausente genera `unknown_event`.

**Límite:** el binario de esos ocho juegos se conserva como contenido opaco. Se detectan cambios del sobre, versión, secuencia y acción siguiente; no se detecta una semántica nueva escondida exclusivamente dentro del binario si esos controles permanecen iguales. Esto no afecta al guardado de los bytes recibidos. No hace falta calcular premios ni conciliar saldos para recolectarlos.

Para ampliar los **251 modos restantes**, recuperar primero los bindings y las capturas nativas referenciados por los índices:

```text
provider-work/rubyplay-chat2/results/rp_*-binding.json
provider-work/rubyplay-chat2/catalogs/official-links.json
provider-work/mocks/rubyplay/capture/rp_*.json
provider-work/mocks/rubyplay/bank/rp_*.json
```

El índice original cita 178 bindings por juego que no están incluidos. El nuevo informe no entrega ese conjunto; sólo prueba las parejas y acciones usadas en esta ampliación. No se extrapolan las versiones o compras de un juego al resto.

Para interpretar elecciones y eventos internos del binario también faltan estos componentes y las fuentes/fixtures a los que remiten:

```text
provider-work/rubyplay-chat2/native_bundle.cjs
provider-work/rubyplay-chat2/native_choices.cjs
provider-work/rubyplay-chat2/native_contracts.json
provider-work/rubyplay-chat2/validate_next_init.cjs
provider-work/rubyplay-chat2/plan_next_normal.cjs
private/live/<game>-preparation/game-js.body.private
```

El índice `rubyplay-rp47-source-index.json` sólo sirve para la versión exacta del cliente de African Cats. Con cuatro cupos de RubyPlay, ahora pueden ejecutarse cuatro juegos distintos por salida, dentro de los nueve disponibles. `plan` aplica cualquier filtro de selección al calcular ese máximo.

## Playtech

Se conserva el binding completo de Blitz10: **un modo de 3.940 identificados**. Faltan los motores y perfiles referenciados por los índices públicos:

```text
provider-work/playtech-root/playtech_engine/
bet-collector/collection_full_playtech.py
bet-collector/playtech_family_replay.py
bet-collector/collection_families.py
playtech-private-root/ngm-families/profile-loader.cjs
playtech-private-root/ngm-families/profile-worker.cjs
playtech-private-root/ngm-families/profiles/registry.json
```

`playtech_engine/` comprende protocolo, runtime, transporte, fuentes del cliente, command bindings, selectores y continuadores. Para NGM también hacen falta los adaptadores, fuentes y fixtures por perfil. Los UUID de menú y el resumen de 143 perfiles no contienen esos programas ni los mensajes nativos anidados.

El nuevo `mocks/playtech/README.md` documenta respuestas reconstruidas: `_mockReconstructed`, `playStack: []` y formatos propios `_mockOwnFormat` para init/configuración/errores. Sirven para probar ese simulador, pero no prueban que sus mensajes sean aceptados por la demo oficial. El ZIP no aporta el motor genérico ausente. Los históricos de miles de juegos/rondas tampoco sustituyen sus comandos.

## Otros modos pendientes

| Proveedor | Pendientes | Límite concreto / archivos que recuperar |
|---|---:|---|
| Betsoft | 194 | Familias modernas de 1017/1052, continuadores nativos de los estados de 224/719, gamble/cartas y otras variantes. En 692/818 el cliente histórico cerraba por STATE ausente más BALANCE; no hay captura que pruebe otra señal de cierre. Buscar `betsoft-private/init-*.json`, `betsoft-private/results/`, `client-548.js`, `modern_audit.py` y capturas modernas. El directorio `mocks/betsoft/` ya fue entregado y aprovechado. |
| IronDog | 337 | Launchers y tipos nativos para los IDs restantes, modos no documentados de las familias conocidas y continuadores. Faltan `mocks/irondog/capture/`, el audit de modos `iron-dog-profile-mode-audit-20261104.json` y los perfiles/resultados que cita. El nuevo `build_bank.py` e inventario sí están incluidos; no sustituyen sus archivos de entrada. |
| Kendoo | 36 | Mapeos de compras fuera de Coin Dragon: relación entre modo y `selected_mode` / `buy_spin_type`, y elecciones si las hay. Buscar `kendoo-chat4/catalog.json`, `live/games/`, `normalizer.py` y colectores/contratos de esa familia. Los 33 códigos de juego ya se recuperaron. |
| Hardfire/Kalamba | 65 | `collection_claude_extra.py`, `claude-demo-catalog.json` y sus selectores/continuadores. La nueva entrega declara expresamente que no conserva payloads de esta sesión. |
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
