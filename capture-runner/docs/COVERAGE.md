# Cobertura de protocolos

Resultado del binario release: `capture-runner plan --config capture-runner/examples/direct-explicit.json --details`. «Ejecutable» significa que existe un selector y flujo implementado; no certifica disponibilidad remota actual ni éxito de una captura. Cada modo pendiente se enumera en [coverage.json](coverage.json).

| Proveedor | Juegos con modo ejecutable | Modos ejecutables | Modos identificados | Pendientes |
|---|---:|---:|---:|---:|
| 1spin4win | 232 | 282 | 283 | 1 |
| amigogaming | 156 | 207 | 207 | 0 |
| amusnet | 468 | 763 | 763 | 0 |
| belatra | 117 | 302 | 302 | 0 |
| betsoft | 211 | 380 | 574 | 194 |
| bgaming | 215 | 787 | 808 | 21 |
| egt-digital | 231 | 331 | 331 | 0 |
| ela-games | 68 | 134 | 134 | 0 |
| elk | 159 | 803 | 803 | 0 |
| endorphina | 238 | 563 | 563 | 0 |
| evoplay | 238 | 591 | 1.080 | 489 |
| gamesglobal | 2 | 2 | 2 | 0 |
| habanero | 205 | 313 | 315 | 2 |
| hacksaw | 142 | 744 | 744 | 0 |
| hardfire-kalamba | 1 | 1 | 66 | 65 |
| irondog | 19 | 64 | 401 | 337 |
| kagaming | 837 | 885 | 885 | 0 |
| kendoo | 33 | 40 | 76 | 36 |
| mascot | 126 | 211 | 211 | 0 |
| microgaming | 1 | 1 | 49 | 48 |
| netent | 197 | 373 | 440 | 67 |
| nolimit | 141 | 1.147 | 1.147 | 0 |
| peterandsons | 70 | 223 | 223 | 0 |
| playngo | 476 | 493 | 493 | 0 |
| playson | 114 | 242 | 242 | 0 |
| playtech | 1 | 1 | 3.940 | 3.939 |
| pragmatic | 656 | 1.665 | 1.687 | 22 |
| redtiger | 361 | 609 | 610 | 1 |
| reelkingdom | 73 | 183 | 183 | 0 |
| rubyplay | 9 | 11 | 262 | 251 |
| smartsoft | 41 | 66 | 66 | 0 |
| spinomenal | 536 | 1.343 | 1.581 | 238 |
| threeoaks | 103 | 234 | 234 | 0 |
| thunderspin | 60 | 84 | 84 | 0 |
| yggdrasil | 138 | 301 | 301 | 0 |
| **Total** | **6.675** | **14.379** | **20.090** | **5.711** |

El catálogo resultante tiene **35 proveedores, 9.878 juegos y 20.090 modos**. Veinte proveedores cubren todos sus modos identificados; quince conservan alguna combinación pendiente. El inventario común original permanece intacto: Games Global agrega dos selectores e IronDog agrega cuatro juegos y 22 pares nativos mediante suplementos con hash propio.

## Ampliación con ENTREGA-CLAUDE

La revisión incorpora **538 modos ejecutables adicionales**: 516 ya identificados en el catálogo anterior y 22 de IronDog que la nueva evidencia identifica por primera vez. No se usan las conciliaciones históricas como requisito de soporte.

| Proveedor | Ejecutables antes | Ejecutables ahora | Incremento |
|---|---:|---:|---:|
| 1spin4win | 265 | 282 | +17 |
| amusnet | 759 | 763 | +4 |
| betsoft | 3 | 380 | +377 |
| irondog | 3 | 64 | +61 |
| kendoo | 2 | 40 | +38 |
| mascot | 180 | 211 | +31 |
| rubyplay | 1 | 11 | +10 |

Mascot incorpora 22 compras y nueve normales de mesas/otros juegos; sus 31 faltantes eran modos, no 31 juegos. Amusnet incorpora las cuatro modalidades de sus dos juegos de dados pendientes. IronDog enumera los 61 pares de IntegratedSlots además de los tres Morphic anteriores; los 22 nuevos pares conservan sus IDs exactos, sin aliases.

## Límites de la medida

Los 648 juegos del inventario común que no tienen modos no originan apuestas inventadas. Dos de ellos reciben los selectores suplementarios publicados de Games Global. Su cobertura «2/2» sólo se refiere a esos modos identificados; no demuestra cobertura de todo su catálogo.

RubyPlay permite nueve juegos distintos y 11 modos por salida. En ocho de esos juegos se captura el binario íntegro y se interpretan los controles del sobre HTTP; un evento nuevo exclusivamente dentro del binario puede no clasificarse si esos controles no cambian. African Cats conserva su decodificador específico. Playtech sigue con un solo binding completo: las respuestas sintéticas del nuevo mock no proporcionan los motores nativos ausentes.

[MISSING-EVIDENCE.md](MISSING-EVIDENCE.md) identifica las rutas y contratos concretos pendientes. Los cupos se calculan independientemente de esta cobertura; `plan` limita la concurrencia máxima al número de juegos ejecutables elegidos.

Las elecciones condicionales sólo se marcan completas si se ejerció la acción solicitada. Si la ronda cierra sin ofrecer esa rama conocida, se conserva la captura con el motivo `target_not_observed`; no se inventa una elección.

Los bindings y protocolos tienen procedencia verificable en [PROVENANCE.md](PROVENANCE.md). Las [pruebas de esta entrega](VERIFICATION.md) son locales: no se establecieron túneles reales ni se contactaron proveedores.
