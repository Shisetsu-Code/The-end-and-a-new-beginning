# Capture runner

Recolector independiente en Rust para **VM Linux, sin GUI**. Usa los inventarios y protocolos de `standalone info/`. No carga el runner anterior, su base de datos ni sus bloqueos por conciliación.

## Concurrencia

Cada salida tiene un proceso. Dentro de ese proceso cada proveedor tiene su propio cupo de juegos. Los modos y muestras de un mismo juego se ejecutan secuencialmente, con sesión y cookies nuevas por intento.

Para 30 túneles y dos proveedores con `parallel_games: 4`, el límite configurado es **30 × (4 + 4) = 240 juegos activos**. Con 35 proveedores sería **4.200**. La selección de juegos ejecutables y los recursos de la VM pueden reducir la concurrencia efectiva; `plan` muestra ambos límites. Una única instancia de un juego no ocupa varios cupos a la vez.

Precedencia del cupo: ajuste del proveedor en una salida > ajuste general del proveedor > `default_parallel_games`. No existe un bloqueo global de un proveedor entre países. Cada worker usa dos hilos de ejecución asíncrona; el cupo mide juegos, no hilos.

## Compilar

Desde la raíz del repositorio, con Rust 1.99 o posterior y un compilador C para los decodificadores incorporados:

```bash
cargo build --release --locked --manifest-path capture-runner/Cargo.toml
```

El binario queda en `capture-runner/target/release/capture-runner`. No necesita Node, Python, navegador ni los crates anteriores en ejecución. Sí necesita los inventarios, planes y manifiestos de `standalone info/`.

Para WireGuard la VM debe disponer de soporte WireGuard del kernel, `iproute2`, `wireguard-tools` y, si se usa `run_as`, `runuser`. Los comandos administrativos se ejecutan con privilegios; los workers pueden usar una cuenta dedicada. Ver [WIREGUARD.md](docs/WIREGUARD.md).

## Configuración

Ejemplo de `/opt/The-end-and-a-new-beginning/capture-runner/config.local.json`:

```json
{
  "evidence_root": "../standalone info",
  "data_root": "/var/lib/capture-runner/captures",
  "run_id": "captura-001",
  "wireguard_dir": "/etc/capture-runner/wireguard",
  "default_parallel_games": 4,
  "samples_per_mode": 1,
  "max_steps": 256,
  "timeout_seconds": 30,
  "providers": [
    {"id": "pragmatic", "parallel_games": 4},
    {"id": "rubyplay", "parallel_games": 4}
  ],
  "exits": [
    {"id": "de", "parallel_games": {"pragmatic": 6}}
  ]
}
```

Cada archivo `*.conf` de `wireguard_dir` crea una salida con el nombre del archivo: `de.conf` → `de`. El ejemplo aplica seis juegos de Pragmatic en Alemania y cuatro en las demás salidas. RubyPlay tiene cuatro cupos por salida, sujetos a la cobertura realmente disponible. Para incluir todos los proveedores, usar `"providers": []`.

`games` y `modes` son filtros opcionales. Los juegos pueden escribirse como ID o `proveedor/ID`; los modos usan sus claves exactas del inventario. Un filtro inexistente produce error. Las rutas relativas se resuelven desde el archivo de configuración.

`max_steps` limita continuaciones o rondas de prerrequisito dentro de un intento. ELK Pirots 5 Boss reúne primero los tokens mediante apuestas de progresión nativas; puede necesitar más de 256 pasos. Para una campaña que incluya esos modos, se puede aumentar el límite, por ejemplo a `2048`. Llegar al límite conserva la captura como incompleta; no fabrica el desbloqueo.

Para una ejecución directa, sin VPN, debe existir una salida explícita como `{"id":"direct"}` y no debe tener `namespace` ni `wireguard_config`. No hay salida directa automática cuando falla un túnel.

## Ejecutar

Desde la raíz del repositorio:

