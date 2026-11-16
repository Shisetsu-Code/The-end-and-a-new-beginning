# Retomar el proyecto

**Estado al 16 de noviembre de 2026 UTC —15 por la noche en UTC−3—:** el runner y replay están publicados en `main`. La última campaña amplió la muestra a diez juegos de cada proveedor con cierres previos, usando el binario de `af8f957`; no cambió adaptadores ni cobertura. Comprobar `git status` y `git log -3 --oneline` al retomar. La ampliación original sigue identificada por `f550c02` y la corrección de sesiones por `317fe6f`; conservar los resultados de cada etapa por separado.

## Última entrada: adjuntos históricos del 16 de noviembre

Se revisaron seis adjuntos —cinco contenidos distintos: los dos README son idénticos— para localizar la evidencia de los fallos pendientes. Leer [EVIDENCE-ATTACHMENTS-2026-11-16.md](EVIDENCE-ATTACHMENTS-2026-11-16.md) y consultar por proveedor el [manifiesto de recuperación](evidence-recovery-2026-11-16.json); no volver a cargar íntegros los adjuntos de 90 MB ni el manifiesto en contexto.

- `inventory.sqlite` es la base del recolector: **36 corridas, 47555 filas de casos y 16806 intentos** —15206 completed, 1592 needs_review, 8 not_sent—, con 34 proveedores que tienen intentos. Pasó quick_check. No contiene mensajes HTTP/WS. No es el `indice.sqlite` de 853 MiB descrito en los README, que no se adjuntó.
- `historical-integrated.json`, generado el 11/11, contiene **8896 juegos de 30 proveedores principales y dos suplementos**. Son referencias, hashes y estados históricos; no incluye los payloads ni el código de los callables citados. `cobertura.md` describe otro corte, de 14387 intentos; no mezclar sus cifras con la DB adjunta.
- Cruce con los 109 parciales actuales: SQLite tiene **49 pares exactos con algún completed histórico**; el JSON declara **80 covered/replay verified** con otra semántica. Ninguno de esos números equivale a fallos resueltos ni demuestra que el antecedente recorriera la rama pendiente.
- Recuperación preparada: **67 carpetas completed únicas** —49 exactas y 18 ELK provisionales por ID nativo 10234—, seis Kalamba needs_review separadas y 373 combinaciones de archivo externo/hash para doce juegos prioritarios. El manifiesto guarda IDs, rutas relativas, pointers y hashes; la existencia de los originales no está comprobada aquí.
- Ruby `rp_103`, `rp_214`, `rp_92`, Hacksaw `2670/bonus` y Betsoft `945/0` tienen fuentes concretas localizadas. Yggdrasil `7400` tiene estados completed en SQLite, pero faltan los mensajes de `C`. Recuperar los originales indicados y contrastar sus bytes antes de ampliar contratos. Los archivos adjuntos se leyeron como datos; no se ejecutó `indexar.py` ni se modificó el runtime o la cobertura.

La campaña sigue en **473 completos / 109 parciales**. El runner conserva **35 proveedores**: el histórico y la documentación usan universos/aliases distintos, no reemplazan su catálogo. Los hashes y límites de cada adjunto están en el informe.

## Última campaña: diez juegos por proveedor con cierres previos

La solicitud se ejecutó completa: **21 proveedores × 10 juegos, 582 intentos de modos implementados, 473 cierres nativos verificados y 109 parciales**. No hubo modos implementados sin iniciar, duplicados, `transport_error` ni timeout. Los **11 modos sin contrato** quedan separados. Leer el [informe](COMPLETE-ROUNDS-10-2026-11-15.md) y consultar los [casos JSON](complete-rounds-10-2026-11-15.json) por proveedor antes de repetir el diagnóstico.

