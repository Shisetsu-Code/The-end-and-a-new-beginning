# Adjuntos históricos: recepción y recuperación de evidencia

Revisión del **16 de noviembre de 2026**, posterior a la [campaña de diez juegos por proveedor](COMPLETE-ROUNDS-10-2026-11-15.md). Los seis adjuntos contienen cinco contenidos distintos: los dos README son idénticos. Se analizaron como datos, sin ejecutar el indexador ni modificar los originales.

## Qué aportan

**Permiten localizar capturas y contratos históricos, pero no incorporan sus mensajes HTTP/WS.** El [manifiesto de recuperación](evidence-recovery-2026-11-16.json) conserva hashes de los adjuntos, cruces por proveedor/juego/modo, IDs de intentos y rutas/pointers de los archivos externos. No hace falta repetir el análisis completo para recuperar un caso.

| Fuente | Contenido comprobado | Límite |
|---|---|---|
| `inventory.sqlite` | 36 corridas, 47555 filas de casos, **16806 intentos**, 34 proveedores con intentos; 17800 combinaciones planificadas distintas | Resultados resumidos, planes y selectores. Sin transcripciones HTTP/WS ni rutas internas de journals |
| `historical-integrated.json` | Generado el **11/11/2026**; **8896 juegos**, 30 proveedores principales y dos suplementos —Mascot/Yggdrasil— | Referencias, hashes, checks y descriptores de funciones; no contiene el código de esas funciones ni mensajes nativos |
| `cobertura.md` | Describe un corte del índice del 16/11 a las 09:58:42 UTC con **14387 intentos** | Sus cifras no son las de la SQLite adjunta |
| `indexar.py` y README | Documentan cómo construir/consultar `evidencia-dashboard/indice.sqlite` y localizar originales | El índice de **853 MiB** descrito en la documentación no fue adjuntado |

La SQLite adjunta pasó `PRAGMA quick_check`: `ok`. Sus estados son **15206 completed, 1592 needs_review y 8 not_sent**. No hay filas running en esta copia. La tabla `provider_timing` tiene 23 filas, una métrica distinta del número de proveedores con intentos.

El histórico declara **8024 juegos completed**, de los cuales **1656 no tienen replay verified**. Los campos de estado del catálogo, el resultado del recolector y el cierre nativo del runner son conceptos distintos. El recolector antiguo también utilizaba conciliación financiera; ese requisito no se traslada al runner actual.

El histórico representa 32 identidades de proveedor y no contiene Games Global, Microgaming ni Hardfire/Kalamba, siquiera como suplemento. `cobertura.md` enumera 36 etiquetas porque incluye `kalamba` y `hardfire-kalamba`; esto **no cambia los 35 proveedores registrados en el runner**. No reemplazar el catálogo actual por este histórico parcial.

## Cruce con los 109 parciales actuales

El cruce SQLite es **exacto por proveedor, juego y modo**:

| Proveedor | Parciales actuales | Con especificación | Con intentos históricos | Con algún completed histórico |
|---|---:|---:|---:|---:|
| betsoft | 1 | 1 | 1 | 1 |
| elk | 62 | 26 | 26 | 26 |
| endorphina | 1 | 1 | 0 | 0 |
| hacksaw | 1 | 0 | 0 | 0 |
| hardfire-kalamba | 6 | 6 | 6 | 0 |
| peterandsons | 1 | 0 | 0 | 0 |
| playngo | 4 | 4 | 2 | 2 |
| pragmatic | 4 | 4 | 1 | 1 |
| reelkingdom | 2 | 0 | 0 | 0 |
| rubyplay | 4 | 1 | 1 | 1 |
| smartsoft | 3 | 3 | 3 | 2 |
| spinomenal | 4 | 4 | 1 | 1 |
| yggdrasil | 16 | 16 | 16 | 15 |
| **Total** | **109** | **66** | **57** | **49** |

Los 57 pares intentados suman 71 registros: 56 completed y 15 needs_review. Los 56 completados tienen `closed=true`, pero sus transcripciones son necesarias para comprobar qué acciones recorrieron. Sólo dos completados exactos son posteriores a la campaña actual: ELK `wild-toro-2/DEF` y `kaiju/DEF`, el 16/11 aproximadamente a las 07:05 y 07:10 UTC−3; los timestamps exactos están en el manifiesto.

El JSON histórico usa otra fuente y semántica: **80/109 modalidades aparecen covered/replay verified**, siete aparecen not_verified y 22 no tienen fila principal —16 Yggdrasil y seis Kalamba—. Los 80 no son 80 fallos resueltos ni deben sumarse a los 49 de SQLite.

## Evidencia prioritaria localizada

| Caso | Referencias disponibles | Qué falta comprobar |
|---|---|---|
| Ruby `rp_103` | `/games/5565` del histórico; binding y 50 tripletas de solicitud/cuerpo/journal bajo `provider-work/rubyplay-chat2/private/live/rp_103-preparation/` | Si alguna captura contiene la elección `pick` posterior a los freespins |
| Ruby `rp_214` | `/games/5491`; binding y 18 tripletas de compra | Parámetros y cierre de la rama `respin` observada |
| Ruby `rp_92` | `/games/5535`; binding y 11 tripletas | La respuesta posterior a `select` y la evolución del contrato |
| Ruby `rp_4` | `/games/5661`; init/normal y un intento SQLite completed | La rama `select`; el índice no acredita que el normal histórico la recorriera |
| Hacksaw `2670/bonus` | `/games/900`; trazas `2670--buy-0.json`, `2670--buy-1.json`, contratos y protocolo | La elección `fs`/`fs_2_coin`; SQLite sólo registra normal de ese juego |
| Betsoft `945/0` | `/games/7508`; `betsoft-private/results/945-0.json`, init y auditoría con pointers `/rounds/353` a `/rounds/355` | Los mensajes de `BUY_CASCADE_FREE` |
| Yggdrasil `7400` | SQLite tiene BASIC, BUY_BONUS y BUY_RAGNAROK marcados completed | Comando `C`, sus parámetros y cierre; no están en la DB |
| Play’n GO | `753/normal` y `928/normal` tienen completed; `476` y `568` sólo especificación | Cliente/parámetros y journals de las rondas respectivas |
| ELK | 26 pares exactos con completed; trazas de Nitropolis/Pirots referenciadas | Comparar las solicitudes y sesiones aceptadas con los HTTP 500 actuales |