```bash
# Inventarios, cobertura y cupos. No abre sesiones ni hace solicitudes a proveedores.
capture-runner/target/release/capture-runner plan --config capture-runner/config.local.json

# Mostrar también cada combinación pendiente de implementación.
capture-runner/target/release/capture-runner plan --config capture-runner/config.local.json --details

# Inspeccionar comandos de red sin ejecutarlos ni imprimir claves privadas.
sudo capture-runner/target/release/capture-runner wg plan --config capture-runner/config.local.json

# Crear los namespaces y levantar todas las salidas configuradas.
sudo capture-runner/target/release/capture-runner wg up --config capture-runner/config.local.json

# Ejecutar todos los proveedores/salidas en paralelo.
sudo capture-runner/target/release/capture-runner run --config capture-runner/config.local.json

# Consultar eventos acumulados.
sudo capture-runner/target/release/capture-runner status --config capture-runner/config.local.json

# Después de detener los workers, retirar los recursos de red propios.
sudo capture-runner/target/release/capture-runner wg down --config capture-runner/config.local.json
```

`run` no modifica la red ni levanta túneles por sí mismo. Una salida que no esté disponible se informa y las demás siguen. Ctrl-C/SIGTERM cancelan los intentos abiertos, conservan lo recibido y detienen los workers. Los namespaces quedan disponibles hasta `wg down`.

## Qué se guarda

Cada intento tiene una carpeta privada bajo:

```text
<data_root>/<run_id>/<salida>/<proveedor>/<juego>/<modo>/sample-N/attempt-UUID/
```

Los componentes de juego/modo incluyen un sufijo hash para admitir claves con barras, espacios o signos sin colisiones de ruta.

| Archivo | Contenido |
|---|---|
| `attempt.json` | Juego, modo, muestra, salida, inicio e identidad del inventario |
| `wire.jsonl` | Solicitudes HTTP, cabeceras, fragmentos de respuesta, frames WebSocket, sesiones y notas nativas, en orden |
| `result.json` | Estado del intento, cierre nativo reconocido o motivo del fallo |
| `events.jsonl`, en la carpeta de salida | Evolución de todos los trabajos de ese worker |
| `alerts.jsonl`, en la carpeta de salida | Eventos desconocidos, con proveedor, juego, modo, motivo y ruta de captura |
| `summary-UUID.json`, en la carpeta de salida | Contadores al terminar esa ejecución del worker |

Los bytes HTTP y binarios WebSocket se guardan en base64. Un mensaje se conserva **antes de interpretarlo**. Los errores de lectura conservan los fragmentos ya recibidos. El diario registra el intento de envío antes de transmitir; si falta una respuesta, eso no prueba que el servidor no procesara la solicitud. Los archivos contienen sesiones y payloads originales y tienen permisos privados.

La alerta sale también por `stderr`, inmediatamente. Se dispara ante acciones, estados, formatos o versiones de protocolo que el conector identifica como desconocidos. Detiene ese intento y permite continuar otros juegos. Un rechazo conocido del proveedor y un fallo de transporte se registran con categorías distintas.

RubyPlay conserva los binarios completos y sigue los controles HTTP documentados para 179 juegos: versión, identidad, secuencia, acción siguiente y elecciones observadas. Los eventos internos que no cambien esos controles pueden permanecer opacos. African Cats conserva su decodificador específico para el modo normal. Una respuesta de inicio puede omitir `sessionData`; las continuaciones que lo requieren deben traerlo.

## Banco y reproducción local

Los comandos `export`, `import-ruby` y `replay` permiten usar las capturas sin contactar a los proveedores:

```bash
# Copiar capturas completas y parciales a un banco nuevo, con SHA-256 por archivo.
capture-runner/target/release/capture-runner export --input /var/lib/capture-runner/captures/captura-001 --output /var/lib/capture-runner/bancos/captura-001

# Importar las capturas históricas completas de RubyPlay.
capture-runner/target/release/capture-runner import-ruby --input /ruta/ENTREGA-CLAUDE/2-evidencia-claude/rubyplay/capture --output /var/lib/capture-runner/bancos/ruby

# Listar intentos; elegir un id para inspección/reproducción.
capture-runner/target/release/capture-runner replay --bank /var/lib/capture-runner/bancos/ruby
```

El replay compara cada solicitud con la grabada antes de entregar sus respuestas. Conserva las elecciones, cuerpos binarios, fragmentos y errores. No reescribe automáticamente identificadores ni inventa resultados después de agotar una secuencia. La guía [REPLAY.md](docs/REPLAY.md) explica el formato, la generación de solicitudes y el replay por CLI/API. Para ver el juego con su interfaz original hacen falta sus recursos compatibles.