- **Normales: 181/210.** Catorce proveedores cerraron sus diez normales; ocho cerraron todos sus modos implementados. Por juego: **163 cerraron todos sus modos, 19 algunos y 28 ninguno**.
- Parciales: **90 `provider_error` y 19 `unknown_event`**. El informe agrupa 22 causas: 55 errores HTTP de apertura, 15 rechazos durante operación, 15 controles/contratos locales y 24 continuaciones pendientes. `provider_error` no atribuye por sí mismo un fallo al servidor.
- Sólo **514 intentos registraron alguna intención de acción de ronda**; 68 no registraron ninguna. Las 2578 acciones marcadas y los 260 intentos con varias acciones no equivalen a giros base ni a bonos independientes.
- [Captura 43784896618](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43784896618): 341 s, del 16/11 01:12:45 al 01:18:26 UTC —15/11 22:12:45–22:18:26 UTC−3—. El workflow conserva `failure`/exit 2 por parciales y cobertura pendiente; cifrado y artifact correctos. Solapamiento observado de intervalos: **42 juegos / 21 proveedores**, máximo dos juegos por proveedor; no es concurrencia exacta HTTP/WS.
- Solicitud/workflow: **`84b4b96`**, [CI 43784896641 correcto](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43784896641). Binario: **`af8f957`**, obtenido del [CI 43699080917](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43699080917). Hashes exactos y configuración en el informe y la [guía operativa](../capture-runner/operations/COMPLETE-ROUNDS-10.md).
- **1746 hashes/tamaños de archivos de intentos correctos, cero incidencias de integridad.** Replay de los 582 intentos offline: **473 exit 0 / 109 exit 2**, con **4945 solicitudes/pasos** y cero incidencias; las parciales se conservan como tales.
- Banco privado verificado y entregado aparte: **`complete-rounds-10-bank-20261115.zip`**, 142555475 bytes, SHA-256 `7ea575226fb25e63e4b6b63c403fe8f32352104381a8597ae88720d6e8f3909e`. Incluye el banco, binario, original cifrado, material privado de recuperación y manifiesto; todo ese material permanece fuera de Git. El informe identifica el índice y los controles del paquete.

**Aclaración posterior, 16/11:** los seis Kalamba se detienen al validar el WebSocket `gameserver-api-for-kl-na-2.test.twmc8fvg.click`, porque el patrón local sólo admite un sufijo numérico; el documento del launcher sí está permitido. En Pragmatic `vs10bookfallen/pur-1`, la etiqueta local `demo session unavailable` encubre `frozen`/`SystemError`, sin `unlogged`: no diagnosticar ausencia de sesión a partir de ella. Ver la aclaración causal del informe y `diagnostic_revision` del JSON. Sin nuevas capturas, cambios de runtime ni variaciones de conteos; el paquete privado original permanece intacto.

La selección es fija y dirigida: 41 juegos con cierres anteriores, nueve por diversidad y 160 por estratos. No extrapolar sus resultados al catálogo. Los 14 proveedores sin cierres previos quedaron fuera; **esta campaña no revalidó ni resolvió los 403 del retest de sesiones** descrito a continuación. No repetir capturas por una edición documental.

## Histórico: sesiones Playwright y reproducción HTTP

El usuario pidió comprobar si los 403 se debían a falta de sesión, iniciar las demos con Playwright y portar su contrato a HTTP/WS. **La inspección de 12 juegos terminó y los cambios están implementados.** Leer [SESSION-BOOTSTRAP-2026-11-15.md](SESSION-BOOTSTRAP-2026-11-15.md) y [sus casos JSON](session-bootstrap-2026-11-15.json) antes de repetir la investigación.

- Playwright aceptó ambos init KA sin cookies; GoldBlitz autenticó, pero sus assets fallaron por certificado y no hubo Refresh. Evoplay/Kendoo recibieron bloqueos Cloudflare; ThreeOaks falló CORS; Red Tiger Panda recibió 403 enviando una cookie recién emitida y Happy llegó a entry 500. No todos los 403 acreditan ausencia de sesión.
- Portado: UA único; perfil/firma KA 2101 para ThiefDog/TropicalHoliday y `init.to` del launcher; `gsc`/text/plain/UUID y correlación Kendoo/ThreeOaks; contexto de authenticate Games Global; identificador aleatorio de 32 bytes sólo en `/setup` Red Tiger. No se reutilizan cookies ni tokens históricos.
- **Retest HTTP 43698411683: 12/12 intentos rechazados con 403 antes del giro; 0 cierres.** Son 34 solicitudes y 12 parciales conservadas, exit 2. CI del código y del binario correctos. `abe03e1` corrige el disparador que había omitido la primera campaña por no tener `rg`; esa ejecución omitida cuenta como cero intentos. El acceso nativo sigue pendiente, no declarar resueltos los 403.
- Banco privado `session-bootstrap-evidence-20261115.zip`: evidencia del navegador y HTTP, 12 parciales / 34 solicitudes, con hashes y replay offline verificados. Ver el informe para su hash y rutas de cada caso.
- Verificación local: **274 tests correctos, 2 opt-in ignorados**, fmt/clippy/release correctos. No se volvieron a ejecutar los originales Ruby. Cobertura y los 44 bindings intactos.
- No reabrir los falsos CAPTCHA Evoplay/Kendoo: provenían de un comentario HTML y el clasificador ya está corregido. El GET comparativo de Red Tiger ocurrió después de invalidar su cookie y no prueba que una sesión válida no pueda transferirse.

