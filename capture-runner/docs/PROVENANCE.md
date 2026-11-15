# Procedencia y alcance

## Repositorio de ejecución

El proyecto se publica en [The-end-and-a-new-beginning](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning), destino indicado por el usuario. Se importa el runner verificado en `Standalone@02176d12635a18174e7231bf3cbe7bc61c07073b`, con todos sus bindings, tests y documentación.

Este repositorio incluye los 105 archivos de inventario/plan/manifiesto que usa el cargador y las tres fuentes adicionales de los tests de ELK/Yggdrasil. Sus bytes originales permanecen intactos. Las referencias históricas a `standalone info/*/codigo`, `game-runner/`, `bet-collector/` y los demás archivos de fuente se resuelven en el repositorio de origen y commit indicados; no son dependencias de compilación ni ejecución. `docs/IMPORTED-FILES.json` registra los hashes de la importación inicial, antes de las modificaciones posteriores.

## Base publicada

El punto de partida es [Standalone, commit 749ae8e4ca8e50326cc756fda8efb916c47b0be8](https://github.com/Shisetsu-Code/Standalone/tree/749ae8e4ca8e50326cc756fda8efb916c47b0be8).

Se preserva `standalone info/` sin modificaciones. Contiene 35 inventarios, 9.874 registros de juegos y 20.066 modos. El cargador verifica los hashes de inventario y plan contra el manifiesto de cada proveedor. El inventario individual publicado de Games Global aporta dos selectores. El nuevo suplemento IronDog añade otros 22 pares: 20 en cuatro juegos nuevos y dos antebets en juegos existentes. La unión utilizada por el runner tiene 9.878 juegos y 20.090 modos; su fingerprint incluye el hash del suplemento, sin modificar ni omitir la comprobación de los manifiestos originales.

La fuente de los protocolos son las copias de `standalone info/*/codigo`, los adaptadores de referencia de `game-runner/src/adapters`, los módulos portables de `bet-collector/game-runner/src` y los protocolos Python completos conservados dentro de `referencias`. Se reconstruyeron los conectores sobre un transporte y un motor independientes. No existe dependencia de ejecución con los crates anteriores, su `Store`, sus recibos financieros o la suite Python externa.

Los 40 bindings públicos utilizados al compilar están incluidos en este crate: se conservan sin cambios los 33 anteriores y se agregan siete derivados de la nueva entrega. [binding-provenance.json](binding-provenance.json) distingue copias exactas y datos derivados, e identifica sus fuentes, tamaño y SHA-256. En las fuentes nuevas, `source_archive` identifica el ZIP y `path` es el nombre exacto del miembro, incluido el prefijo `ENTREGA-CLAUDE/`; sin `source_archive`, la ruta corresponde a la raíz del repositorio de origen `Standalone`, en el commit indicado. Los fixtures derivados omiten sesiones privadas y campos financieros innecesarios. Para comprobar sus bytes, ejecutar desde la raíz:

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

### RubyPlay: sobre HTTP y binario opaco

El índice anterior `rubyplay-rp47-source-index.json` conserva los offsets y la identidad de la fuente exacta del decodificador de African Cats (`rp_47`). La ampliación agrega ocho juegos y diez modos mediante el sobre nativo HTTP: usa las versiones documentadas, arrastra `funModeData`, comprueba `an` y sigue `next_action`. La unión tiene nueve juegos y once modos, incluido el normal original de `rp_47`.

En los ocho juegos nuevos se registra `binary` completo como contenido opaco. El recolector reconoce el cierre cuando el sobre vuelve a `next_action=spin`; no necesita decodificar premios. Los formatos, secuencias o continuaciones desconocidos generan `unknown_event`. Las elecciones `pick` y `select` también detienen la ronda con una alerta, sin probar índices. Un evento nuevo oculto únicamente dentro del binario puede quedar registrado sin clasificarse si los controles externos no cambian; esta ampliación no aporta un decodificador universal ni permite afirmar cobertura de esas novedades internas.

### Playtech: simulación sin nuevos contratos ejecutables

La nueva entrega reconstruye respuestas de simulación a partir de resúmenes: utiliza `_mockReconstructed`, `_mockOwnFormat` y `playStack:[]`. No aporta el motor genérico, los perfiles y las clases nativas de comandos que faltaban. Esos resultados no amplían el binding ejecutable de Blitz10. Las rutas pendientes permanecen documentadas en [MISSING-EVIDENCE.md](MISSING-EVIDENCE.md).

## Cobertura ejecutable

La tabla de conectores es única para planificación y ejecución. `plan --details` enumera cada modo sin implementación disponible. Los nombres `ready`, `campaign_eligible`, `verified` y los conteos históricos no amplían esa cobertura por sí solos.

NoLimit reconstruye los 1.147 selectores de sus 141 juegos y sus contratos de elección publicados. ELK reconstruye los 803 selectores de 159 fichas, con ECF clásico/moderno, variantes y progreso nativo Boss. BGaming incorpora legacy v2/v1, variantes, elecciones y las familias RPC/Cocos reconstruibles: 787 modos de 215 juegos. Comprueba la fuente y el menú activo antes de enviar la apuesta. Play’n GO agrega las 18 mesas y juegos clásicos cuyos contratos Python sí estaban publicados. Las pruebas de los selectores usan inicializaciones y contratos de la evidencia.

RubyPlay combina el decodificador de African Cats (`rp_47`) con los contratos del sobre HTTP descritos arriba; otras versiones y elecciones siguen pendientes cuando no existe evidencia suficiente. Playtech conserva los índices de sus motores genéricos sin los programas correspondientes. [MISSING-EVIDENCE.md](MISSING-EVIDENCE.md) lista rutas concretas y diferencia los archivos ausentes de las familias aún no portadas. No se extrapola un selector o decodificador a otro juego por pertenecer al mismo proveedor.

Una captura local clasificada como `complete` significa que el conector reconoció el cierre nativo observado. Las pruebas automatizadas validan parsers, transiciones, journal, planificación y aislamiento de procesos con fixtures locales. La disponibilidad actual de los servidores, los cambios de JavaScript y la conectividad de túneles reales se verifican al ejecutar en la VM.

## WireGuard

La red sigue el modelo oficial de [WireGuard y namespaces](https://www.wireguard.com/netns/): crear la interfaz en el namespace original y moverla al del worker conserva el socket UDP en el original. Las rutas del worker sólo incluyen su interfaz WireGuard y loopback. Los detalles de instalación se basan en la [guía oficial](https://www.wireguard.com/quickstart/).

Las claves son archivos operativos externos, nunca bindings del repositorio. Las pruebas de setup, rollback y cierre usan comandos simulados y no requieren peers reales.
