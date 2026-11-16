# Verificación del runner

## Campaña de diez juegos por proveedor: 15 de noviembre de 2026 UTC−3

La muestra se ejecutó el **16/11/2026, 01:12:45–01:18:26 UTC** —15/11, 22:12:45–22:18:26 UTC−3—. [Informe completo](../../docs/COMPLETE-ROUNDS-10-2026-11-15.md), [casos JSON](../../docs/complete-rounds-10-2026-11-15.json) y [guía operativa](../operations/COMPLETE-ROUNDS-10.md). La campaña reutiliza un binario verificado; no cambió adaptadores ni cobertura.

| Comprobación | Resultado |
|---|---|
| Solicitud/workflow | `84b4b96d227e73c2d4ac046a0a67e7d2452a09ba`; [CI 43784896641](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43784896641) correcto |
| Fuente del ejecutable | `af8f957cce4cd57fe633da50b4620116ed19c6e3`; [CI 43699080917](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43699080917) correcto |
| SHA-256 del ejecutable usado | `faceddbd1e234c09eb2147affcd553b2e8dce00c9423de6d2e2de4010af179a6` |
| Plan offline e identidad de entradas | 21 proveedores, 210 juegos ejecutables, 582 modos implementados y 11 sin contrato; 42 cupos; runtime/evidencia coincidentes con el binario fijado |
| [Captura 43784896618](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43784896618) | **582/582 intentos iniciados y terminados** en 341 s; **473 cierres / 109 parciales**, sin timeout ni `transport_error` |
| Estados originales de las parciales | 90 `provider_error`, 19 `unknown_event`; los 11 modos sin contrato se cuentan aparte |
| Modos normales | **181/210 completos**; 14 proveedores con sus diez normales completos |
| Todos los modos implementados | Ocho proveedores y 163 juegos completan toda su muestra; otros 19 juegos cierran algunos modos y 28 ninguno |
| Integridad del banco | **1746/1746 archivos** con SHA-256 y tamaño correctos; cero ausentes, truncamientos o inconsistencias |
| Replay completo del banco, sin red | **582 intentos**, **4945 solicitudes/pasos**; 473 exit 0, 109 exit 2, cero incidencias |
| Solapamiento observado | Máximo **42 intervalos de juegos / 21 proveedores**, dos juegos por proveedor; no mide solicitudes HTTP/WS simultáneas |
| Artefacto y resultado de Actions | Cifrado y upload correctos; el workflow conserva `failure`/exit 2 por parciales y cobertura pendiente |

Se contrastaron `index.state`, incidencias, `result.status`, `completion` no nulo, último `attempt_finish`, metadatos, secuencia del journal, hashes y tamaños. Los saldos no participan en el cierre. Se verificaron también los 582 replays con las solicitudes originales: el exit 2 de una parcial preserva su estado y no se trata como fallo nuevo del reproductor.

El índice del banco tiene SHA-256 `fc058196d57d1daf16bff914a3f0637cc16a6a34e3eba93a305e6ee89d93e39e`. El paquete privado `complete-rounds-10-bank-20261115.zip` se identifica en el informe; originales, mensajes, claves y resultados privados permanecen fuera de Git.

La muestra fija contiene 41 juegos con cierres previos, nueve por diversidad y 160 por estratos. Sólo 514 intentos registraron alguna intención de acción de ronda; 68 no registraron ninguna. Ni 582 intentos ni 2578 acciones marcadas equivalen a ese número de giros aceptados. La campaña no revalidó los 14 proveedores excluidos ni resolvió los 403 del retest histórico siguiente.

## Sesiones demo: actualización del 15 de noviembre de 2026

Implementación: [317fe6f](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/commit/317fe6f87b5726ac3cae3ec69751809adf007f3c), precedida por la corrección de User-Agent en `5ed8087`. La inspección con Playwright y el contraste HTTP están documentados en [SESSION-BOOTSTRAP-2026-11-15.md](../../docs/SESSION-BOOTSTRAP-2026-11-15.md). Los resultados de compilación no sustituyen a los resultados remotos de ese informe.

