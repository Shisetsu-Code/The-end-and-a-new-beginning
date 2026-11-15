# Verificación de la entrega

Verificado el 15 de noviembre de 2026 en Linux x86_64, con `rustc 1.100.0 (0ef1bc386 2026-11-09)`. La base del repositorio es `749ae8e4ca8e50326cc756fda8efb916c47b0be8`; esta ampliación sigue a la entrega inicial `fc84eebbcae3192a8e5b949ade839e6f8d50f59a`.

## Resultado

| Comprobación | Resultado |
|---|---|
| `cargo fmt --manifest-path capture-runner/Cargo.toml --check` | Correcto |
| `cargo test --offline --locked --manifest-path capture-runner/Cargo.toml` | 213 pruebas de biblioteca y 3 de CLI: 216 correctas, ninguna fallida ni ignorada |
| `cargo clippy --offline --locked --manifest-path capture-runner/Cargo.toml --all-targets -- -D warnings` | Correcto, sin advertencias |
| `cargo build --release --offline --locked --manifest-path capture-runner/Cargo.toml` | Correcto |
| Plan del binario release frente a `docs/coverage.json` | Idénticos, byte a byte |
| `sha256sum -c capture-runner/docs/BINDINGS.sha256` | 40/40 bindings correctos |
| Fuentes de los siete bindings nuevos | 102/102 verificadas: 100 miembros del ZIP y dos fuentes del commit base |

Los comandos se ejecutaron desde la raíz del repositorio. `--offline` usó dependencias previamente descargadas; no es un requisito para la primera compilación en la VM.

El plan final contiene 35 proveedores, 9.878 juegos inventariados y 20.090 modos. Hay 14.379 modos con contrato ejecutable, distribuidos en 6.675 juegos, y 5.711 modos pendientes. La ampliación incorpora 538 modos ejecutables: 516 del inventario anterior y 22 nuevos pares de IronDog. [COVERAGE.md](COVERAGE.md) desglosa las cifras y [MISSING-EVIDENCE.md](MISSING-EVIDENCE.md) identifica los contratos ausentes.

## Qué prueban las comprobaciones

Las pruebas cubren los contratos nativos implementados, selección de modos, cierres y continuaciones conocidas, eventos desconocidos, preservación de respuestas parciales, aislamiento de cookies, límites de concurrencia independientes, cancelación y reanudación sin reenviar intentos inciertos. También ejercitan configuración de 30 salidas, argumentos de administración de WireGuard, propiedad de recursos y rollback mediante comandos simulados. Las pruebas de transporte usan servidores de loopback.

La ampliación verifica además las ofertas dinámicas y 31 cadenas de Mascot, las familias de dados Amusnet, los selectores y restricciones nativas de antebet 1spin4win, los códigos/acciones Kendoo, los controles de sesión/versión de RubyPlay y los comandos Betsoft/IntegratedSlots. Las pruebas del catálogo comprueban que los 61 pares de IntegratedSlots aparecen exactamente una vez, mantienen los objetos originales y añaden el hash del suplemento a la identidad del catálogo.

La revisión independiente corrigió el encabezado requerido de IronDog, las continuaciones Betsoft sin avance y los cierres inferidos sin señal nativa suficiente. También comprobó que no se perdieran modos conocidos por estar ausentes del catálogo anterior. En RubyPlay se conserva el binario opaco sin prometer detección de nuevos eventos internos que no cambien el sobre HTTP.

Los inventarios y archivos de evidencia existentes permanecen sin cambios. Los 33 bindings anteriores conservan sus bytes; los siete nuevos tienen fuentes y hashes verificables en [PROVENANCE.md](PROVENANCE.md). No se publican los archivos crudos de la entrega ni sus sesiones históricas.

## Límites de esta verificación

No se realizaron solicitudes a proveedores ni se levantaron túneles WireGuard reales. No se ha medido el consumo de la VM con 30 túneles ni con miles de juegos simultáneos. La disponibilidad remota, autenticación demo, handshake de los peers, salida geográfica y capacidad efectiva deben comprobarse en la VM con sus configuraciones. Un modo con contrato ejecutable puede recibir un rechazo remoto o alcanzar un límite de pasos; esos resultados conservan su captura.

## Binario local

SHA-256 de `capture-runner/target/release/capture-runner`:

```text
fefba73d41839d0804dc33cd3b8f7934a803ecd79367fe8ab1c7ae6cb5a87974
```

Este hash identifica la compilación local, no promete builds reproducibles entre entornos. El ejecutable es x86_64 y enlaza dinámicamente con glibc, libm y libgcc_s de Linux. Para otra arquitectura o distribución, compilar en la VM. El workflow de GitHub Actions compila y adjunta su propio artefacto Linux.
