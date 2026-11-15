# Sesiones demo: Playwright y reproducción mediante HTTP

## Resultado y alcance

El usuario pidió iniciar los juegos con Playwright, observar cómo crean su sesión y reproducir ese contrato mediante HTTP/WS. Se inspeccionaron **12 juegos de los seis proveedores que mostraban 403** en la [campaña anterior](LIVE-SMOKE-2026-11-15.md). Se corrigieron diferencias comprobadas del runner y se ejecutó una muestra nativa de los mismos juegos con contextos HTTP nuevos.

**Resultado HTTP: 12 intentos terminados, 12 rechazos 403 durante el lanzamiento/login, cero giros enviados y cero rondas cerradas.** Los campos corregidos están implementados; el acceso del runner a estos juegos sigue sin resolverse. El banco conserva los doce intentos parciales y sus 34 solicitudes para inspección y replay local.

La inspección no sostiene que todos los 403 se debieran a ausencia de sesión. KA Gaming inició ambos juegos sin cookies. Red Tiger Panda envió una cookie recién emitida y recibió igualmente un 403. Un identificador de lanzamiento, una autenticación aceptada y una ronda cerrada son etapas diferentes.

## Ejecuciones identificadas

| Etapa | Commit / ejecución | Alcance |
|---|---|---|
| Navegador | `5ed80876a13fbca94e26757207f8047d371c4de1`; [run 43695833003](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43695833003) | 12 contextos nuevos, seis proveedores simultáneos y un juego por proveedor a la vez |
| Fuente Kendoo adicional | `a1cb6677475e1d86513386f93e5f7527a3ffa1d6`; [run 43696363705](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43696363705) | GET de la URL pública exacta observada; no abrió otra sesión |
| Contratos HTTP corregidos | [317fe6f87b5726ac3cae3ec69751809adf007f3c](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/commit/317fe6f87b5726ac3cae3ec69751809adf007f3c) | Correcciones descritas abajo, sin cambios de cobertura |
| CI inicial del código | [run 43697499119](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43697499119) | Correcto: formato, tests, clippy y compilación release de `317fe6f` |
| Corrección del disparador y CI del binario utilizado | `abe03e10499e3988dd3d90fc8e552c85a99d6697`; [run 43698065416](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43698065416) | Mismo código Rust; nuevo identificador operativo `r2` |

Playwright **1.62.1**, Chromium **151.0.7922.34**, Node **22.23.4**, Linux con Xvfb. Captura del navegador: **08:43:58–08:45:04 UTC** del 15 de noviembre de 2026, 66 segundos. Se guardaron 1309 solicitudes y 1234 respuestas HTTP, con límites explícitos de cuerpos. El HAR no incorpora los cuerpos: los cuerpos seleccionados están en archivos separados. La sonda no solicitó giros, compras ni envíos manuales de WebSocket.

El entorno local no resolvía los hosts y Firetrace no estaba conectado; las ejecuciones remotas se hicieron en GitHub Actions. El navegador y el runner usaron ejecuciones distintas: no se presume igualdad de IP, geografía ni huella TLS.

## Qué mostró el navegador

| Proveedor / juegos | Hecho comprobado | Límite de la conclusión |
|---|---|---|
| Evoplay: `venicefestival`, `treeoflightfabledfruits` | Emisor 302 con identificador demo nuevo; documento siguiente 403 de Cloudflare. | No se alcanzó init. El HTML de bloqueo no demuestra una cookie faltante. |
| Games Global: `GoldBlitzExtreme` | Login y authenticate 200; token emitido y SignalR conectado. El preloader/los assets de `contentioa.gcontent.eu` fallaron con `ERR_CERT_AUTHORITY_INVALID`. | No hubo Refresh ni inicialización de juego completa. SignalR: 2 frames enviados (`pushhub.authenticate`/`subscribe`) y 9 recibidos; ningún giro. |
| Games Global: `12MasksofFireDrums` | Redirector 403 con desafío Cloudflare y carga de Turnstile. | No llegó al login nativo. |
| KA Gaming: `TropicalHoliday`, `ThiefDog` | `startGame` 200, estado demo aceptado y campos nativos de sesión. Cero Cookie/Set-Cookie observados y cero cookies finales. | Demuestra inicialización por ese cliente; la sonda no probó giros. |
| Kendoo: `mariachi-lock`, `coin-dragon` | Token extraído del launcher; login 403 de Cloudflare. | No hubo login aceptado. Los identificadores del launcher no bastan para certificar sesión de juego. |
| Red Tiger: `pandagoesboom000` | Emisor 200, entry 302, frontend 200. `/setup` y `/style` enviaron `EVOSESSIONID` y recibieron 403 que eliminaron esa cookie; `/setup` también llevaba `X-Fingerprint`. | Causa inicial del rechazo no confirmada. El posterior `EV.7` apareció después de logout/invalidez y no explica por sí solo el primer 403. |
| Red Tiger: `happycappy000000` | Emisor 200, entry 302 y siguiente entry 500. | No llegó a frontend/setup. El 500 pertenece a entry, no al emisor. |
| ThreeOaks: `lava_coins_2`, `lady_fortune` | El cliente repitió login 25 y 24 veces respectivamente; falló CORS desde los primeros segundos, sin `Access-Control-Allow-Origin`. | HAR status −1 y ausencia de cabeceras de respuesta: no se conoce el status HTTP real desde el navegador. Los 200 de otros recursos no son un login aceptado. |

