# Prueba real de 35 proveedores: 2 juegos por proveedor

Realizada el **15 de noviembre de 2026, de 05:18:46 a 05:19:55 (Argentina, UTC−3)**, en Ubuntu de GitHub Actions con salida directa. [Ejecución 43693889120](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43693889120).

## Resultado

**13 proveedores completaron toda la muestra; 8 tuvieron resultados parciales; 14 no completaron ningún intento.** Se seleccionaron 70 juegos y se ejecutaron 69: Microgaming sólo tiene un juego con contrato ejecutable. Se conservaron todos los resultados y cuerpos recibidos.

| Medida | Observado |
|---|---:|
| Proveedores iniciados | 35 |
| Juegos seleccionados / ejecutados | 70 / 69 |
| Intentos de modalidad iniciados / finalizados | 202 / 202 |
| Cierres nativos completos | 109 |
| Errores clasificados como provider_error | 71 |
| Eventos/continuaciones no reconocidos | 22 |
| Errores de transporte en GitHub Actions | 0 |
| Modos normales cerrados / intentados | 40 / 69 |
| Juegos con algún modo completo | 41 |
| Juegos con todos sus modos soportados completos | 32 |
| Modos seleccionados sin implementación, omitidos | 3 |
| Duración de planificación y ejecución | 69 segundos |

`provider_error` también puede incluir un fallo del adaptador; no identifica por sí solo al responsable. HTTP 200 no cuenta como una apuesta completada: se exige el cierre nativo y su registro `completion`. Los 93 intentos restantes se conservan como parciales en el banco.

## Paralelismo y alcance

Un único worker de salida, con los 35 proveedores programados conjuntamente y `parallel_games=2` por proveedor. Los modos de cada juego fueron secuenciales, una muestra por modalidad soportada, sesiones nuevas y sin reintentos automáticos. La selección incluye normales, compras, antebets y boosters; cada juego ejecutado tiene entre una y cinco modalidades ejecutables.

Los intervalos registrados **se solapan hasta 69 juegos y 35 proveedores**, con máximo de dos juegos dentro de un proveedor; Microgaming llega a uno. El cálculo usa `[primer inicio de intento, último fin de intento)` por juego. Incluye huecos entre modalidades y no equivale a 69 solicitudes simultáneas. La ventana de esos intervalos abarca 66,919 segundos.

Se registraron 864 acciones marcadas como apuesta/continuación y 1599 eventos de salida: 1096 solicitudes HTTP, 456 mensajes WS y 47 aperturas WS. Esas acciones incluyen continuaciones; no son 864 partidas independientes. El registro se escribe antes del envío y su existencia no demuestra aceptación remota.

## Resultado por proveedor

«Modalidades» muestra cierres completos / intentos ejecutados. «Normal» es la cantidad de juegos cuyo modo normal cerró / juegos con normal intentado. El resultado completo sólo corresponde a esta muestra, no a todo el catálogo.