El workflow nativo nuevo espera CI exitoso y usa el binario de ese mismo commit; sólo solicita una campaña si cambió `capture-runner/operations/session-http-request.json`. Las instrucciones reproducibles están en [SESSION-BOOTSTRAP.md](../capture-runner/operations/SESSION-BOOTSTRAP.md). Los cambios documentales no repiten capturas.

## Campaña completa anterior: 35 proveedores × 2 juegos

El usuario pidió 2 juegos por proveedor y los 35 proveedores simultáneos. [Informe completo](LIVE-SMOKE-2026-11-15.md), [casos en JSON](live-smoke-2026-11-15.json), [ejecución 43693889120](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43693889120). Configuración/workflow: `20cd8d0`; binario: `f550c02`.

- 70 seleccionados, 69 ejecutables (Microgaming sólo tiene uno), 202 intentos terminados en 69 s: **109 completos, 71 provider_error, 22 unknown_event**, cero errores de transporte. Normales: 40/69.
- **13 proveedores pasan toda la muestra, 8 parcialmente y 14 ninguno.** Solapamiento de intervalos: 69 juegos/35 proveedores, máximo 2 por proveedor; no es concurrencia exacta de solicitudes.
- Pendientes localizados: **Belatra compara modification numérico con String; ELA rechaza connected; Playson encuentra fuentes nuevas; Playtech recibe GLU6 no soportado.** También faltan continuaciones en varios proveedores, incluido `rp_103` compra freespin → `pick`. Ver el informe antes de reanalizar las capturas.
- Otros fallos son de acceso/inicialización; algunos muestran Cloudflare, ipBlocked, país no permitido o error del emisor. `provider_error` no significa necesariamente fallo remoto.
- Banco nuevo entregado fuera de Git: `live-smoke-35x2-bank-20261115.zip`, 202 intentos (109 completos/93 parciales), con hashes, informes y recuperación del artifact cifrado. Los cambios de sesiones posteriores están descritos arriba; los otros fallos siguen pendientes.

El workflow de la campaña 35×2 reutiliza un binario fijado por run/hash: actualizar ambos para probar un cambio de código con esa campaña. Sólo modificar documentación no dispara otra campaña. El primer intento local falló por DNS/transporte del entorno; usar los resultados de GitHub Actions para el diagnóstico.

## Objetivo y decisiones ya acordadas

Recolectar apuestas demo y conservar mensajes/estados nativos por salida, proveedor, juego, modalidad e intento; después exportarlos y reproducirlos localmente para estudiar su funcionamiento. Runner Rust independiente, VM Linux, sin GUI. Cada salida tiene un proceso y cada proveedor su propio cupo: 30 salidas × (4 juegos Pragmatic + 4 RubyPlay) = 240 juegos configurados, sujetos a cobertura y recursos.

Se reutiliza la evidencia, no los runtimes antiguos. La conciliación de saldos no bloquea captura ni cierre. Una continuación desconocida queda parcial con alerta; nunca se inventa. Las sesiones de capturas nuevas son independientes; las rondas históricas importadas conservan su contexto original. [Invariantes completos](../AGENTS.md).

## Entregado: no volver a implementarlo

| Área | Resultado en `f550c02` |
|---|---|
| RubyPlay | De 11 a **255/262 modos**, 179 juegos. Init sin `sessionData` válido; controles de identidad/versión/`an`, continuaciones y elecciones por modo; precedencia del decodificador normal de `rp_47` conservada. |
| Playtech GPAS | De 1 a **100/3940 modos**, 48 juegos. Fuentes/JMP/JMM actuales, configuración por ámbito, comandos modernos/legacy, zlib y continuaciones. Transporte común admite recursos del directorio validado de cada juego. |
| Kalamba (`hardfire-kalamba` en configuración) | De 1 a **66/66 normales inventariados**. Constructor del cliente y respuestas de apertura/giro contrastados; ramas desconocidas quedan parciales. |
| Banco y replay | `export`, `import-ruby`, `replay`; formato versionado, SHA-256, conservación de parciales, correspondencia estricta, mismatch sin avance ni entrega de respuesta. |
| Fiabilidad | Configuración WG ausente no se transforma en salida directa; DNS `0644` aun con umask `077`; publicación atómica sin sobreescribir metadatos; eliminado el enlace `target` que rompía CI. |

