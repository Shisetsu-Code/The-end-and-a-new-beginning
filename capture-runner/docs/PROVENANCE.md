# Procedencia y alcance

## Repositorio de ejecución

El proyecto se publica en [The-end-and-a-new-beginning](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning), destino indicado por el usuario. Se importa el runner verificado en `Standalone@02176d12635a18174e7231bf3cbe7bc61c07073b`, con todos sus bindings, tests y documentación.

La migración quedó publicada en `6d814f8`; la ampliación Ruby/Playtech/Kalamba y el banco/replay en `f550c02`. El [traspaso vigente](../../docs/HANDOFF.md) permite retomar sin leer toda la historia. Esta página conserva la trazabilidad de cada entrega; las cifras de una etapa anterior no sustituyen a [COVERAGE.md](COVERAGE.md).

Este repositorio incluye los 105 archivos de inventario/plan/manifiesto que usa el cargador y las tres fuentes adicionales de los tests de ELK/Yggdrasil. Sus bytes originales permanecen intactos. Las referencias históricas a `standalone info/*/codigo`, `game-runner/`, `bet-collector/` y los demás archivos de fuente se resuelven en el repositorio de origen y commit indicados; no son dependencias de compilación ni ejecución. `docs/IMPORTED-FILES.json` registra los hashes de la importación inicial, antes de las modificaciones posteriores.

## Base publicada