| Proveedor | Normal | Modalidades | Observación |
|---|---:|---:|---|
| 1spin4win | 2/2 | 5/5 | Todas las modalidades probadas completadas. |
| amigogaming | 2/2 | 5/5 | Todas las modalidades probadas completadas. |
| amusnet | 0/2 | 0/7 | Inicialización con ipBlocked en 4 intentos y HTTP 429 en otros 3. |
| belatra | 0/2 | 0/7 | Bug de comparación: modification numérico se compara con String y rechaza una identidad equivalente. |
| betsoft | 2/2 | 5/6 | Continuación BUY_CASCADE_FREE no reconocida en 945 / 0. |
| bgaming | 2/2 | 8/8 | Todas las modalidades probadas completadas. |
| egt-digital | 2/2 | 6/6 | Todas las modalidades probadas completadas. |
| ela-games | 0/2 | 0/6 | El conector rechaza el evento Socket.IO connected después de una apertura WS correcta. |
| elk | 2/2 | 6/6 | Todas las modalidades probadas completadas. |
| endorphina | 2/2 | 6/6 | Todas las modalidades probadas completadas. |
| evoplay | 0/2 | 0/6 | HTTP 403 con página de bloqueo/desafío Cloudflare antes de inicializar. |
| gamesglobal | 0/2 | 0/2 | HTTP 403 con control de seguridad antes de inicializar. |
| habanero | 2/2 | 6/6 | Todas las modalidades probadas completadas. |
| hacksaw | 2/2 | 6/7 | La modalidad bonus de 2670 llega a una elección/estado no implementado. |
| hardfire-kalamba | 2/2 | 2/2 | Todas las modalidades probadas completadas. |
| irondog | 0/2 | 0/6 | Launcher HTTP 500; el cuerpo declara error7000 y juego no disponible en el país. |
| kagaming | 0/2 | 0/4 | Init HTTP 403 con desafío Cloudflare. |
| kendoo | 0/2 | 0/7 | Login HTTP 403 con bloqueo/desafío Cloudflare. |
| mascot | 2/2 | 6/6 | Todas las modalidades probadas completadas. |
| microgaming | 0/1 | 0/1 | Practice HTTP 406: el servidor indica una cabecera ausente, sin identificarla. |
| netent | 0/2 | 0/6 | El emisor demo responde success=false: invalid response from API. |
| nolimit | 2/2 | 9/9 | Todas las modalidades probadas completadas. |
| peterandsons | 2/2 | 7/7 | Todas las modalidades probadas completadas. |
| playngo | 2/2 | 4/4 | Todas las modalidades probadas completadas. |
| playson | 0/2 | 0/6 | La fuente JS actual no coincide con el SHA-256 del binding; se detiene antes de apostar. |
| playtech | 0/2 | 0/4 | Los manifests de ambos juegos declaran env.glu6=true; ese bridge aún no está implementado. |
| pragmatic | 2/2 | 5/6 | vs10bookfallen / pur-1: demo session unavailable. |
| redtiger | 0/2 | 0/8 | demo_entry HTTP 403, Access Denied; causa de la política no determinada. |
| reelkingdom | 2/2 | 5/6 | vswaysfltdrg / pur-1: no se encontró una opción de bonus disponible y verificada. |
| rubyplay | 2/2 | 3/4 | rp_103 / buy_feature:freespin llega a pick sin parámetros revisados para continuar. |
| smartsoft | 2/2 | 6/6 | Todas las modalidades probadas completadas. |
| spinomenal | 2/2 | 4/6 | Una compra cambia la cantidad de free spins y otra exige una elección no implementada. |
| threeoaks | 0/2 | 0/10 | Login HTTP 403 con bloqueo/desafío Cloudflare. |
| thunderspin | 1/2 | 3/4 | Juego7 / NORMAL deriva a respins, aún no contemplado. |
| yggdrasil | 1/2 | 2/7 | Continuaciones C y opciones NON_GAMBLE/GAMBLE no reconocidas en los juegos7400/7408. |

## Hallazgos para continuar

1. **Belatra: bug localizado.** En [belatra.rs](../capture-runner/src/adapters/belatra.rs), la validación de `launch` admite `modification` numérico pero lo compara con una cadena. En las siete capturas coinciden juego, cifrado, SID y el valor textual de modification; se rechaza por la comparación de tipos. Corregir esa comparación conservando las demás verificaciones de identidad.
2. **ELA Games: evento de conexión no contemplado.** [ela_games.rs](../capture-runner/src/adapters/ela_games.rs) sólo admite `message`/`exception`; las seis capturas reciben `connected` después de WS 101. Hay que reconocer su función en el handshake y verificar el siguiente mensaje nativo. La notificación por sí sola no acredita un giro.
3. **Playson:** se recibió la fuente actual, pero su hash cambió. Revisar la fuente y el contrato antes de actualizar el pin; no desactivar la validación. **Playtech:** ambos manifests declaran `env.glu6=true` y se detienen en la guardia del bridge GLU6 no implementado. El soporte GPAS estático no demuestra soporte de esa variante activa.
4. **Continuaciones observadas pendientes:** Betsoft `BUY_CASCADE_FREE`; Hacksaw elección de `bonus`; Ruby `rp_103 / buy_feature:freespin → pick`; ThunderSpin `respins`; Yggdrasil `C` y variantes de elección; compras de Spinomenal con cantidad de giros distinta o elección pendiente. Las capturas nuevas permiten investigar esas ramas; no se fabricaron continuaciones.
5. **Acceso e inicialización:** Amusnet declara `ipBlocked` y también devuelve 429; IronDog declara país no permitido; Evoplay, Games Global, KA, Kendoo, Red Tiger y Three Oaks rechazan acceso. Varias respuestas son páginas de desafío, no respuestas del juego. Microgaming pide una cabecera sin identificar cuál. NetEnt devuelve un error del emisor/API. No hay evidencia para atribuir todos estos casos a geografía ni para afirmar que cambiar una cabecera concreta los resuelve.