Ruby tiene además referencias a `native_choices.cjs` y `native_contracts.json`. Las rutas completas, hashes declarados y pointers están en `recovery.targeted_sources`. Las compras pendientes de `rp_103`, `rp_214` y `rp_92` no aparecen como casos exactos de la SQLite adjunta: las referencias anteriores provienen del JSON histórico.

El suplemento Yggdrasil sólo remite a `yggdrasil-checkpoint-20261107.json`, cuyo hash ya figura en el binding actual. Sus resúmenes no incorporan el comando `C`. Las modalidades pendientes de Pragmatic/Reel Kingdom ya están marcadas no verificadas en el histórico y remiten a cuatro archivos `provider-work/mocks/pragmatic/raw/<game>.jsonl`.

### Correlación ELK por ID nativo, aún provisional

La SQLite llama `pirots-4-inferno` al ID nativo **10234**. El binding actual asigna ese ID a `pirots-4` y `pirots-4-frozen-fury`, con los mismos 18 selectores. Existen 18 modalidades y 20 intentos históricos completed de Inferno, candidatos para investigar 36 pares actuales.

Esta correlación está **separada de los 49 cruces exactos**. Coincidir en ID/selector no acredita que los aliases, versiones, sesiones y secuencias sean intercambiables. No se modificaron nombres ni bindings a partir de esa suposición.

## Recuperación concreta

Se seleccionaron **67 carpetas completed distintas**: 49 por coincidencia exacta y 18 por la correlación ELK provisional. Se separaron **seis carpetas Kalamba needs_review** para diagnóstico. La ruta se deriva de la convención documentada:

```text
bet-collection-private/attempts/<attempt_id>/
```

Ejemplos: Ruby `rp_4/normal` → `59e477d5ce6742b392f4173ee3ee2658`; Yggdrasil `7400/BASIC` → `28b0e6e70155410999d4b69bba9ee218`; Play’n GO `753/normal` → `c6966ef6be684c2297909f432805caa5`.

Hay que recuperar las carpetas con su entrada, contrato histórico, resultado, recibo y diarios nativos originales. El README documenta `transport.jsonl`, `native.json`, `native-full-misc.jsonl`, `urllib-wire.private.jsonl` y variantes por proveedor; **inventory.sqlite no permite saber cuáles existen dentro de cada intento**. La existencia de las carpetas no se ha comprobado en este entorno. Los formatos `ricota/<proveedor>.json` también están referenciados por los documentos, pero no fueron adjuntados.

El manifiesto enumera además **373 combinaciones de archivo externo y hash declarado** para doce juegos prioritarios. `matching_checkout_paths` permite reutilizar los que ya estén presentes con el hash esperado. Los demás hashes son referencias de versión; no se presentan como verificación de un archivo ausente.

Consultas acotadas desde la raíz del repositorio, con `jq`:

```bash
jq '.recovery.preferred_completed_attempts[] | select(.provider == "rubyplay" or .provider == "yggdrasil")' docs/evidence-recovery-2026-11-16.json
jq '.recovery.targeted_sources[] | select(.provider == "rubyplay" and .game == "rp_103")' docs/evidence-recovery-2026-11-16.json
jq -r '.recovery.preferred_completed_attempts[].derived_relative_directory' docs/evidence-recovery-2026-11-16.json
```

No publicar las transcripciones, claves o sesiones en Git. Una vez recibidos los originales, comprobar su hash y contenido, identificar la rama concreta y añadir su contrato/verificación. La presencia de un resumen completed por sí sola no habilita una continuación.

## Identidad de los adjuntos

| Archivo | Bytes | SHA-256 |
|---|---:|---|
| `README(1).md` | 14918 | `47fcfa50ef8d0d615c3234442e5f5e4b5d92bca902d76f280427e85ecc63b462` |
| `README(2).md` | 14918 | `47fcfa50ef8d0d615c3234442e5f5e4b5d92bca902d76f280427e85ecc63b462` |
| `cobertura.md` | 18668 | `8ebb250b9b8ca41753ad5179ea846a68c6cf6c16a67fe097043f766f1fa25c5e` |
| `historical-integrated.json` | 89629924 | `6fe19ac41e9a563f431a97e9837878644cce3b1313312361df19ac810b1c5552` |
| `indexar.py` | 23758 | `f8db3fd0dc9af231243165f29498b58fa09640a87928555fd12e608428612213` |
| `inventory.sqlite` | 93741056 | `84e8cbf8fccf87b22e4ce3e247b9f1c47b3ef3a63d565f848c8287952b762058` |

## Estado al cerrar esta revisión

La revisión produjo el mapa de recuperación y actualizó el traspaso. **No añadió capturas, contratos ni cierres al runner.** Siguen vigentes los resultados 473 completos/109 parciales de la campaña y los 35 proveedores registrados. Los originales adjuntos y el banco privado entregado permanecen sin modificar; el script `indexar.py` fue leído, no ejecutado.
