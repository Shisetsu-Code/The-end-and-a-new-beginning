# Rondas completas: diez juegos por proveedor con cierres previos

## Resultado

Se ejecutaron **210 juegos distintos de 21 proveedores**, diez por proveedor, con una muestra de cada modalidad implementada: **582 intentos iniciados y terminados, 473 con cierre nativo verificado y 109 parciales**. No hubo modos implementados sin iniciar, intentos duplicados ni errores de transporte. Los **11 modos inventariados sin contrato** se informan aparte y no cuentan como intentos ni como fallos live.

En los modos normales cerraron **181/210**. Catorce proveedores completaron sus diez normales; ocho completaron todos los modos implementados de su muestra. Por juego, **163 cerraron todos sus modos implementados, 19 cerraron algunos y 28 no cerraron ninguno**. Los 21 proveedores tuvieron algún cierre; esto describe los juegos y modalidades elegidos, no el catálogo completo.

La [ejecución 43784896618](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43784896618) duró **341 segundos** de runner, del **16 de noviembre de 2026, 01:12:45 a 01:18:26 UTC**; corresponde al **15 de noviembre, 22:12:45 a 22:18:26, UTC−3**. El workflow terminó en `failure` al conservar el **exit 2** del runner: hubo parciales y cobertura pendiente. **No hubo timeout**, y el cifrado y la subida del artefacto terminaron correctamente. El nombre y `run_id` de la campaña conservan la fecha del 15 de noviembre.

Los [casos y metadatos públicos en JSON](complete-rounds-10-2026-11-15.json) identifican cada combinación y los ejemplos de banco. La configuración y sus controles están en [COMPLETE-ROUNDS-10.md](../capture-runner/operations/COMPLETE-ROUNDS-10.md).

Para retomar un proveedor sin cargar todo el JSON, desde la raíz del repositorio:

```bash
jq '.providers[] | select(.provider == "rubyplay")' docs/complete-rounds-10-2026-11-15.json
jq '.attempts[] | select(.provider == "rubyplay" and .bank_state == "partial")' docs/complete-rounds-10-2026-11-15.json
```

## Procedencia y alcance de la verificación

| Elemento | Identificación |
|---|---|
| Solicitud y workflow | [`84b4b96d227e73c2d4ac046a0a67e7d2452a09ba`](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/commit/84b4b96d227e73c2d4ac046a0a67e7d2452a09ba) |
| CI de la solicitud | [43784896641](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43784896641), `success` |
| Fuente del binario reutilizado | [`af8f957cce4cd57fe633da50b4620116ed19c6e3`](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/commit/af8f957cce4cd57fe633da50b4620116ed19c6e3) |
| CI que produjo ese binario | [43699080917](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43699080917), `success` |
| SHA-256 del binario | `faceddbd1e234c09eb2147affcd553b2e8dce00c9423de6d2e2de4010af179a6` |
| Run de captura | [43784896618](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43784896618) |
| `run_id` | `complete-rounds-10-20261115` |
| SHA-256 de `bank/index.json` | `fc058196d57d1daf16bff914a3f0637cc16a6a34e3eba93a305e6ee89d93e39e` |

El workflow comprobó que fuentes, bindings, manifiestos Cargo y `standalone info/` coincidieran con el commit fuente del binario. Esta campaña **no cambió adaptadores ni cobertura**. La disponibilidad remota se midió con salida directa; no se verificaron peers WireGuard ni otras geografías.

Un cierre verificado exige `index.state=complete`, incidencias del índice vacías, `result.status=complete`, `completion` no nulo, último evento `attempt_finish` completo y coincidencia entre metadatos, resultado e índice. También se verifican los hashes, tamaños y la secuencia íntegra del journal. Los saldos y premios no intervienen.

**Integridad: 1746/1746 archivos correctos**, correspondientes a 582 `attempt.json`, 582 `wire.jsonl` y 582 `result.json`. No hubo hashes distintos, archivos ausentes, colas truncadas ni inconsistencias. Los análisis detallados y los mensajes originales permanecen en el paquete privado; Git contiene sólo los metadatos saneados necesarios para este informe.

## Selección

La [selección fijada](../capture-runner/operations/complete-rounds-10-selection.json) parte de los proveedores con al menos un cierre en la [muestra anterior de 35 × 2](LIVE-SMOKE-2026-11-15.md): 13 habían completado toda su muestra y ocho sólo una parte. Los 14 sin cierres anteriores quedaron fuera de esta campaña.

Se eligieron **41 juegos con cierres previos**, **nueve por diversidad de modalidades** y **160 mediante estratificación determinista por cantidad de modos**. Los desempates usan un SHA-256 estable. Se conservaron todos los modos implementados de cada juego, incluidos los que ya habían fallado. RubyPlay incluyó freespin, respin, minispin y select. Todos los proveedores elegibles tenían al menos diez juegos ejecutables.