Esta tarea comprobó y documentó el comportamiento; los fallos anteriores **no fueron corregidos** durante la campaña. Consultar estas capturas antes de repetir la investigación o solicitar nuevamente evidencia ya obtenida.

## Tres modos no ejecutables

- `kendoo/mariachi-lock / buy:1`.
- `microgaming/3rd-base / buy`.
- `microgaming/bronco-big-bounty / normal`: segundo juego seleccionado como control explícito de cobertura ausente.

Se registraron como `unsupported`, sin emitir apuestas para esos modos. No cuentan entre los 202 intentos ejecutados.

## Reproducibilidad y evidencia

- Implementación del binario: [f550c02](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/commit/f550c0228774ff307135ec25c2e2138e6a97a236), sin cambios de código durante esta prueba.
- Configuración y workflow publicados en [20cd8d0](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/commit/20cd8d0c8998632b9f7720c8a73fff6734b47b23).
- [Selección exacta y configuración](../capture-runner/operations/live-smoke-35x2-request.json): dos juegos por proveedor, una muestra por modo, timeout 20 s por operación, max_steps 256 y límite global 600 s.
- [Workflow](../.github/workflows/live-smoke-35x2.yml): un job, sin matriz de 35 jobs; conserva código de salida 2 y sube evidencia cifrada incluso cuando hay fallos. La conclusión failure de esta ejecución refleja esos resultados, no un fallo de compilación ni pérdida de capturas.
- Binario del artifact del run 43686603978, SHA-256 `c4c8a6e17e12581f558f862207c3b141fcd8feb3e215eb7aaad7e4bd21f54271`.
- [Resultados por proveedor, juego, modo e identificador del banco](live-smoke-2026-11-15.json). Este JSON público omite sesiones y cuerpos nativos.

El workflow se dispara al cambiar el request indicado o por ejecución manual. No se vuelve a ejecutar al actualizar este informe. El binario está fijado al release probado: para ensayar un parche futuro, actualizar el run de origen y su hash antes de usarlo. Los artifacts remotos tienen retención limitada; el banco entregado preserva el resultado de esta ejecución.

### Banco entregado

`live-smoke-35x2-bank-20261115.zip`, **37551146 bytes**, contiene los 202 intentos: 109 completos y 93 parciales. Incluye el banco portable, informes, configuración y registros globales de campaña, más el material privado para recuperar el artifact cifrado original. Se conserva fuera de Git y se entrega al usuario.

SHA-256 del ZIP:

```text
afe2c7eca286b3b2873c121d4e01d35be61e63b527910a99656fc05f69315030
```

Se verificaron los 606 hashes de archivos de intentos y la integridad del ZIP. Un replay del normal completo de `rp_103` devolvió 0 y su compra parcial devolvió 2, conservando la diferencia. Para usarlo, extraer el ZIP y ejecutar `capture-runner replay --bank bank`; la [guía de replay](../capture-runner/docs/REPLAY.md) detalla los comandos. El banco no incluye el cliente gráfico original ni genera nuevas rondas.

### Primer intento desde el entorno de trabajo

La primera ejecución local terminó en 5,724 s con 202 errores de transporte y ninguna respuesta nativa recibida. La resolución DNS del sistema devolvía `EAI_AGAIN`; 11 fallos WS lo indicaban explícitamente y los errores HTTP eran genéricos. Esa pasada no permite juzgar los proveedores. Los resultados de este informe pertenecen exclusivamente a la repetición en GitHub Actions, donde sí hubo respuestas y cero errores de transporte.
