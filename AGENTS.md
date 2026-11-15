# Guía para agentes

## Leer lo mínimo necesario

1. Leer [docs/HANDOFF.md](docs/HANDOFF.md): estado vigente, trabajo terminado, pendientes y mapa de archivos.
2. Para modificar el crate, leer también [capture-runner/AGENTS.md](capture-runner/AGENTS.md).
3. Abrir sólo la documentación y el código del área solicitada. Los planes de `docs/superpowers/` son registros históricos; no son tareas pendientes ni asignaciones actuales de agentes.

No empezar con otra auditoría completa, volver a extraer todos los ZIP ni cargar `coverage.json`, los bindings o los fixtures completos en contexto. Usar `rg` y seleccionar el proveedor/campo necesario. El checkout contiene lo necesario para compilar y ejecutar la suite normal; los originales externos sólo hacen falta para sus pruebas opt-in o para investigar evidencia adicional.

## Objetivo e invariantes

- Recolectar apuestas **demo**, sus solicitudes/respuestas y continuaciones para reproducirlas después localmente. Runtime Rust, VM Linux, sin GUI.
- El cupo de juegos es por **proveedor y salida**. Los modos de cada juego son secuenciales; una caída no bloquea otras salidas. Nunca restaurar el bloqueo global de un juego por proveedor.
- Guardar los mensajes antes de interpretarlos. Conservar binarios, sesiones, rechazos, parciales y eventos desconocidos. No inventar acciones ni finales.
- Cierre nativo y conciliación financiera son independientes. Los saldos/premios no son requisitos de captura, cierre ni exportación.
- Sesiones nuevas para capturas nuevas. El importador histórico conserva el estado original; no fabrica un init por ronda ni reutiliza esas sesiones contra el proveedor.
- Replay estricto de mensajes grabados, sin reescritura automática de identificadores, nuevos resultados aleatorios ni frontend original.
- WireGuard nunca cae silenciosamente a la red directa. Las claves, sesiones privadas y ZIP completos permanecen fuera de Git; las fuentes originales se conservan sin editar.

## Mantener el traspaso

La autoridad es el código/CLI actual, después la cobertura generada y la verificación ligada a un commit. Las cifras y planes históricos no amplían contratos ejecutables ni prueban disponibilidad remota.

Al terminar un cambio, actualizar sólo los documentos afectados y el estado/resumen de `docs/HANDOFF.md` si cambió. Registrar el commit realmente verificado, comandos, resultado y límites; no anunciar CI exitoso antes de comprobarlo. Para cambios sólo documentales, comprobar enlaces, cifras, ejemplos y diff; no repetir capturas ni toda la investigación de protocolos.
