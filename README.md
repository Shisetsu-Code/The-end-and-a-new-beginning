# The end and a new beginning

Recolector de apuestas **demo** en Rust para una VM Linux, sin interfaz gráfica. Guarda solicitudes y respuestas nativas, sigue las continuaciones conocidas y permite exportarlas y reproducirlas localmente. Los saldos y la conciliación financiera no bloquean la captura.

## Retomar con un LLM

Leer [AGENTS.md](AGENTS.md) y [docs/HANDOFF.md](docs/HANDOFF.md). El traspaso concentra el estado publicado, lo terminado, pendientes concretos, resultados verificados y rutas para continuar sin repetir la investigación. Los planes anteriores son históricos; los originales externos no son necesarios para compilar ni ejecutar la suite normal.

## Paralelismo y WireGuard

Cada salida de red tiene un proceso independiente. Dentro de cada salida, cada proveedor dispone de su propio cupo de juegos simultáneos. Los modos de un mismo juego se ejecutan secuencialmente con sesiones nuevas.

Por ejemplo: **30 WireGuard × (4 juegos Pragmatic + 4 juegos RubyPlay) = 240 juegos simultáneos configurados**. Cada proveedor puede tener otro cupo; también puede cambiar por salida. La concurrencia efectiva depende de los juegos con contrato disponible y los recursos de la VM. `plan` muestra ambos límites.

Los archivos `*.conf` se descubren desde el directorio configurado. Cada túnel tiene namespace, rutas y DNS propios. Una salida caída no se reemplaza automáticamente por la IP directa de la VM.

## Empezar

Con Rust 1.99 o posterior y un compilador C:

```bash
git clone https://github.com/Shisetsu-Code/The-end-and-a-new-beginning.git
cd The-end-and-a-new-beginning
cargo build --release --locked --manifest-path capture-runner/Cargo.toml

# Verificar inventarios, cobertura y cupos; no contacta proveedores.
capture-runner/target/release/capture-runner plan   --config capture-runner/examples/direct-explicit.json --details
```

Para ejecutar una campaña, crear `capture-runner/config.local.json` siguiendo [la configuración completa](capture-runner/README.md). Con WireGuard, instalar `iproute2` y `wireguard-tools`, guardar las configuraciones fuera del repositorio y ejecutar:

```bash
sudo capture-runner/target/release/capture-runner wg up   --config capture-runner/config.local.json
sudo capture-runner/target/release/capture-runner run   --config capture-runner/config.local.json
```

Los comandos de red y sus requisitos están en [WIREGUARD.md](capture-runner/docs/WIREGUARD.md). Una ejecución directa requiere una salida explícita; `run` no levanta túneles por sí mismo.

## Capturas y alertas

Cada intento queda separado por ejecución, salida, proveedor, juego, modo y muestra. El diario conserva los mensajes antes de interpretarlos, incluidas respuestas parciales. Un estado desconocido genera `unknown_event`, se guarda en `alerts.jsonl` y sale por `stderr`; sólo se detiene ese intento.

Al repetir el mismo `run_id`, no se reenvían muestras que ya tuvieron un intento, aunque haya quedado incompleto. Para una nueva pasada deliberada se usa otro `run_id`.

## Contenido del proyecto

| Ruta | Contenido |
|---|---|
| `capture-runner/` | Código Rust, bindings nativos, tests, ejemplos y documentación de operación |
| `standalone info/` | Inventarios, planes y manifiestos originales de 35 proveedores, más las fuentes concretas usadas por los tests |
| `docs/IMPORTED-FILES.json` | Procedencia y hashes de la importación inicial |
| `.github/workflows/capture-runner.yml` | Formato, tests locales, lint, compilación y artefacto Linux |

Este checkout contiene lo necesario para compilar, probar y ejecutar el runner. No necesita clonar el repositorio anterior ni ejecutar sus runners. Las fuentes históricas completas se identifican por repositorio, commit y hash en la documentación de procedencia.

## Estado y documentación

- [Traspaso y estado actual para agentes](docs/HANDOFF.md)
- [Operación y configuración](capture-runner/README.md)
- [Exportación, importación Ruby y replay local](capture-runner/docs/REPLAY.md)
- [Cobertura por proveedor y modo](capture-runner/docs/COVERAGE.md)
- [Evidencia pendiente](capture-runner/docs/MISSING-EVIDENCE.md)
- [Procedencia de los protocolos](capture-runner/docs/PROVENANCE.md)
- [Verificación y límites](capture-runner/docs/VERIFICATION.md)

La cobertura publicada indica contratos implementados. Las pruebas automatizadas no realizan apuestas en proveedores ni levantan peers WireGuard reales. La conectividad y disponibilidad actuales se comprueban al ejecutar en la VM.