| Comprobación local, Rust 1.100.0 / Linux x86_64 | Resultado |
|---|---|
| `cargo fmt --manifest-path capture-runner/Cargo.toml --check` | Correcto |
| `cargo test --offline --locked --manifest-path capture-runner/Cargo.toml` | **274 correctas, 0 fallidas, 2 opt-in ignoradas**: 262 biblioteca, 3 CLI, 3 CLI replay y 6 importación Ruby |
| `cargo clippy --offline --locked --manifest-path capture-runner/Cargo.toml --all-targets -- -D warnings` | Correcto, sin advertencias |
| `cargo build --release --offline --locked --manifest-path capture-runner/Cargo.toml` | Correcto |
| Plan de `operations/session-http-request.json` | 6 proveedores, 12 juegos, 12 normales ejecutables, máximo 12 juegos activos |
| Revisión independiente de contratos y workflow | Sin hallazgos bloqueantes; verificación remota informada por separado |
| CI `43697499119` (`317fe6f`) y `43698065416` (`abe03e1`, mismo Rust) | **Correctos**: formato, suite, clippy, release y artifact |
| [Retest HTTP 43698411683](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43698411683) | **12 rechazos 403, 0 giros, 0 cierres**; exit 2 conservado y evidencia cifrada subida |
| Banco de la muestra HTTP | 12 parciales, 34 solicitudes; 36 hashes verificados y los 12 replays offline terminan con exit 2, sin pérdida de mensajes |

Las regresiones comprueban el User-Agent real y su journal mediante TCP; `gsc`, Content-Type, UUID y correlación de Kendoo/ThreeOaks; las firmas KA 2090/2101 con un oráculo JavaScript y contextos sintéticos; el contexto público de `authenticate` de Games Global; y el identificador aleatorio de 32 bytes enviado sólo a `/setup` de Red Tiger. Los 44 bindings y la cobertura no cambiaron. No se repitieron las dos pruebas opt-in de originales Ruby para esta corrección de sesiones.

## Ampliación e importación anteriores: `f550c02`

Verificado el 15 de noviembre de 2026 en Linux x86_64 con Rust 1.100.0. Implementación publicada: [f550c0228774ff307135ec25c2e2138e6a97a236](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/commit/f550c0228774ff307135ec25c2e2138e6a97a236), sobre la base `6d814f8e83490ba5a7c8eee92f97e0536b06aa16`. Incluye RubyPlay, GPAS/Playtech, Kalamba, banco/replay y correcciones del runtime. [Estado para retomar](../../docs/HANDOFF.md).

## Resultados de `f550c02`

La prueba real posterior de 35 proveedores × 2 juegos está en [LIVE-SMOKE-2026-11-15.md](../../docs/LIVE-SMOKE-2026-11-15.md): 109 de 202 intentos cerrados, con fallos de acceso, adaptadores y continuaciones pendientes. La tabla siguiente corresponde a la verificación offline de la implementación.

| Comprobación | Resultado |
|---|---|
| `cargo fmt --manifest-path capture-runner/Cargo.toml --check` |Correcto|
| Suite offline con las dos pruebas opt-in de evidencia |**264 correctas, 0 fallidas, 0 ignoradas**: 251 biblioteca, 3 CLI existentes, 3 CLI replay, 7 importación Ruby|
| Suite normal, sin originales privados |**262 correctas, 0 fallidas, 2 ignoradas**; las dos ignoradas son las comprobaciones opt-in de Ruby|
| `cargo clippy --offline --locked --manifest-path capture-runner/Cargo.toml --all-targets -- -D warnings` |Correcto, sin advertencias|
| `cargo build --release --offline --locked --manifest-path capture-runner/Cargo.toml` |Correcto|
| Plan del release frente a `capture-runner/docs/coverage.json` |Idénticos byte a byte|
| `sha256sum -c capture-runner/docs/BINDINGS.sha256` |44/44 correctos|
| Revisión independiente y revisión final de integración |Sin hallazgos importantes/críticos abiertos tras corregir y volver a revisar cinco hallazgos|
| [GitHub Actions 43686603978](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43686603978), commit `f550c02` |**Success**: formato, suite normal, lints, release Linux y subida del artefacto `capture-runner-linux-x86_64`|

Los resultados anteriores pertenecen a ese commit de implementación; no deben atribuirse a un cambio de código posterior sin verificarlo. Para repetir la suite normal desde la raíz:

```bash
cargo test --locked --manifest-path capture-runner/Cargo.toml
```

Las pruebas opt-in requieren los originales proporcionados por el usuario, que no se incluyen completos en Git. La suite normal de CI omite sólo esas dos comprobaciones; conserva los 13 archivos completos de RubyPlay del fixture y las pruebas funcionales del importador. `RUBYPLAY_EVIDENCE_DIR` activa el contraste del adaptador y `RUBYPLAY_CAPTURE_DIR` el del importador/replay; ambos apuntan al mismo directorio. Para ejecutar todo:

```bash
RUBYPLAY_EVIDENCE_DIR=/ruta/ENTREGA-CLAUDE/2-evidencia-claude/rubyplay/capture \
RUBYPLAY_CAPTURE_DIR=/ruta/ENTREGA-CLAUDE/2-evidencia-claude/rubyplay/capture \
cargo test --offline --locked --manifest-path capture-runner/Cargo.toml -- --include-ignored
```

`--offline` presupone dependencias descargadas. No es necesario para la primera compilación en una VM nueva.

## Evidencia comprobada

- **RubyPlay:** 179 archivos, 255 pares juego/modo, 432 rondas cerradas y 1491 intercambios nativos. Cada solicitud producida por el estado del adaptador se comparó con la grabada. La importación conservó 434 intentos: 432 completos y 2 parciales/rechazados, y se reprodujo todo el conjunto offline. Se corrigió la ausencia legítima de `sessionData` en init y se conservó la precedencia del decodificador normal de rp_47.
- **Playtech:** 100 modos de 48 juegos con UUID/tipo exacto. Se comprobaron 47 solicitudes suministradas, incluidas 25 continuaciones, configuración por ámbitos, constructores modernos/legacy, límites de coste, fuente/pins, descompresión y dominios de elección. Las respuestas saneadas no se trataron como cuerpos de replay completos.
- **Kalamba:** 66 normales con aperturas y primer giro/cierre contrastados con el cliente y las líneas fuente. La revisión independiente comprobó 198 hashes de registros nativos/proyecciones.
- **Banco/replay:** conservación byte a byte de diarios parciales, chunks HTTP/binarios, integridad, separación por intento, mismatch sin avance ni resultado, agotamiento, ocultación de contexto futuro durante replay y preservación íntegra en inspección.
- **Runtime:** fallo cerrado ante una configuración VPN desaparecida, salida directa explícita, permisos DNS con umask 077, publicación atómica de metadatos sin sobreescritura y competencia entre ocho escritores. Se eliminó el enlace `target` versionado que impedía la compilación normal.

Se mantienen los controles anteriores de sesiones nuevas, cancelación, alertas, reanudación sin reenvíos y cupos independientes por proveedor/salida. No se añadió conciliación financiera como requisito de cierre o exportación.

## Correcciones de la revisión

Se restringió Pigeon a su multiplicador nativo 1; se filtraron los multiplicadores por límites del ámbito efectivo; se permitió elegir otra vez una opción que el servidor vuelve a ofrecer sin marcarla consumida; se excluyó Venetian Masks por faltar su constructor Ferox exacto; y se evitó que `replay_start` exponga resultados futuros almacenados en el contexto importado. Los cinco casos tuvieron regresión negativa y verificación positiva.

El transporte común también se actualizó para aceptar los recursos JS/JMP/JMM y manifiestos dentro del directorio versionado del juego en el host oficial GPAS. La prueba cubre otros juegos además de Blitz10 y rechaza rutas/hosts fuera de ese alcance.

## Cobertura resultante

35 proveedores, 9878 juegos inventariados y 20090 modos. Hay **14787 modos implementados en 6957 juegos** y 5303 pendientes: 408 modos adicionales en esta ampliación. [COVERAGE.md](COVERAGE.md) contiene el desglose y [MISSING-EVIDENCE.md](MISSING-EVIDENCE.md) los límites concretos.

## Límites de la verificación offline de `f550c02`

No se realizaron apuestas ni solicitudes a proveedores y no se levantaron túneles reales. La disponibilidad remota, cambios de fuentes, peers WireGuard, salida geográfica y carga de miles de juegos deben comprobarse en la VM. Los contratos habilitados pueden producir rechazos/eventos desconocidos; se conservan esas capturas.

El replay actual funciona por CLI y API Rust, con solicitudes estrictamente iguales a las grabadas. No reescribe automáticamente tokens/contadores, no genera nuevas rondas aleatorias y no incluye el frontend gráfico original de los juegos.

## Banco Ruby entregado

Se generó `rubyplay-capture-bank-20261115.zip` con el release verificado y los 179 originales suministrados. Contiene 434 intentos, 432 completos y 2 parciales, con 1491 pares nativos. Se verificaron los 1302 hashes de archivos de intentos y el CRC de los 1305 miembros del ZIP. El tamaño, SHA-256, contenido y forma de regenerarlo están en [REPLAY.md](REPLAY.md). El banco completo permanece fuera de Git.

## Binario local verificado de `f550c02`

Linux x86_64, 16469080 bytes. SHA-256:

```text
0fa61b2db9cf0c40463093603f5eb7f7f79363a5c44280c1455bf38c554d1721
```

Identifica esta compilación local; no promete reproducibilidad binaria entre entornos. GitHub Actions compila y adjunta su propio ejecutable Linux.