Total: **35 proveedores**, **9878 juegos / 20090 modos inventariados**, **14787 modos implementados en 6957 juegos**, **5303 pendientes**. Esta entrega añadió 408 modos. Son contratos implementados, no una certificación de disponibilidad live ni de todas las ramas posibles. [Cobertura](../capture-runner/docs/COVERAGE.md).

## Ruby completo: significado exacto

Se recibieron **179 archivos de captura**, con **434 intentos: 432 cerrados y 2 parciales/rechazados**, y **1491 pares aceptados de solicitud/respuesta**. Cubren 255 pares juego/modo: 179 normales, 51 freespin, 17 respin, 4 minispin y 4 select. `binary` y `funModeData.sessionData` se conservan; son JSON nativos procesados, no el HTTP/HAR original íntegro.

Las dos parciales corresponden a `rp_120` y `rp_171`, modo `buy_feature:freespin`, rechazo `M-0`: contienen errores/contexto y **cero pares de acción reproducibles**. Eso no es pérdida de datos del importador. Los otros cinco modos pendientes son `ruby_buy` de `rp_26`, `rp_29`, `rp_37`, `rp_57`, `rp_65`.

El banco `rubyplay-capture-bank-20261115.zip` fue generado, verificado y entregado aparte; no está en Git. Su identificación y cómo regenerarlo están en [REPLAY.md](../capture-runner/docs/REPLAY.md). Los 13 archivos fuente completos del fixture incluido permiten la suite normal sin el ZIP externo.

## Pendientes reales y límites

- **Ruby:** esos siete modos requieren evidencia aceptada de entrada/continuación. Los decodificadores internos adicionales no son requisito para conservar/reproducir binarios opacos.
- **Playtech:** NGM, GLU6, PSI/Extreme Fruits, Venetian Masks/Ferox y otros UUID/constructores incompletos. Las siete trazas contienen 47 comandos, pero 19 payloads están redactados y 28 `playStack` truncados: no forman un banco de respuestas completas. Los 986 recibos tampoco equivalen a 986 partidas reproducibles.
- **Kalamba:** el 66/66 abarca sólo los normales inventariados; no habilita compras ni continuaciones sin contrato.
- **Local visual:** existe CLI/API de mensajes, sin cliente gráfico/assets originales, reescritura automática de tokens/contadores ni generación de nuevas rondas/RNG.
- **Operación real:** se probaron las muestras de 35 × 2 y 21 × 10 en GitHub Actions, con salida directa. Los fallos y continuaciones de la última campaña están localizados en su informe y no se corrigieron en esta tarea. Sigue pendiente verificar peers WireGuard reales, otras salidas/geografías y el resto del catálogo; las muestras no se extrapolan al proveedor completo.

Los demás pendientes y los archivos exactos que faltan están en [MISSING-EVIDENCE.md](../capture-runner/docs/MISSING-EVIDENCE.md). No repetir una auditoría de todos los proveedores para cambiar uno.

## Evidencia disponible y externa

`standalone info/` aporta inventarios/planes/manifiestos originales verificados; `capture-runner/bindings/` contiene 44 bindings; `capture-runner/tests/fixtures/` contiene los fixtures publicados. No modificar la evidencia fuente para hacer pasar una prueba.

El usuario **ya entregó** `ENTREGA-CLAUDE(2).zip`, `evidencia-claude-20261115.zip` y `Playtech evidence(1).zip`. El segundo es un subconjunto idéntico del primero. Los originales privados no se publicaron: su ausencia en un checkout nuevo no significa que no se hayan analizado. Ubicarlos por nombre/hash en los adjuntos disponibles sólo cuando sean necesarios; los hashes y rutas internas están en [PROVENANCE.md](../capture-runner/docs/PROVENANCE.md). Evitar depender de rutas temporales de otro agente.

## Dónde continuar según la tarea