### Correcciones a la clasificación automática

El resumen bruto de la primera sonda marcó los cuatro casos Evoplay/Kendoo como `captcha` por el comentario HTML `captcha-container`. La revisión identificó páginas genéricas de bloqueo Cloudflare, sin widget CAPTCHA observado. Se conserva el resumen original y se publica la clasificación revisada en [session-bootstrap-2026-11-15.json](session-bootstrap-2026-11-15.json). El clasificador ahora ignora comentarios, scripts y estilos antes de buscar señales específicas.

La comparación HTTP adicional de la sonda hizo sólo GET de documentos de cliente, con un contexto limpio y otro con cookies del navegador. No probó RPC ni apuestas. En Red Tiger Panda se realizó después de eliminar `EVOSESSIONID`; por ello **no demuestra que una sesión válida no pueda transferirse a HTTP**. ThreeOaks no quedó simplemente esperando carga y el error de certificado de Games Global no se resolvía esperando más tiempo.

## Cambios implementados y su evidencia

| Componente | Cambio | Comprobación |
|---|---|---|
| Transporte común | Respeta un `User-Agent` explícito, sin añadir el predeterminado como segundo header. | Regresión TCP de los bytes enviados y del journal, incluyendo variantes de mayúsculas. |
| KA Gaming | Motor 2101 / aplicación `1.0.269 (2101)` para los dos juegos observados; el resto conserva 2090. `init.to` procede del parámetro `t` del launcher. | Firma de ambos init reales contrastada con el JS publicado; oráculo JS con contextos sintéticos, ambas ramas de versión y Unicode. Fuentes y menús verificados; no se amplió la cobertura. |
| Kendoo | `?gsc=<command>`, `Content-Type: text/plain`, `request_id` UUID v4 de 32 caracteres hexadecimales. | Tráfico de login y fuente oficial `gr.js`; fixture TCP de login/start/play, IDs nuevos y rechazo de correlación incorrecta. Allowlist restringida a esos comandos. |
| ThreeOaks | `?gsc=<command>` y `Content-Type: text/plain`; conserva UUID y correlación nativa. | Tráfico y fuente oficial `gr.js`; mismo contraste TCP de solicitudes y respuestas. |
| Games Global | `authenticate` usa Origin de la API, `clienttypeid: 70` y Referer del iframe de comunicaciones, derivado del producto/módulo/cliente. | Fixture con cabeceras públicas observadas y prueba de derivación. Login y Refresh conservan su contexto propio. |
| Red Tiger | Genera 32 bytes con CSPRNG, los codifica en Base64 y envía `X-Fingerprint` sólo a `/setup`. | La fuente oficial hace `crypto.getRandomValues(new Uint8Array(32))`; prueba de renovación, formato y aislamiento respecto de `/config`. |

En Red Tiger el navegador observado envió `wrapped=true` y el runner conserva `wrapped=false`: el cliente decide ese campo desde el User-Agent, y el UA explícito del runner difiere legítimamente del UA reducido de Chromium. No se copiaron cookies, tokens ni fingerprints históricos al runner. Los hashes y fixtures están en [PROVENANCE.md](../capture-runner/docs/PROVENANCE.md).

## Resultado del runner nativo

