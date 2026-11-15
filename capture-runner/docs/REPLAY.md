# Exportación y reproducción local

El banco conserva los mensajes nativos y reproduce una secuencia grabada cuando recibe sus solicitudes en el orden correcto. Todo funciona localmente en el ejecutable Rust; estos comandos no abren sesiones ni hacen llamadas a proveedores.

Los ejemplos se ejecutan desde la raíz del repositorio, después de compilar con `cargo build --release --locked --manifest-path capture-runner/Cargo.toml`. Usan la ruta del binario generado; si se define `CARGO_TARGET_DIR`, ajustar esa ruta. Para el estado del desarrollo, leer el [traspaso](../../docs/HANDOFF.md).

## 1. Exportar capturas del runner

```bash
capture-runner/target/release/capture-runner export --input /var/lib/capture-runner/captures/captura-001 --output /var/lib/capture-runner/bancos/captura-001
```

También se puede seleccionar la carpeta de un proveedor, un juego o un intento. El destino debe ser nuevo y estar fuera del árbol de origen. El banco incluye los intentos completos, rechazados e interrumpidos. Una línea final truncada se copia íntegra y se señala como incidencia. El exportador copia un prefijo acotado si el archivo sigue creciendo; lo marca parcial cuando detecta el cambio. No necesita balances ni premios para exportar.

`index.json` identifica proveedor, juego, modo, muestra, estado y archivos de cada intento. Bajo `attempts/<id>/` se conservan `attempt.json`, `wire.jsonl` y `result.json` cuando existen, con tamaño y SHA-256. Los cuerpos HTTP, fragmentos, frames binarios y notas nativas mantienen los bytes guardados originalmente. Se verifica la integridad antes de reproducir. Los directorios y archivos nuevos conservan permisos privados.

## 2. Importar la evidencia existente de RubyPlay

Tras extraer el ZIP entregado, indicar su directorio `2-evidencia-claude/rubyplay/capture`:

```bash
capture-runner/target/release/capture-runner import-ruby --input /ruta/ENTREGA-CLAUDE/2-evidencia-claude/rubyplay/capture --output /var/lib/capture-runner/bancos/ruby-historico
```

También acepta un único `rp_*.json`. El conjunto revisado contiene 179 juegos y 434 intentos: 432 completos y 2 parciales/rechazados, con 1491 pares aceptados de solicitud/respuesta. Se genera un intento por ronda, identificado por juego, modo e índice original.

Estos archivos contienen objetos JSON nativos ya procesados: se importan como `native_request` y `native_response`. Se preservan `binary`, `funModeData`, `sessionData`, versiones, errores, contexto y SHA-256 de la fuente. No se fabrican cabeceras HTTP ni tiempos de red. `at_ms` indica el momento de importación; los tiempos del proveedor permanecen dentro de sus mensajes.

Varias rondas proceden de una misma sesión. El importador conserva el `an` y el estado reales de cada primera solicitud. La inicialización original queda como contexto consultable, sin presentarla como una sesión nueva de cada ronda. Una respuesta final aceptada con `next_action=spin` y la declaración de cierre original permiten clasificar el intento como completo; los datos financieros no intervienen.

Las dos parciales son `rp_120` y `rp_171`, `buy_feature:freespin`, rechazadas con `M-0`. Se conserva su contexto/error con resultado `provider_error`; contienen cero pares de acción reproducibles. Agotar esas entradas no equivale a reproducir una compra cerrada y devuelve código 2.

### Banco Ruby entregado el 15 de noviembre de 2026

`rubyplay-capture-bank-20261115.zip` contiene los 434 intentos ya importados, la carpeta `rubyplay-bank-20261115/`, `LEEME.md` y esta guía en su versión de la entrega. Es un archivo externo entregado al usuario, no un archivo versionado ni una dependencia del runner. Ocupa **2363325 bytes** y su SHA-256 es:

```text
2f08ecad4668ead0c986aee440d66ab754cfe29208a35d7cf8b98ea7100cfae1
```