| Necesidad | Leer primero | Código relevante bajo `capture-runner/src/` |
|---|---|---|
| Retomar resultados de diez juegos por proveedor | [Informe 21 × 10](COMPLETE-ROUNDS-10-2026-11-15.md), [casos JSON](complete-rounds-10-2026-11-15.json) y [operación](../capture-runner/operations/COMPLETE-ROUNDS-10.md) | `capture_bank.rs`, `replay.rs`, adaptador del proveedor afectado |
| Ejecutar/configurar VM | [README del runner](../capture-runner/README.md), [WireGuard](../capture-runner/docs/WIREGUARD.md) | `config.rs`, `engine.rs`, `supervisor.rs`, `wireguard.rs` |
| Exportar/importar/reproducir | [REPLAY](../capture-runner/docs/REPLAY.md), [interfaces](../capture-runner/INTERFACES.md) | `capture_bank.rs`, `ruby_import.rs`, `replay.rs`, `main.rs` |
| Diagnosticar 403 / sesiones | [Contraste Playwright/HTTP](SESSION-BOOTSTRAP-2026-11-15.md), casos JSON y evidencia privada identificada allí | `context.rs`, adaptador del proveedor, `operations/SESSION-BOOTSTRAP.md` |
| Ampliar un proveedor | [Evidencia pendiente](../capture-runner/docs/MISSING-EVIDENCE.md), su entrada en `binding-provenance.json` | `adapters/<proveedor>.rs`, su binding; registro `adapters/mod.rs` |
| Ruby / Playtech / Kalamba | Secciones de esos proveedores en [procedencia](../capture-runner/docs/PROVENANCE.md) | `adapters/rubyplay_extra.rs`, `adapters/rubyplay.rs`, `adapters/playtech/{native,sources}.rs`, `adapters/kalamba.rs` |
| Cambiar cobertura o captura | [COVERAGE](../capture-runner/docs/COVERAGE.md), [interfaces](../capture-runner/INTERFACES.md) | `catalog.rs`, `adapters/mod.rs`, `context.rs`, `journal.rs` |

Para consultar sólo un proveedor sin cargar el JSON completo en contexto, desde la raíz y con `jq` disponible:

```bash
jq '.providers[] | select(.provider == "rubyplay")' capture-runner/docs/coverage.json
```

## Verificación y reanudación mínima

Última verificación de campaña: binario `af8f957` fijado por CI/hash, 582 intentos capturados y reproducidos offline, 473 completos / 109 parciales, 1746 archivos de intentos íntegros. Esta campaña no añadió código Rust ni modificó cobertura; las pruebas de implementación de cada etapa se mantienen atribuidas a su commit.

Rust mínimo 1.99; verificado con 1.100.0. En `317fe6f`, suite normal **274 correctos y 2 opt-in ignorados**; fmt, clippy y release correctos. La ampliación `f550c02` tuvo 264 correctos incluyendo los dos opt-in, con 44 hashes de bindings y cobertura generada verificados; los cambios de sesiones no alteran esos bindings ni la cobertura. Los detalles y comandos opt-in están en [VERIFICATION.md](../capture-runner/docs/VERIFICATION.md); no volver a ejecutar la investigación completa para una edición documental.

Desde la raíz, para compilar y consultar el plan sin contactar proveedores:

```bash
cargo build --release --locked --manifest-path capture-runner/Cargo.toml
capture-runner/target/release/capture-runner plan --config capture-runner/examples/direct-explicit.json
```

`--details` añade cada combinación pendiente. Si se define `CARGO_TARGET_DIR`, ajustar la ruta del ejecutable. Los comandos `run`/`wg up` sí son operativos; no usarlos como prueba de documentación. Para validar cambios de código, seguir los comandos de [verificación](../capture-runner/docs/VERIFICATION.md) según el área afectada.

## Historia mínima

- `6d814f8`: migración del runner verificado al repositorio solicitado; inventarios y dependencias de tests incluidos.
- `f550c02`: ampliación Ruby/Playtech/Kalamba, banco/importador/replay y fiabilidad descritos arriba; CI aprobado.
- `20cd8d0`: campaña real acotada de 35 proveedores × 2 juegos seleccionados, conservación cifrada de evidencia y reporte de los fallos observados; sin cambios del código del runner.
- `5ed8087`: corrección de User-Agent y sonda Playwright de los seis proveedores con 403.
- `a1cb667`: recuperación de la fuente pública Kendoo exacta observada; sin otra sesión de juego.
- `317fe6f`: contratos de sesión portados, regresiones y campaña HTTP acotada vinculada a su CI.
- `abe03e1`: corrección del gate sin depender de `rg` y solicitud `r2`; mismo Rust, CI correcto y muestra HTTP real de 12 parciales.
- `84b4b96`: solicitud de diez juegos de los 21 proveedores con cierres previos, con binario `af8f957` fijado por CI/hash; 582 intentos, 473 cierres / 109 parciales, banco y replay offline íntegros. Sin cambios de adaptadores ni cobertura.
- Este traspaso conserva los resultados por etapa y los paquetes privados; una revisión documental no añade cambios de comportamiento.

Los planes/especificaciones de `docs/superpowers/` explican decisiones históricas. No reabrir sus tareas completadas ni usar sus conteos antiguos como estado actual.