Es una muestra fija dirigida a ampliar la evidencia y probar continuaciones. **No es una muestra aleatoria representativa del catálogo**; los porcentajes resultantes no estiman la disponibilidad general del proveedor. Una muestra por modo tampoco recorre todas sus ramas posibles.

La ejecución usó sesiones nuevas por intento, una muestra por modo, `max_steps: 512` y `timeout_seconds: 20`. El presupuesto de pasos se interpreta por adaptador y el timeout se aplica por solicitud/intercambio. Los modos de cada juego se ejecutan secuencialmente y cada proveedor dispone de dos cupos independientes.

## Resultado por proveedor

“Modos cerrados” incluye los normales y las demás modalidades implementadas. “Parciales” reúne intentos terminados sin cierre nativo. Las diez variantes Betsoft y la compra Spinomenal sin contrato no forman parte del denominador de intentos.

| Proveedor | Normales cerrados | Modos cerrados / intentados | Parciales |
|---|---:|---:|---:|
| 1spin4win | 10/10 | 13/13 | 0 |
| amigogaming | 10/10 | 15/15 | 0 |
| betsoft | 10/10 | 15/16 | 1 |
| bgaming | 10/10 | 31/31 | 0 |
| egt-digital | 10/10 | 14/14 | 0 |
| elk | 1/10 | 1/63 | 62 |
| endorphina | 9/10 | 18/19 | 1 |
| habanero | 10/10 | 16/16 | 0 |
| hacksaw | 10/10 | 56/57 | 1 |
| hardfire-kalamba | 4/10 | 4/10 | 6 |
| mascot | 10/10 | 16/16 | 0 |
| nolimit | 10/10 | 127/127 | 0 |
| peterandsons | 10/10 | 31/32 | 1 |
| playngo | 6/10 | 8/12 | 4 |
| pragmatic | 10/10 | 18/22 | 4 |
| reelkingdom | 10/10 | 24/26 | 2 |
| rubyplay | 9/10 | 11/15 | 4 |
| smartsoft | 9/10 | 13/16 | 3 |
| spinomenal | 10/10 | 24/28 | 4 |
| thunderspin | 10/10 | 14/14 | 0 |
| yggdrasil | 3/10 | 4/20 | 16 |
| **Total** | **181/210** | **473/582** | **109** |

## Resultado por modalidad

Se usa el tipo declarado en el inventario, no una inferencia a partir del nombre de la modalidad.

| Tipo | Intentos | Cierres | `provider_error` | `unknown_event` |
|---|---:|---:|---:|---:|
| Normal | 210 | 181 | 26 | 3 |
| Compra | 224 | 198 | 11 | 15 |
| Antebet | 25 | 24 | 0 | 1 |
| Booster | 55 | 55 | 0 | 0 |
| Variante | 68 | 15 | 53 | 0 |
| **Total** | **582** | **473** | **90** | **19** |

No hubo `transport_error` ni intentos interrumpidos. `provider_error` es el estado del runner: también puede originarse en un control local del adaptador y no atribuye por sí mismo una avería al servidor.

## Qué representan las acciones y el solapamiento

De los 582 intentos, **514 registraron alguna intención de acción de ronda marcada `round_action=true` y 68 no registraron ninguna**. Por eso el total de intentos no equivale a 582 tiradas aceptadas por los juegos.

El banco contiene **28137 eventos** y **4945 intenciones salientes**: 3076 solicitudes HTTP, 203 handshakes WebSocket y 1666 frames WebSocket enviados. También conserva 1714 frames WebSocket recibidos. Las **2578 acciones de ronda marcadas** se reparten entre 1158 HTTP y 1420 frames WebSocket; las inicializaciones y los mensajes auxiliares se cuentan por separado.

**260 intentos tuvieron más de una acción de ronda**, de los cuales 253 cerraron y siete quedaron parciales. Una secuencia puede contener elecciones, cobros o continuaciones: ese número no es un conteo de bonos ni de freespins. Ningún cierre verificado carece de acciones de ronda registradas. Los campos nativos `completion.details.steps/actions` se conservan con su tipo original y no se suman entre adaptadores.

El máximo observado fue de **42 intervalos de juegos y 21 proveedores**, con máximo de **dos juegos por proveedor**. Los 42 juegos se solaparon durante 4960 ms y los 21 proveedores durante 7096 ms. El cálculo usa intervalos semiabiertos `[started_ms, finished_ms)` que abarcan los modos de cada juego; para cada proveedor se toma la unión de los intervalos de sus juegos. **No mide simultaneidad exacta de solicitudes HTTP/WS**. La ventana cubierta por estos metadatos es de 340558 ms; los 341 segundos anteriores corresponden a los límites temporales registrados para el runner.