Extraerlo antes de usar `--bank`: el argumento debe apuntar al directorio que contiene `index.json`, no al ZIP. Se comprobaron los 1302 hashes de archivos de intentos y la integridad del ZIP. Si sólo se dispone de los originales, el comando `import-ruby` anterior reconstruye el banco; los tiempos de importación y el ZIP resultante pueden diferir, por lo que este hash identifica únicamente el archivo entregado. La procedencia del ZIP fuente está en [PROVENANCE.md](PROVENANCE.md).

## 3. Elegir y reproducir un intento

Listar los intentos y copiar su `id`:

```bash
capture-runner/target/release/capture-runner replay --bank /var/lib/capture-runner/bancos/ruby-historico
```

Inspeccionar contexto y solicitudes de un intento:

```bash
capture-runner/target/release/capture-runner replay --bank /var/lib/capture-runner/bancos/ruby-historico --attempt ID
```

Emitir las solicitudes grabadas y reproducirlas:

```bash
capture-runner/target/release/capture-runner replay --bank /var/lib/capture-runner/bancos/ruby-historico --attempt ID --emit-requests > solicitudes.jsonl
capture-runner/target/release/capture-runner replay --bank /var/lib/capture-runner/bancos/ruby-historico --attempt ID --requests solicitudes.jsonl
```

`--requests -` lee JSONL de la entrada estándar. Cada línea tiene `event` y `data`. Ejemplo del formato importado:

```json
{"event":"native_request","data":{"action":"pick","index":2,"an":7,"key":"[REDACTED]","funModeData":{}}}
```

El ejemplo sólo muestra la estructura; la solicitud efectiva debe conservar todos los campos capturados. El resultado se emite en JSONL: `replay_start`, un `replay_step` por solicitud aceptada y `replay_finish`. Los pasos contienen los eventos originales recibidos hasta la siguiente solicitud. El inicio de la reproducción muestra identidad y observaciones iniciales; los resúmenes futuros de resultado se consultan por separado mediante inspección.

## Reglas de correspondencia

La comparación es estricta. No se ignoran apuesta, modalidad, índice de elección, versión, contador, URL, cabeceras ni cuerpos. Sólo se omiten las anotaciones `label` y `round_action` del transporte; en mensajes `native_request` todos los campos nativos participan. Una solicitud distinta falla sin avanzar ni entregar la respuesta siguiente. La API `ReplaySession` permite corregirla y volver a intentar sobre el mismo cursor.

Los identificadores y contadores deben coincidir con la grabación; esta versión no reescribe automáticamente tokens ni datos opacos. Cada sesión de replay tiene su propio cursor. Al agotarse la secuencia no se inventan resultados nuevos.

| Salida | Significado |
|---|---|
|0|Exportación/inspección correcta, o replay agotado de un intento con cierre completo|
|1|Error de formato, integridad, ruta o solicitud distinta de la esperada|
|2|Replay conservado pero incompleto: faltan solicitudes, el original quedó parcial o el diario termina truncado|

## API Rust

[`capture_bank`](../src/capture_bank.rs) ofrece `export`, `load` y `summary`; [`ruby_import::import`](../src/ruby_import.rs) convierte las capturas originales. Con un `BankAttempt` del índice, [`ReplaySession::open`](../src/replay.rs) verifica sus archivos y abre un cursor independiente. `expected_request` devuelve la siguiente solicitud; `submit` la compara y entrega los eventos siguientes. `complete` exige cierre original, evento terminal y ausencia de truncamiento; `exhausted` por sí solo no demuestra cierre. `inspect`, `emit_requests` y `run` implementan la inspección y las entradas/salidas JSONL de la CLI. Ver los [contratos de interfaces](../INTERFACES.md).

## Alcance

El reproductor entrega mensajes y permite inspeccionar la lógica observada de una partida. Para mostrar el juego con su interfaz original hace falta un cliente compatible, sus recursos y una adaptación de sus conexiones al banco local. Esos recursos no forman parte de esta entrega.

Los flujos saneados de Playtech conservan comandos y estados parciales, pero sus `payload` comprimidos o `playStack` están recortados. No se importan como respuestas completas. Las próximas capturas del runner sí guardan los cuerpos íntegros antes de interpretarlos.
