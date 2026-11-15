# Retomar el proyecto

**Estado al 15 de noviembre de 2026:** la ampliación y el replay están implementados y publicados en `main`, commit [f550c02](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/commit/f550c0228774ff307135ec25c2e2138e6a97a236). [CI offline de esa entrega: correcto](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43686603978). Después se ejecutó la prueba real descrita abajo, con fallos pendientes. Comprobar `git status` y `git log -3 --oneline` al retomar; si hay cambios posteriores, revisar su diff antes de aplicar esta fotografía.

## Última prueba real: leer antes de corregir proveedores

El usuario pidió 2 juegos por proveedor y los 35 proveedores simultáneos. [Informe completo](LIVE-SMOKE-2026-11-15.md), [casos en JSON](live-smoke-2026-11-15.json), [ejecución 43693889120](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43693889120). Configuración/workflow: `20cd8d0`; binario: `f550c02`.

- 70 seleccionados, 69 ejecutables (Microgaming sólo tiene uno), 202 intentos terminados en 69 s: **109 completos, 71 provider_error, 22 unknown_event**, cero errores de transporte. Normales: 40/69.
- **13 proveedores pasan toda la muestra, 8 parcialmente y 14 ninguno.** Solapamiento de intervalos: 69 juegos/35 proveedores, máximo 2 por proveedor; no es concurrencia exacta de solicitudes.
- Pendientes localizados: **Belatra compara modification numérico con String; ELA rechaza connected; Playson encuentra fuentes nuevas; Playtech recibe GLU6 no soportado.** También faltan continuaciones en varios proveedores, incluido `rp_103` compra freespin → `pick`. Ver el informe antes de reanalizar las capturas.
- Otros fallos son de acceso/inicialización; algunos muestran Cloudflare, ipBlocked, país no permitido o error del emisor. `provider_error` no significa necesariamente fallo remoto.
- Banco nuevo entregado fuera de Git: `live-smoke-35x2-bank-20261115.zip`, 202 intentos (109 completos/93 parciales), con hashes, informes y recuperación del artifact cifrado. Los fallos aún no están corregidos.

El workflow reutiliza un binario fijado por run/hash: actualizar ambos para probar un cambio de código. Sólo modificar documentación no dispara otra campaña. El primer intento local falló por DNS/transporte del entorno; usar los resultados de GitHub Actions para el diagnóstico.

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
- **Operación real:** ya se probó la muestra indicada arriba en GitHub Actions, con salida directa. Sigue pendiente verificar peers WireGuard reales, otras salidas/geografías y el resto del catálogo; los resultados de dos juegos no se extrapolan al proveedor completo.

Los demás pendientes y los archivos exactos que faltan están en [MISSING-EVIDENCE.md](../capture-runner/docs/MISSING-EVIDENCE.md). No repetir una auditoría de todos los proveedores para cambiar uno.

## Evidencia disponible y externa

`standalone info/` aporta inventarios/planes/manifiestos originales verificados; `capture-runner/bindings/` contiene 44 bindings; `capture-runner/tests/fixtures/` contiene los fixtures publicados. No modificar la evidencia fuente para hacer pasar una prueba.

El usuario **ya entregó** `ENTREGA-CLAUDE(2).zip`, `evidencia-claude-20261115.zip` y `Playtech evidence(1).zip`. El segundo es un subconjunto idéntico del primero. Los originales privados no se publicaron: su ausencia en un checkout nuevo no significa que no se hayan analizado. Ubicarlos por nombre/hash en los adjuntos disponibles sólo cuando sean necesarios; los hashes y rutas internas están en [PROVENANCE.md](../capture-runner/docs/PROVENANCE.md). Evitar depender de rutas temporales de otro agente.

## Dónde continuar según la tarea

| Necesidad | Leer primero | Código relevante bajo `capture-runner/src/` |
|---|---|---|
| Ejecutar/configurar VM | [README del runner](../capture-runner/README.md), [WireGuard](../capture-runner/docs/WIREGUARD.md) | `config.rs`, `engine.rs`, `supervisor.rs`, `wireguard.rs` |
| Exportar/importar/reproducir | [REPLAY](../capture-runner/docs/REPLAY.md), [interfaces](../capture-runner/INTERFACES.md) | `capture_bank.rs`, `ruby_import.rs`, `replay.rs`, `main.rs` |
| Ampliar un proveedor | [Evidencia pendiente](../capture-runner/docs/MISSING-EVIDENCE.md), su entrada en `binding-provenance.json` | `adapters/<proveedor>.rs`, su binding; registro `adapters/mod.rs` |
| Ruby / Playtech / Kalamba | Secciones de esos proveedores en [procedencia](../capture-runner/docs/PROVENANCE.md) | `adapters/rubyplay_extra.rs`, `adapters/rubyplay.rs`, `adapters/playtech/{native,sources}.rs`, `adapters/kalamba.rs` |
| Cambiar cobertura o captura | [COVERAGE](../capture-runner/docs/COVERAGE.md), [interfaces](../capture-runner/INTERFACES.md) | `catalog.rs`, `adapters/mod.rs`, `context.rs`, `journal.rs` |

Para consultar sólo un proveedor sin cargar el JSON completo en contexto, desde la raíz y con `jq` disponible:

```bash
jq '.providers[] | select(.provider == "rubyplay")' capture-runner/docs/coverage.json
```

## Verificación y reanudación mínima

Rust mínimo 1.99; entrega verificada con 1.100.0. **264 tests correctos** con los dos opt-in; suite normal **262 correctos y 2 ignorados**. Fmt, clippy, release, 44 hashes de bindings y cobertura generada correctos. Los detalles y comandos opt-in están en [VERIFICATION.md](../capture-runner/docs/VERIFICATION.md); no volver a ejecutar la investigación completa para una edición documental.

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
- Este traspaso: punto de entrada común, identificación del banco entregado y corrección de instrucciones/planes obsoletos; sin cambios de comportamiento.

Los planes/especificaciones de `docs/superpowers/` explican decisiones históricas. No reabrir sus tareas completadas ni usar sus conteos antiguos como estado actual.
