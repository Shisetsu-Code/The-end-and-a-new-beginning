# Inspección de sesiones demo con Playwright

Esta sonda observa el inicio de los mismos dos juegos de Evoplay, Games Global,
KA Gaming, Kendoo, Red Tiger y 3 Oaks que recibieron HTTP 403 en la
[campaña del 15 de noviembre](../../docs/LIVE-SMOKE-2026-11-15.md).
Su objetivo es contrastar el navegador oficial con el bootstrap HTTP del runner.
No certifica rondas completas ni modifica la cobertura de modos.
El [informe revisado del 15 de noviembre](../../docs/SESSION-BOOTSTRAP-2026-11-15.md)
registra los resultados del navegador y de la prueba HTTP por separado.

## Ejecución acotada

- Playwright **1.62.1** y su Chromium fijado, con pantalla virtual Xvfb.
- Seis proveedores simultáneos, un juego por proveedor; cada juego tiene un
  contexto nuevo y un máximo de 90 segundos.
- Navegación por las entradas demo oficiales y ejecución del cliente publicado.
  No solicita giros, compras ni envíos manuales de WebSocket.
- Conserva HAR, cabeceras, cuerpos relevantes, mensajes WS, almacenamiento,
  errores y capturas de pantalla. Los límites de cuerpos y las omisiones quedan
  registrados; no se presenta el HAR sin cuerpos como una captura íntegra.
- La comparación adicional repite sólo un GET de un documento de cliente
  observado, con un contexto HTTP limpio y con las cookies del navegador.
  Usa `APIRequestContext` de Playwright: no es reqwest ni demuestra por sí sola
  que el login o la apuesta funcionen mediante HTTP independiente.

El workflow [session-bootstrap-403.yml](../../.github/workflows/session-bootstrap-403.yml)
se activa manualmente o al cambiar únicamente `session-bootstrap-request.json`
en `main`. Revisar el alcance antes de cambiar esa solicitud. Una edición de
documentación no repite las navegaciones.

```bash
node capture-runner/operations/session-bootstrap-probe.cjs \
  --config capture-runner/operations/session-bootstrap-request.json \
  --output /ruta/nueva/diagnostico --validate

xvfb-run -a node capture-runner/operations/session-bootstrap-probe.cjs \
  --config capture-runner/operations/session-bootstrap-request.json \
  --output /ruta/nueva/diagnostico
```

El primer comando sólo valida la selección. El segundo contacta proveedores.
Requieren Playwright instalado y resoluble por Node; el workflow instala la
versión exacta. El directorio de salida debe ser nuevo.

## Evidencia y lectura de resultados

`private/` contiene material de sesión y nunca se publica en Git ni en logs de CI.
El workflow cifra toda la salida con el certificado público ya configurado y
sube sólo el artifact cifrado, con siete días de retención. La clave privada
permanece en los entregables privados de la campaña anterior.

`public/summary.json` contiene identificadores de casos, contadores y estados
observados. Revisar las respuestas nativas antes de concluir que existe una
sesión inicializada. Un 403 del navegador también es un resultado útil; no
convertirlo automáticamente en una corrección pendiente de cookies.

El cambio de transporte asociado evita agregar el `User-Agent` predeterminado
cuando el adapter ya proporciona uno. La regresión verifica la solicitud real
en un servidor TCP local y su journal. No presupone que ese defecto explique
los rechazos remotos.

## Comprobación posterior con el runner nativo

[`session-http-request.json`](session-http-request.json) selecciona exactamente
los mismos doce juegos y un normal por juego (`0` es el normal de Red Tiger).
[`session-http-check.yml`](../../.github/workflows/session-http-check.yml) espera
que la CI del commit solicitado termine correctamente y descarga el ejecutable
producido por esa misma ejecución. Sólo inicia capturas cuando ese commit cambia
la solicitud; una corrección documental posterior no repite la campaña.

La muestra usa sesiones HTTP nuevas, sin importar cookies ni tokens de la
inspección histórica de Playwright. Son doce intentos como máximo, con dos juegos
por proveedor, una salida directa, interrupción a los 240 segundos, terminación forzada hasta 20 segundos después
y evidencia cifrada. El resultado nativo se conserva: un exit 2 indica capturas parciales.

La inspección de la fuente Kendoo omitida por el primer HAR se reproduce con
[`inspect-kendoo-source.yml`](../../.github/workflows/inspect-kendoo-source.yml).
Ese workflow sólo descarga la URL pública exacta observada en Chromium; no
inicia nuevas sesiones ni partidas.