## Estados y reanudación

| Estado | Significado |
|---|---|
| `complete` | El protocolo reconoce el cierre de la secuencia solicitada |
| `unknown_event` | El flujo necesita una acción, estado, formato o versión que no se reconoce |
| `transport_error` | Conexión, timeout o recepción incompleta |
| `provider_error` | Rechazo del proveedor, requisito no ofrecido u otro fallo del intento |
| `unsupported` | Inventariado, pero sin conector/selector completo en esta entrega |
| `interrupted` | Cancelación durante el intento |

El saldo, la moneda y los premios se conservan cuando llegan en el payload. **No son requisitos de aceptación, exportación o continuidad.** La finalización reconocida tampoco certifica que se haya encontrado toda rama aleatoria posible del juego.

Al repetir el mismo `run_id`, una muestra que ya tiene un intento se omite, incluso si quedó parcial o falló. Eso evita reenviar una acción de resultado incierto. Los trabajos sin intento todavía pueden continuar. Para una nueva pasada deliberada, usar otro `run_id`. Los inventarios cambiados requieren otro `run_id`. Un bloqueo por archivo impide dos workers simultáneos sobre la misma combinación de ejecución/salida.

Los modos sin conector quedan en `plan --details` y en los eventos de ejecución. El código de salida es `0` cuando todos los trabajos elegidos terminan sin incidencias, `2` cuando hay cobertura pendiente, errores, eventos desconocidos o cancelación, y `1` ante errores de configuración/infraestructura previos al resultado. Un fallo no cancela las demás salidas.

## Cobertura y procedencia

Los 35 proveedores y sus juegos se leen del inventario, sin filtrar por `ready`, `campaign_eligible` o conciliaciones anteriores. Games Global agrega dos selectores de su inventario individual. El suplemento IronDog incorpora cuatro juegos y 22 modos identificados en la nueva entrega, preservando los archivos de evidencia originales y registrando el hash del suplemento en la identidad del catálogo. La cobertura ejecutable se calcula mediante la misma tabla que despacha los conectores; no se confunde una fila de inventario con un protocolo implementado.

Se incorporaron también los protocolos completos conservados en referencias: NoLimit, ELK, BGaming legacy/RPC/Cocos, mesas y clásicos de Play’n GO, y los selectores de Yggdrasil respaldados por el traspaso. La entrega adicional del 15 de noviembre amplía Betsoft, IronDog, RubyPlay, Kendoo, 1spin4win, Mascot y Amusnet. Hay contratos ejecutables para **14.787 de 20.090 modos**, en 6.957 juegos. Veintiún proveedores cubren todos sus modos identificados. La tabla de los 35 proveedores está en [COVERAGE.md](docs/COVERAGE.md).

Algunas familias conservan únicamente un juego o un subconjunto de modos implementable con los archivos entregados. RubyPlay dispone de 179 juegos distintos por salida y 255 modos, con elecciones observadas y cuerpos completos. Playtech incorpora 100 modos de 48 juegos mediante el motor GPAS portado y validación de fuentes/configuración; las familias incompletas siguen pendientes. Kalamba cubre sus 66 normales identificados. [MISSING-EVIDENCE.md](docs/MISSING-EVIDENCE.md) lista las rutas concretas, los límites de cada familia y qué material nuevo ya fue aprovechado. Ver también [PROVENANCE.md](docs/PROVENANCE.md) y `plan --details`.

## Verificación local

```bash
cargo fmt --manifest-path capture-runner/Cargo.toml --check
cargo test --locked --manifest-path capture-runner/Cargo.toml
cargo clippy --locked --manifest-path capture-runner/Cargo.toml --all-targets -- -D warnings
```

Las pruebas usan archivos temporales, servidores locales y comandos simulados. Comprueban cupos independientes por proveedor/salida, conservación de respuestas parciales, alertas, continuidad después de un evento desconocido, reanudación sin reenvío, configuración de 30 salidas y propiedad/rollback de recursos de red. No solicitan apuestas a proveedores ni levantan túneles reales.

Los resultados de las pruebas y los límites de la verificación están en [VERIFICATION.md](docs/VERIFICATION.md).