El primer disparo, [43697904839](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43697904839), terminó en verde pero **omitió la captura y no generó artifact**: la imagen remota carecía de `rg` y el error dentro del condicional se interpretaba como ausencia de cambios. Se reprodujo localmente esa condición y se corrigió con `git diff` y comparación Bash en [abe03e1](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/commit/abe03e10499e3988dd3d90fc8e552c85a99d6697). La comprobación rechaza un SHA incorrecto. La solicitud nueva se identifica como `session-http-20261115-r2`; este cambio no modifica el Rust de `317fe6f`. La ejecución omitida cuenta como cero intentos, no como doce éxitos.

[Ejecución 43698411683](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43698411683), commit operativo `abe03e1`, binario del [CI 43698065416](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43698065416), ambos identificados en el JSON. Captura **09:16:12–09:16:13 UTC**. El workflow terminó con **exit 2 / failure** porque la campaña quedó parcial; ejecución, cifrado y subida de evidencia se completaron correctamente.

| Proveedor | Resultado de los dos juegos | Qué quedó verificado en la llamada real |
|---|---|---|
| Evoplay | Emisor 302 → documento del juego 403, bloqueo genérico Cloudflare. | UA único; no se alcanzó init. |
| Games Global | Emisor 307 → redirector 403, desafío Cloudflare. | No llegó a login, authenticate, Refresh ni SignalR; las nuevas cabeceras de authenticate no se ejercitaron. |
| KA Gaming | Fuentes 2101 descargadas y verificadas; startGame 403, desafío Cloudflare. | Versión, fuentes y parámetros de init aplicados. Ambas firmas coinciden con el firmante oficial 2101 sobre los cuerpos reales; el control 2090 no coincide. Ningún init aceptado por HTTP. |
| Kendoo | Página y launcher 200 → login 403, bloqueo genérico Cloudflare. | `gsc=login`, text/plain, UUID v4 hex32, timestamp y token del launcher; se envió `__cf_bm`. No llegó a start/play. |
| Red Tiger | Página y emisor 200 → primer entry 403, HTML Access Denied de Akamai. | UA único. No alcanzó frontend/setup, por lo que no se ejercitó X-Fingerprint en este retest. |
| ThreeOaks | Metadata y launcher 200 → login 403, bloqueo genérico Cloudflare. | `gsc=login`, text/plain, UUID v4 hex32, timestamp y token; no llegó a start/play. |

Total: **34 solicitudes/respuestas HTTP**: 18×200, 2×302, 2×307 y 12×403. Cero conexiones WS y cero acciones de ronda. Los 12 resultados son `provider_error`, categoría del runner que no determina por sí sola la causa. No hubo errores de lectura, intentos inacabados ni errores de transporte. El solapamiento de intervalos de intento alcanzó 12 juegos / 6 proveedores y máximo 2 juegos por proveedor, en un intervalo de 1188 ms; no equivale a solicitudes exactamente simultáneas.

### Diferencias que todavía requieren un contraste controlado

- **Kendoo:** el token coincide con el histórico del navegador, pero se comprobó que el runner lo extrae del HTML 200 recién descargado y que coincide con la URL del launcher. No se copió desde la captura y no debe afirmarse que el proveedor emitió un token único diferente.
- **ThreeOaks:** el runner envía cookies al login cross-origin; los headers registrados por Chromium no contienen Cookie y `gr.js` no configura `withCredentials`. Es una diferencia restante, sin causalidad del 403 demostrada. Tampoco convertir el fallo CORS del navegador en un status 403 observado retrospectivamente.
- **Identidad y red:** el UA del runner, sus headers y transporte difieren de Chromium. Las ejecuciones tuvieron contextos y salidas independientes. No se aisló el efecto de UA, IP, geografía, TLS ni cookies; no afirmar una causa concreta sin una comparación en el mismo entorno.
- **Red Tiger / KA:** el Referer de entry conserva en Rust la ruta/query de la demo, mientras Chromium envía el origen. KA declara `ctx.dt/dv` del runner, no Chrome; sus firmas son correctas para esos valores. Son diferencias observadas sin prueba de que expliquen el rechazo.
- **Etapas no alcanzadas:** la corrección de authenticate Games Global y X-Fingerprint de Red Tiger permanece contrastada con fuente/fixtures, pero no obtuvo validación remota en esta muestra. Los comandos start/play Kendoo/ThreeOaks tampoco se alcanzaron.

