# Cobertura de protocolos

Resultado de `capture-runner plan --config capture-runner/examples/direct-explicit.json --details`. «Ejecutable» significa que hay selector y flujo implementados con controles de fuente/configuración; no certifica disponibilidad remota ni éxito de una captura nueva. El catálogo y sus hashes permanecen iguales. Cada modo pendiente se enumera en [coverage.json](coverage.json).

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
| hardfire-kalamba | 66 | 66 | 66 | 0 |
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
| playtech | 48 | 100 | 3.940 | 3.840 |
| pragmatic | 656 | 1.665 | 1.687 | 22 |
| redtiger | 361 | 609 | 610 | 1 |
| reelkingdom | 73 | 183 | 183 | 0 |
| rubyplay | 179 | 255 | 262 | 7 |
| smartsoft | 41 | 66 | 66 | 0 |
| spinomenal | 536 | 1.343 | 1.581 | 238 |
| threeoaks | 103 | 234 | 234 | 0 |
| thunderspin | 60 | 84 | 84 | 0 |
| yggdrasil | 138 | 301 | 301 | 0 |
| **Total** | **6.957** | **14.787** | **20.090** | **5.303** |

El catálogo tiene **35 proveedores, 9.878 juegos y 20.090 modos**. 21 proveedores cubren todos los modos identificados; 14 conservan pendientes. Los juegos sin modos no reciben apuestas inventadas.

## Ampliación con capturas completas y motor GPAS

| Proveedor | Antes | Ahora | Modos adicionales | Juegos ejecutables ahora |
|---|---:|---:|---:|---:|
| RubyPlay |11|255|244|179|
| Playtech |1|100|99|48|
| Kalamba |1|66|65|66|
| **Total de modos nuevos** | | | **408** | |

RubyPlay incorpora 179 normales, 51 compras freespin, 17 respin, 4 minispin y 4 select. Se verificaron los 179 archivos, 432 rondas cerradas y 1491 pares nativos. Quedan dos compras rechazadas sin cierre aceptado y cinco modos `ruby_buy` sin contrato. El binario se conserva íntegro; salvo el decodificador original de African Cats, su semántica interna puede permanecer opaca.

Playtech incorpora 47 normales, 29 compras, 22 antebets y 2 variantes con UUID exacto. Los 100 modos provienen del cruce del inventario, reglas explícitas, 32 modos de recibos y dos casos de base documentados, excluyendo productores/familias no implementados. Las solicitudes se validan contra fuentes y configuración actuales, límites nativos y dominios de elección. Las 47 solicitudes de los flujos entregados se prueban offline; sus respuestas recortadas no constituyen un banco completo de replay.

Kalamba cubre los 66 normales identificados. Sus respuestas de primera apertura/giro están verificadas contra el constructor entregado. Una rama nueva de bonus o una compra sin comando documentado conserva el intento parcial.

## Entrega inicial anterior (histórico)

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

La cobertura proviene de la misma tabla utilizada para despachar conectores. Los indicadores históricos `ready`, `verified` o de conciliación no habilitan un protocolo. La oferta actual puede no admitir un modo o una fuente puede cambiar; el intento se conserva con el rechazo o evento desconocido correspondiente.

Los 648 juegos sin modos del inventario original no originan comandos inventados. Games Global aporta dos selectores suplementarios y su «2/2» se refiere sólo a éstos, no a todo su catálogo. IronDog conserva su suplemento de cuatro juegos y 22 pares.

Las elecciones condicionales sólo se consideran ejercidas cuando se observa la acción solicitada. Si una ronda cierra sin ofrecer esa rama, se conserva con `target_not_observed`; no se fabrica la elección.

[MISSING-EVIDENCE.md](MISSING-EVIDENCE.md) describe los contratos pendientes; [PROVENANCE.md](PROVENANCE.md) identifica las fuentes; [VERIFICATION.md](VERIFICATION.md) registra la validación offline. [REPLAY.md](REPLAY.md) explica cómo importar/exportar y reproducir los mensajes locales.