## Parciales: causas observadas

Las 109 parciales se agrupan en **55 errores HTTP de apertura**, **15 rechazos durante una operación**, **15 detenciones por contratos o controles locales** y **24 continuaciones sin implementación compatible**. Son categorías de diagnóstico distintas de `provider_error`/`unknown_event`. Un HTTP 400/500 o un código nativo identifica la respuesta observada; no establece su causa raíz ni la responsabilidad del proveedor frente a la solicitud enviada.

| Proveedor | Casos | Motivo observado |
|---|---:|---|
| betsoft | 1 | Estado BUY_CASCADE_FREE no reconocido después de tres continuaciones. |
| elk | 54 | HTTP 500 durante init; no se envió acción de ronda. |
| elk | 1 | HTTP 500 durante checkwin. |
| elk | 7 | HTTP 500 durante purchase. |
| endorphina | 1 | Nombre del juego lanzado distinto del inventario; detenido antes de apostar. |
| hacksaw | 1 | Estado started con opciones fs y fs_2_coin; el adaptador no tiene esa elección. |
| hardfire-kalamba | 6 | El WebSocket emitido por el launcher tiene un nombre que no cumple el patrón numérico permitido por la allowlist local; se detiene antes del handshake. |
| peterandsons | 1 | HTTP 400 al enviar complete después de play. |
| playngo | 4 | No se encontró el par de parámetros de apuesta esperado en el cliente; sin acción de ronda. |
| pragmatic | 1 | HTTP 200 con `frozen` y `ext_code=SystemError` después de `doMysteryScatter`: error interno con reinicio anunciado. La etiqueta local de sesión es demasiado genérica. |
| pragmatic | 3 | La respuesta pide una selección de bonus para la que no hay opción verificada. |
| reelkingdom | 2 | La respuesta pide una selección de bonus para la que no hay opción verificada. |
| rubyplay | 1 | La respuesta posterior a select cambió respecto del contrato de elección. |
| rubyplay | 1 | Faltan parámetros revisados para la continuación pick. |
| rubyplay | 1 | Faltan parámetros revisados para la continuación respin. |
| rubyplay | 1 | Faltan parámetros revisados para la continuación select. |
| smartsoft | 2 | Documento HTTP 200 sin confirmación del portal DEMO que exige el adaptador; sin acción de ronda. |
| smartsoft | 1 | HTTP 500 al obtener el documento demo. |
| spinomenal | 2 | Compra respondió HTTP 200 pero el número de giros difiere del contrato esperado. |
| spinomenal | 2 | La compra requiere una elección aún no vinculada. |
| yggdrasil | 5 | HTTP 200 con rechazo nativo de comando, código 1. |
| yggdrasil | 11 | La ronda queda Pending y ofrece comando C; el adaptador sólo admite freeSpin. |

### Aclaración causal del 16 de noviembre

Relectura de las mismas capturas y del código, sin nuevas sesiones ni cambios de resultados:

- **Kalamba, seis casos:** el documento `awscdn.test.kalamba.net/index.html` sí está permitido. El launcher emite el WebSocket `gameserver-api-for-kl-na-2.test.twmc8fvg.click`, mientras que [la validación local](../capture-runner/src/context.rs) sólo admite dígitos entre `gameserver-api-` y `.test.twmc8fvg.click`. La parada ocurre al validar ese destino, antes del handshake. No fue un rechazo de una apuesta por el servidor.
- **Pragmatic, `vs10bookfallen/pur-1`:** la respuesta real contiene `frozen=Internal server error. The game will be restarted.` y `ext_code=SystemError`; `unlogged` está ausente. [El parser](../capture-runner/src/adapters/pragmatic.rs) usa la misma etiqueta `demo session unavailable` para `unlogged` y para `frozen` sin `na`. En este caso esa etiqueta no demuestra falta de sesión. Se corrige la interpretación del informe; el resultado sigue siendo parcial.
- **Errores remotos sin causa específica:** las tres respuestas ELK inspeccionadas —init, purchase y checkwin— son páginas HTTP 500 genéricas idénticas. Los cinco rechazos Yggdrasil con código 1 dicen `Technical problem, please retry.`. Peter & Sons devuelve `TRANSACTION_REJECTED` y `Application Error` al cerrar. Estos mensajes no permiten atribuir la causa a sesión, IP, país, carga o parámetros concretos.

Los **39 casos detenidos por continuaciones o controles locales** y los **70 con respuesta remota de error** requieren diagnósticos distintos. Una modalidad implementada puede entrar correctamente y encontrar después una rama aún no cubierta. El banco privado original no se modifica; conserva las etiquetas originales del runner y los mensajes que permiten esta aclaración.

