# Verificación de la ampliación de capturas y replay

Verificado el 15 de noviembre de 2026 en Linux x86_64 con Rust 1.100.0. Base: `6d814f8e83490ba5a7c8eee92f97e0536b06aa16`, sobre la que se integran RubyPlay, GPAS/Playtech, Kalamba, banco/replay y correcciones del runtime.

## Resultados

| Comprobación | Resultado |
|---|---|
| `cargo fmt --manifest-path capture-runner/Cargo.toml --check` |Correcto|
| Suite offline con las dos pruebas opt-in de evidencia |**264 correctas, 0 fallidas, 0 ignoradas**: 251 biblioteca, 3 CLI existentes, 3 CLI replay, 7 importación Ruby|
| `cargo clippy --offline --locked --manifest-path capture-runner/Cargo.toml --all-targets -- -D warnings` |Correcto, sin advertencias|
| `cargo build --release --offline --locked --manifest-path capture-runner/Cargo.toml` |Correcto|
| Plan del release frente a `docs/coverage.json` |Idénticos byte a byte|
| `sha256sum -c capture-runner/docs/BINDINGS.sha256` |44/44 correctos|
| Revisión independiente y revisión final de integración |Sin hallazgos importantes/críticos abiertos tras corregir y volver a revisar cinco hallazgos|

Las pruebas opt-in requieren los originales proporcionados por el usuario, que no se incluyen completos en Git. La suite normal de CI omite sólo esas dos comprobaciones; conserva los 13 archivos completos de RubyPlay del fixture y las pruebas funcionales del importador. Para ejecutar todo:

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

## Límites de verificación

No se realizaron apuestas ni solicitudes a proveedores y no se levantaron túneles reales. La disponibilidad remota, cambios de fuentes, peers WireGuard, salida geográfica y carga de miles de juegos deben comprobarse en la VM. Los contratos habilitados pueden producir rechazos/eventos desconocidos; se conservan esas capturas.

El replay actual funciona por CLI y API Rust, con solicitudes estrictamente iguales a las grabadas. No reescribe automáticamente tokens/contadores, no genera nuevas rondas aleatorias y no incluye el frontend gráfico original de los juegos.

## Binario local verificado

Linux x86_64, 16469080 bytes. SHA-256:

```text
0fa61b2db9cf0c40463093603f5eb7f7f79363a5c44280c1455bf38c554d1721
```

Identifica esta compilación local; no promete reproducibilidad binaria entre entornos. GitHub Actions compila y adjunta su propio ejecutable Linux.