El journal registra la solicitud construida antes de `reqwest.execute` y la respuesta recibida; no captura TLS ni todos los headers agregados por el transporte.

Las respuestas son páginas de control de acceso y no errores JSON nativos de sesión. Esto localiza dónde se bloquea el flujo, pero no identifica la regla que provoca el rechazo ni demuestra que los 403 se deban sólo a cookies ausentes.

La configuración [session-http-request.json](../capture-runner/operations/session-http-request.json) limita la campaña a seis proveedores, dos juegos por proveedor, una muestra normal por juego, una salida directa, interrupción a los 240 segundos y terminación forzada hasta 20 segundos después. `0` es el selector normal de Red Tiger; los demás usan `normal`. El workflow descarga el binario del CI exitoso del mismo commit y conserva también los intentos parciales.

## Verificación y evidencia privada

Suite local del código corregido: **274 correctas, 0 fallidas, 2 opt-in ignoradas**; fmt, clippy y release correctos. Los CI `43697499119` y `43698065416` también terminaron correctamente. Las pruebas opt-in omitidas requieren los originales Ruby y no se volvieron a ejecutar para estos cambios. [Detalle de verificación](../capture-runner/docs/VERIFICATION.md).

El artifact original del navegador, `session-bootstrap-403-encrypted`, tiene ID `14153194972`, 54749826 bytes y SHA-256 `64810432af8c29d134c7f78b96fd816b5823f9091e60f70cc7b8f4efb2749ea2`. El source adicional Kendoo se conservó en `kendoo-gamerunner-source-encrypted`, ID `14155040403`; ZIP SHA-256 `c4fc500007294c667bb1906de591a525ae1ffec6ec78524db7b37f52ed286794`. Los artifacts cifrados tienen siete días de retención; los IDs no sustituyen al paquete privado entregado.

Paquete privado: **`session-bootstrap-evidence-20261115.zip`**. Incluye evidencia de red del navegador, fuente adicional Kendoo, salida original HTTP y banco portable de **12 parciales / 34 solicitudes**. Se verificaron los **36 hashes** de archivos de intentos y se reprodujeron los doce offline: las 34 solicitudes fueron aceptadas por el replay y todos terminaron con exit 2, coherente con su estado parcial. El paquete no transforma estos rechazos en partidas cerradas.

Tamaño: **56692142 bytes**; **1020 archivos** y **1019 hashes internos verificados**, además del CRC del ZIP. SHA-256 del ZIP:

```text
91de415b719c2424c7f61e8418b573472b887251a1491de85de37c0a88294475
```

El manifiesto interno tiene SHA-256 `5f5471947118dc2c063b2436b435ac74c5c5372862c6637d05ec3cd8d87df2b4`.

El artifact HTTP original es `session-http-12-encrypted`, ID `14154869709`, 2022048 bytes, SHA-256 `a7449964634ce6ed3a8b45914a9247d492e9247f0268dc619dedb2e0f821278b`. El SHA-256 del ejecutable utilizado fue `faceddbd1e234c09eb2147affcd553b2e8dce00c9423de6d2e2de4010af179a6`. Los cuerpos, cookies y tokens quedan en el paquete privado; Git contiene el informe saneado, contratos y fixtures.

## Cómo retomar sin repetir la investigación

1. Leer [HANDOFF.md](HANDOFF.md), este informe y el JSON de casos; conservar la distinción entre inicialización, cierre y fallo de acceso.
2. Para un fallo concreto, abrir sólo su directorio de evidencia y su adaptador. Las fuentes oficiales ya están identificadas por hash; no repetir el análisis completo de 35 proveedores.
3. Las capturas históricas sirven para análisis y replay local. Para contactar al proveedor, crear una sesión nueva; no reenviar los tokens de las grabaciones.
4. El [procedimiento operativo](../capture-runner/operations/SESSION-BOOTSTRAP.md) explica las sondas y los triggers. Cambiar `session-http-request.json` solicita otra campaña después del CI; un cambio documental no la solicita.
5. Permanecen fuera de esta corrección los fallos de Belatra, ELA, Playson, Playtech y las continuaciones pendientes de la campaña 35×2. Los **35 proveedores, 9878 juegos, 20090 modos, 14787 modos implementados y 44 bindings** mantienen su alcance anterior.