Las continuaciones pendientes permanecen parciales. No se inventaron selecciones, parámetros, mensajes de cierre ni resultados para completar las secuencias. Los errores saneados y las combinaciones exactas se conservan en el [JSON del informe](complete-rounds-10-2026-11-15.json); los cuerpos y detalles privados quedan fuera de Git.

## Cinco ejemplos de cierre nativo con continuaciones

Estos ejemplos están respaldados por mensajes nativos y el cierre íntegro del intento. Las cantidades siguientes son acciones de protocolo, no necesariamente giros base. El [JSON del informe](complete-rounds-10-2026-11-15.json) asocia proveedor, juego y modalidad con su `bank_attempt_id` para localizar la grabación privada.

| Proveedor / juego | Modalidad | Secuencia registrada | Marcador nativo final |
|---|---|---|---|
| AmigoGaming / `174` | `BUY_PICKME` | 22 acciones WS: 21 `SpinAction` y una `PickMeAction`; se observan estados `PICKME` y `FREE_GAME` | `slot.status=FINISHED`, cero pasos disponibles |
| Nolimit / `Beheaded` | `FREESPIN` | 49 acciones; respuestas con freespins, avalanchas y una elección registrada | Siguiente modo `NORMAL` |
| Pragmatic / `vswaysyinyang` | `pur-0` | 18 `doSpin`, un `doBonus` y un `doCollect`; 16 respuestas con campos nativos de freespin | `na=s`, sin campos de freespin pendientes |
| RubyPlay / `rp_106` | `buy_feature:freespin` | Una compra, 16 `freespin` y un `respin`; `an` consecutivo de 1 a 18 | `next_action=spin` |
| RubyPlay / `rp_142` | `buy_feature:minispin` | Una compra y 12 `minispin`; `an` consecutivo de 1 a 13 | `next_action=spin` |

Los binarios opacos RubyPlay se preservan. El éxito de esas secuencias concretas no permite cerrar automáticamente una elección distinta: otros intentos RubyPlay de esta misma campaña quedaron parciales ante `pick`, `respin`, `select` o un contrato de elección cambiado.

## Banco privado y replay offline

El banco verificado se entregó aparte como **`complete-rounds-10-bank-20261115.zip`**, de **142555475 bytes**. SHA-256:

```text
7ea575226fb25e63e4b6b63c403fe8f32352104381a8597ae88720d6e8f3909e
```

Se verificaron el CRC de sus **1783 miembros ZIP** y las **1782 entradas del manifiesto `SHA256SUMS`**. Estos controles abarcan el paquete completo y se distinguen de los 1746 archivos de intentos verificados dentro del banco.

El paquete incluye el banco con completos y parciales, el binario fijado, el original cifrado, el material privado de recuperación, `SHA256SUMS` y `LEEME`. El índice está identificado por el SHA-256 de la sección de procedencia. Las claves, tokens, cabeceras y cuerpos privados permanecen fuera de Git.

Se reprodujeron **los 582 intentos offline** mediante las solicitudes grabadas y el binario fijado: **473 replays terminaron con exit 0 y 109 con exit 2**, con **4945 solicitudes/pasos reproducidos y cero incidencias**. Exit 2 conserva el carácter parcial del intento original; no representa una pérdida de mensajes ni un nuevo rechazo de red. Esta comprobación no hizo llamadas a los proveedores.

Para listar los intentos del banco extraído, usando el binario verificado:

```bash
capture-runner replay --bank /ruta/al/directorio_con_index_json
```

La [guía de replay](../capture-runner/docs/REPLAY.md) describe la inspección y reproducción estricta. No se reutilizan las sesiones grabadas contra el proveedor. El banco permite reproducir mensajes existentes; no incorpora el frontend original, reescritura de tokens/contadores ni nuevas rondas o RNG.

## Pendientes y límites

Los 22 grupos anteriores localizan el trabajo pendiente de acceso, identidad, parámetros y continuaciones. Esta campaña no implementó esas correcciones. En particular, los 62 parciales ELK muestran una diferencia respecto de su muestra anterior, sin demostrar por sí solos que todo el proveedor haya dejado de funcionar.

Los seis proveedores del [retest histórico de sesiones](SESSION-BOOTSTRAP-2026-11-15.md), con 12 rechazos HTTP 403 y cero cierres, no fueron revalidados por esta ampliación. Ese resultado permanece vigente para aquella muestra. La disponibilidad de otras salidas, geografías y juegos del catálogo sigue sin verificarse.

Modificar estos informes no activa otra campaña. Para repetir la captura hay que cambiar la solicitud o iniciar explícitamente el workflow, manteniendo los controles de CI, binario y evidencia descritos en la [guía operativa](../capture-runner/operations/COMPLETE-ROUNDS-10.md).