El punto de partida es [Standalone, commit 749ae8e4ca8e50326cc756fda8efb916c47b0be8](https://github.com/Shisetsu-Code/Standalone/tree/749ae8e4ca8e50326cc756fda8efb916c47b0be8).

Se preserva `standalone info/` sin modificaciones. Contiene 35 inventarios, 9.874 registros de juegos y 20.066 modos. El cargador verifica los hashes de inventario y plan contra el manifiesto de cada proveedor. El inventario individual publicado de Games Global aporta dos selectores. El nuevo suplemento IronDog añade otros 22 pares: 20 en cuatro juegos nuevos y dos antebets en juegos existentes. La unión utilizada por el runner tiene 9.878 juegos y 20.090 modos; su fingerprint incluye el hash del suplemento, sin modificar ni omitir la comprobación de los manifiestos originales.

La fuente de los protocolos son las copias de `standalone info/*/codigo`, los adaptadores de referencia de `game-runner/src/adapters`, los módulos portables de `bet-collector/game-runner/src` y los protocolos Python completos conservados dentro de `referencias`. Se reconstruyeron los conectores sobre un transporte y un motor independientes. No existe dependencia de ejecución con los crates anteriores, su `Store`, sus recibos financieros o la suite Python externa.

Los 44 bindings utilizados al compilar están incluidos en este crate. Se conserva la procedencia histórica y se amplía `rubyplay-extra.json`; se agregan `kalamba-normal.json` y tres bindings de Playtech. [binding-provenance.json](binding-provenance.json) identifica copias exactas, derivaciones, archivos fuente y sus hashes. Las fuentes con `source_archive` usan el nombre exacto del miembro del ZIP, incluido su prefijo. Las fuentes históricas sin ese campo proceden del repositorio/commit base indicado arriba. Los fixtures nuevos de RubyPlay conservan los mensajes nativos completos con claves previamente redactadas; los de Playtech sólo contienen comandos porque las respuestas de origen fueron recortadas.

```bash
sha256sum -c capture-runner/docs/BINDINGS.sha256
```

Las fórmulas que permanecen en los conectores sirven para formar parámetros nativos de solicitudes. No se exige que un saldo concilie para aceptar un cierre o continuar otro juego.

## Traspaso aportado el 14 de noviembre de 2026

Archivo: `traspaso-claude-20261114.zip`.

SHA-256: `fbec4dffd86c4df04dc7a7f9ece4f4efd922b61d74afd194384e1d8679836a9f`.

Sus 15 entradas de manifiesto coinciden en hash y tamaño. Aporta protocolo y resultados agregados de Habanero, Mascot y Yggdrasil. Se utiliza como evidencia adicional y para construir pruebas sin sesiones reales; el ZIP y los históricos privados no se incorporan al repositorio.

- **Habanero:** `PROTOCOL.md`, las pasadas y el plan de 475 operaciones sustentan elecciones y continuaciones adicionales. La aceptación depende del cierre nativo; los flags históricos de saldo no se convierten en bloqueos.
- **Mascot:** los 129 registros comprenden 117 slots ya identificados por los bindings y 12 juegos de otros tipos. Los resultados ayudan a verificar acciones y cierres; no se inventan selectores para los juegos restantes.
- **Yggdrasil:** el checkpoint especifica la construcción de `play` y `cont`. Su unión con el CSV publicado permite reconstruir los 301 selectores únicos: 138 normales, 135 compras y 28 antebets. Se deduplican cuatro observaciones equivalentes. Se incluyen los 18 intentos históricamente rechazados porque sus parámetros están completos: el resultado anterior es metadata, no un bloqueo. La oferta actual se valida antes de enviar; un rechazo se captura como `provider_error` y una continuación nueva como `unknown_event`.

El traspaso no contiene los decodificadores por juego de RubyPlay. Tampoco entrega los archivos de resultados de Microgaming o Games Global que su informe menciona como pendientes de localizar.

## Entrega adicional `ENTREGA-CLAUDE.zip`

SHA-256: `1a5d5ab725b0b65fb92b0d56406758a4c62237a46ae04f87fb72e5473bb54dd4`.

El ZIP ocupa 14.679.600 bytes; sus 716 archivos suman 14.475.044 bytes descomprimidos. Para los siete bindings nuevos se verificaron 102 fuentes distintas: 100 miembros de este ZIP, comparados también con su extracción, y dos archivos del commit base del repositorio. El manifiesto registra los hashes y tamaños de los bindings y de cada fuente utilizada. El ZIP, sus informes brutos y las sesiones históricas no se publican en el repositorio; se conservan únicamente los controles derivados y los fixtures reducidos necesarios.

La carpeta `mocks/` reúne materiales de distinto alcance. Sus clientes `harvest.py`, `capture.py` y `compare.py` contienen solicitudes dirigidas a las demos oficiales que permiten reconstruir contratos nativos. Los servidores de simulación, respuestas fabricadas, índices y conteos de bancos sirven como documentación o datos de prueba; por sí solos no prueban que exista un cliente ejecutable ni que una ronda se haya cerrado. Las conciliaciones financieras de los recolectores anteriores tampoco se reutilizan como condición de cierre. Cada conector necesita un estado o marcador nativo positivo; la ausencia de un campo de estado no demuestra una ronda terminada.

| Binding nuevo | Evidencia incorporada y alcance |
|---|---|
| `1spin4win-extra.json` | Selectores explícitos del índice y contrato del recolector: 17 pares juego/modo. Las líneas y apuestas se obtienen del init actual; el antebet respeta el control nativo `sbm`. |
| `amusnet-extra.json` | Contratos de `harvest.py` para cuatro modos de `5031` y `5107`: `BET`, `TURBO_DRAW`, `MOVE`, `BONUS_PICK` y `BONUS_PICK_DECISION`, con los wrappers local y remoto y ofertas de la sesión actual. |
| `betsoft-extra.json` | Lanzamiento FREE y controles de apuestas, compras, boosts y continuaciones documentados por el recolector. La aceptación exige marcadores nativos positivos; los juegos `692` y `818` quedan fuera porque el recolector anterior infería su cierre por la ausencia de `STATE` junto con la presencia de `BALANCE`. El catálogo ejecutable se detalla en [COVERAGE.md](COVERAGE.md). |
| `irondog-integrated.json` | 61 pares nativos de 17 IDs: controles de `IntegratedSlots/game`, lanzamiento FUN, transporte cifrado y continuidad del token y `betRecord`. El cierre procede de `bet.settle`. Se añaden las 22 combinaciones ausentes del catálogo: `8142`, `8143`, `8157`, `8501`, y `ante 102` de `8171`/`8183`; se conservan las etiquetas y los modos existentes. Mantiene los tres modos Morphic anteriores. |
| `kendoo-extra.json` | 33 asociaciones juego/código del índice y comandos de `compare.py`. Antes de apostar se comprueba el enlace oficial de cada juego. Las compras incorporadas usan los IDs nativos `coin_dragon:N` ofrecidos en la sesión; otras etiquetas de compra no se extrapolan. |
| `mascot-extra.json` | 84 fuentes de payloads sustentan 31 modos adicionales: 22 compras y nueve normales de mesas y otros juegos no slots. Incluye 31 cadenas de control reducidas, con 330 pasos, sin exigir balances ni premios para aceptar el cierre. |
| `rubyplay-extra.json` | Ocho asociaciones exactas de juego/versión para normales y las compras `freespin` documentadas de `rp_102` y `rp_103`. Proceden del recolector HTTP, los ejemplos del informe y `open_demo_session.py`; no contienen sesiones históricas. |

## Paquetes ampliados del 15 de noviembre

| Paquete | SHA-256 | Alcance |
|---|---|---|
| `ENTREGA-CLAUDE(2).zip` | `6990e6ff5e85430f2d83f9582c327810ceb9636510e3abd41fcda736553d882e` | 932 archivos; incluye 179 capturas RubyPlay y log/cliente Kalamba |
| `evidencia-claude-20261115.zip` | `05c4e92752ca5586ea451b2ff6c617a670b5f78f5a7008becccfe493140ee7db` | 484 archivos, subconjunto byte a byte del paquete anterior |
| `Playtech evidence(1).zip` | `c8f746cf9722633cb7cce8d72ccb6511e4a87289966a2562be23bb513e09ef54` | 53 archivos; motores de interacción GPAS, bindings, recibos y flujos saneados |

Los archivos originales de inventario y evidencia no se modifican. Los ZIP completos no son dependencias del ejecutable.

### RubyPlay: 255 modos y cuerpos nativos completos

`rubyplay-extra.json` incorpora 179 parejas exactas de identidad/versión, las compras observadas, continuaciones por modo y secuencias de elección. Cada captura fuente lleva SHA-256. Los 255 pares cerrados comprenden 179 normales, 51 freespin, 17 respin, 4 minispin y 4 select. Las compras rechazadas de rp_120/rp_171 y cinco modos ruby_buy sin contrato quedan pendientes. El normal original de African Cats sigue usando `rubyplay-rp47-source-index.json` y su decodificador.

Las respuestas de init legítimamente no incluyen `sessionData`; las respuestas no terminales sí deben conservarlo. Se verifican identidad, versión, `an`, `next_action` y elecciones antes de continuar. El binario restante permanece opaco e íntegro: no se calculan premios para aceptar una captura.

El fixture `tests/fixtures/rubyplay/native-complete.json` contiene 13 archivos fuente completos, con sus hashes, cubriendo acciones y elecciones. Dos pruebas adicionales verifican los 179 originales: `RUBYPLAY_EVIDENCE_DIR` para el adaptador y `RUBYPLAY_CAPTURE_DIR` para el importador/replay. Se comprobaron 432 rondas cerradas y 1491 pares nativos. El importador conserva los 434 intentos, incluidas las dos compras rechazadas sin pares de acción reproducibles, con sus contextos y diferencias de formato respecto de una captura HTTP cruda. Los comandos están en [VERIFICATION.md](VERIFICATION.md).

### Kalamba: 66 normales con cierre observado

`kalamba-normal.json` deriva de `kalamba_client.py`, `kalamba-log.jsonl` y el inventario original. Incluye identidad, ubicación/hash de las líneas de apertura e inicio/giro, y proyecciones de los controles nativos que permiten validarlos offline. El cliente define el constructor de la solicitud; el log conserva las respuestas. No se presenta como un log bidireccional completo de apuestas.

Los 66 juegos del inventario con modo normal muestran apertura idle, primera secuencia/round 1 y retorno BaseGame. El conector valida oferta/identidad, apuesta nativa y cierre. Las ramas no documentadas quedan parciales.

### Playtech: contratos GPAS portados, replay histórico limitado

`playtech-native.json` es copia byte a byte del binding entregado. `playtech-receipts.json` proyecta los 32 UUID/nombres/tipos de tres recibos con sus hashes. `playtech-supported.json` cruza UUID y tipo con el inventario y excluye productores/familias no implementados. La cifra vigente se genera desde el adaptador en [COVERAGE.md](COVERAGE.md).

Se portaron a Rust configuración por ámbito, carga estática de fuentes/JMP/JMM, constructores modernos/legacy, dominios de coste/elección, continuaciones y zlib. Las fuentes nuevas se descargan desde el directorio validado del juego y se comprueban los hashes cuando el binding los exige. No se ejecutan los scripts Python ni el JavaScript descargado.

Los siete flujos aportan 47 comandos enviados, de los cuales 25 son continuaciones. Sus respuestas están saneadas: 19 payloads comprimidos redactados y 28 playStack truncados. Los 986 recibos no son 986 partidas completas para replay. Estas limitaciones no se ocultan ni se rellenan con respuestas simuladas. NGM, GLU6, PSI y constructores específicos ausentes siguen pendientes.

### Banco y reproducción

El formato local copia el diario y metadatos existentes con SHA-256, conserva parciales y valida los archivos antes de reproducir. El importador Ruby registra mensajes como JSON nativo procesado y marca sus tiempos como observaciones de importación. No fabrica wire HTTP, sesiones nuevas ni resultados aleatorios. [REPLAY.md](REPLAY.md) detalla la correspondencia estricta y los límites del cliente visual.

El banco completo `rubyplay-capture-bank-20261115.zip` se generó a partir de los 179 miembros `ENTREGA-CLAUDE/2-evidencia-claude/rubyplay/capture/rp_*.json` del paquete ampliado y se entregó fuera de Git. Su hash e instrucciones de reconstrucción se registran en [REPLAY.md](REPLAY.md); no es necesario para compilar ni para la suite normal.

## Cobertura ejecutable

La tabla de conectores es única para planificación y ejecución. `plan --details` enumera cada modo sin implementación disponible. Los nombres `ready`, `campaign_eligible`, `verified` y los conteos históricos no amplían esa cobertura por sí solos.

NoLimit reconstruye los 1.147 selectores de sus 141 juegos y sus contratos de elección publicados. ELK reconstruye los 803 selectores de 159 fichas, con ECF clásico/moderno, variantes y progreso nativo Boss. BGaming incorpora legacy v2/v1, variantes, elecciones y las familias RPC/Cocos reconstruibles: 787 modos de 215 juegos. Comprueba la fuente y el menú activo antes de enviar la apuesta. Play’n GO agrega las 18 mesas y juegos clásicos cuyos contratos Python sí estaban publicados. Las pruebas de los selectores usan inicializaciones y contratos de la evidencia.

RubyPlay combina el decodificador de African Cats (`rp_47`) con los contratos HTTP y las elecciones observadas descritos arriba. Playtech incorpora el motor de interacción GPAS portado; las familias y productores incompletos permanecen pendientes. [MISSING-EVIDENCE.md](MISSING-EVIDENCE.md) lista rutas concretas y diferencia los archivos ausentes de las familias aún no portadas. No se extrapola un selector o decodificador a otro juego por pertenecer al mismo proveedor.

Una captura local clasificada como `complete` significa que el conector reconoció el cierre nativo observado. Las pruebas automatizadas validan parsers, transiciones, journal, planificación y aislamiento de procesos con fixtures locales. La disponibilidad actual de los servidores, los cambios de JavaScript y la conectividad de túneles reales se verifican al ejecutar en la VM.

## WireGuard

La red sigue el modelo oficial de [WireGuard y namespaces](https://www.wireguard.com/netns/): crear la interfaz en el namespace original y moverla al del worker conserva el socket UDP en el original. Las rutas del worker sólo incluyen su interfaz WireGuard y loopback. Los detalles de instalación se basan en la [guía oficial](https://www.wireguard.com/quickstart/).

Las claves son archivos operativos externos, nunca bindings del repositorio. Las pruebas de setup, rollback y cierre usan comandos simulados y no requieren peers reales.
