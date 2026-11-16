# Rondas completas: diez juegos por proveedor con cierres previos

## Solicitud y selección

El usuario pidió ampliar a unos diez juegos por proveedor que funcionó en la prueba anterior. Se consideran elegibles los proveedores con al menos un cierre nativo en [`docs/live-smoke-2026-11-15.json`](../../docs/live-smoke-2026-11-15.json): **21 proveedores**, 13 que cerraron toda su muestra anterior y 8 que cerraron una parte.

- **210 juegos distintos**, diez por proveedor. Todos tienen al menos un modo implementado.
- Se conservan los **41 juegos con algún cierre previo** de esos proveedores y se añaden otros para cubrir tipos de modalidad y cuartiles de cantidad de modos. Los desempates usan SHA-256 estable. No se eliminan los modos que fallaron antes.
- **582 modos a intentar:** 210 normales, 224 compras, 55 boosters, 25 antebets y 68 variantes. Una muestra por modo.
- **11 modos inventariados sin implementación**, diez variantes Betsoft y una compra Spinomenal, se declaran aparte. No crean intentos y no son errores de una llamada remota.
- RubyPlay incluye compras freespin, respin, minispin y select. Las compras y continuaciones se ejecutan sólo cuando el contrato y la oferta nativa lo permiten.

La [selección completa](complete-rounds-10-selection.json) fija IDs, modos, motivos, hashes y conteos. Para consultar un proveedor sin cargarla entera:

```bash
jq '.providers[] | select(.provider == "rubyplay")' capture-runner/operations/complete-rounds-10-selection.json
```

## Ejecución

La [solicitud](complete-rounds-10-request.json) usa salida `direct`, los 21 proveedores simultáneos y **dos juegos activos por proveedor**: 42 cupos. Los diez juegos de cada proveedor pasan por esos cupos; dentro de un juego sus modos son secuenciales.

Cada intento crea un contexto independiente y solicita su sesión al proveedor. El runner conserva requests, responses y continuaciones hasta el marcador nativo de cierre, o registra el motivo de interrupción/error. No exige conciliación de saldos ni premios.

`max_steps: 512` es un presupuesto interpretado por cada adaptador; algunos conservan topes internos menores. No representa 512 solicitudes idénticas para todos los protocolos. `timeout_seconds: 20` se aplica por solicitud/intercambio, no a la ronda entera. Una rama desconocida queda parcial: aumentar el límite no implementa su continuación.

El workflow interrumpe a los 1200 segundos y fuerza la terminación hasta 20 segundos después. La gracia permite finalizar registros, no garantiza que una ronda activa termine naturalmente. El job tiene 30 minutos para incluir preparación, cifrado y upload.

## Binario verificado y controles

Se reutiliza explícitamente el ejecutable Linux de [CI 43699080917](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43699080917), commit **`af8f957cce4cd57fe633da50b4620116ed19c6e3`**, que pasó su CI. SHA-256:

```text
faceddbd1e234c09eb2147affcd553b2e8dce00c9423de6d2e2de4010af179a6
```

El workflow comprueba el estado, repositorio y SHA del CI, el hash del ejecutable y la igualdad de entradas de runtime con el checkout de la solicitud: fuentes, bindings, manifiestos Cargo y todo `standalone info/`, incluidos planes y manifiestos. Si cambia el runtime, la campaña falla antes de ejecutarlo: hay que verificar otro binario y actualizar ambos pines.

También valida el plan exacto: 21 proveedores, diez juegos ejecutables por proveedor, 582 modos implementados, 11 omitidos por cobertura, 42 cupos y hash de catálogo fijado. El commit de la solicitud/workflow y el commit fuente del binario se registran por separado.

## Activación, evidencia y resultado

[`complete-rounds-10.yml`](../../.github/workflows/complete-rounds-10.yml) se activa manualmente o cuando cambia únicamente la ruta de `complete-rounds-10-request.json` en `main`. Modificar documentación no solicita otra campaña.

Toda la salida disponible, incluso parciales y fallos, se cifra con [`complete-rounds-10-recipient.pem`](complete-rounds-10-recipient.pem). La clave privada permanece fuera de Git. Sólo se suben el archivo CMS y su SHA-256, con retención de siete días; la evidencia debe descargarse y conservarse en un banco privado antes de que venza esa retención.

Se preservan los códigos separados de planificación, runner y timeout. **Exit 2 no significa que todos los juegos fallaron:** también señala los 11 modos sin contrato y los intentos parciales. Para clasificar cierres se usa el estado nativo, el `attempt_finish` final y la integridad del journal/exportación. Los modos no iniciados se cuentan aparte.

Las acciones de ronda son intenciones HTTP/WS marcadas `round_action`; no equivalen automáticamente a solicitudes aceptadas ni a giros base. El banco también conserva inicializaciones, handshakes y mensajes auxiliares. La disponibilidad de un juego no se extrapola al catálogo entero.
