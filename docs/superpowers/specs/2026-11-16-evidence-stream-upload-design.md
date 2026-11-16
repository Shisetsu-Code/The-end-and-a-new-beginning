# Transferencia de evidencia sin espacio temporal

## Solicitud y objetivo

El usuario necesita subir `E:\Backups\Final-copia-2026-11-05`, de más de 2 GB, sin comprimir ni duplicar la carpeta porque no tiene espacio en el disco. El destino solicitado es GitHub, para recuperar los originales de proveedores descritos en `docs/EVIDENCE-ATTACHMENTS-2026-11-16.md`.

La herramienta es auxiliar de transferencia; no modifica el runner Rust, los protocolos ni la evidencia original. La ruta Windows no está disponible en el entorno de desarrollo: entregar instrucciones ejecutables y distinguir pruebas con fixtures de una transferencia real.

## Enfoque elegido para implementar

Decisión de implementación: generar un TAR PAX **sin compresión**, directamente desde los archivos, dividir su flujo en bloques en RAM y subirlos como assets de una release de un repositorio privado. El usuario pidió evitar compresión y espacio adicional; no eligió un formato concreto. El TAR conserva los nombres relativos y estructura sin crear un archivo temporal.

Alternativas consideradas: Git normal crea objetos locales adicionales y limita el tamaño de cada archivo; subir un asset por archivo no conserva jerarquía directamente y alcanza rápidamente el límite de assets. Un ZIP o TAR almacenado primero tampoco satisface la restricción de disco.

## Contrato

- Python 3.10+ y biblioteca estándar; un solo script descargable. GitHub CLI aporta la sesión existente, o se usa `GH_TOKEN`/`GITHUB_TOKEN`, sin imprimir credenciales.
- Repositorio privado existente con un commit inicial; el comando de preparación crea sólo el remoto con README, sin clonar. Comprobar privacidad antes de crear la release o enviar evidencia.
- `--source`, `--repo`, `--tag`; bloques predeterminados de 128 MiB, configurables entre 1 y 1024 MiB. `--check` recorre el origen sin autenticación ni red.
- No escribir archivos temporales, archivos de progreso ni una copia Git. Sólo un bloque de datos, buffers pequeños y los nombres del directorio que se ordena en ese momento; no inventario global ni retención de todos los TarInfo.
- Conservar bytes de archivos, nombres Unicode, directorios vacíos y enlaces. No seguir symlinks ni junctions; declarar que los destinos externos no se incluyen. Fallar ante entradas no representables. No es una imagen NTFS ni conserva ACL/ADS.
- Orden y metadatos TAR deterministas. Fijar la huella del origen y tamaño del bloque en el borrador para reanudar. Revisar archivos antes/después de leer y comparar otra pasada de metadatos antes de marcar completo; no ofrece un snapshot atómico de archivos activos.
- Bloques con secuencia y SHA-256; para reutilizarlos exigir nombre, tamaño, estado y digest remoto. Ante divergencia, detenerse sin sobreescribir evidencia.
- Paginar releases y assets; reintentar fallos transitorios reconciliando respuestas ambiguas. Sólo se puede eliminar un asset propio del mismo nombre, vacío y con estado `starter`.
- Máximo 999 bloques y un manifiesto final por release. Cada bloque queda por debajo del límite GitHub de 2 GiB. Detectar exceso antes de subir cuando sea posible y nunca truncar.
- El manifiesto final contiene formato, conteos, tamaños, huellas y partes ordenadas; sólo se publica la release después de verificar todo. Repetir el mismo comando permite retomar o verificar la entrega ya completa.

## Verificación y documentación

Pruebas con fixtures temporales y un doble del servicio remoto: reconstrucción exacta, interrupción/reanudación, cambios del origen, privacidad, hashes erróneos, paginación y recuperación de POST ambiguos. CI en Windows y Linux, sin subir datos reales ni consumir secretos. Guía breve con la ruta exacta y actualización del HANDOFF para que otro agente no confunda un uploader entregado con evidencia ya recibida.

Fuentes: [límites de Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases), [API de assets](https://docs.github.com/en/rest/releases/assets), [creación remota sin clone](https://cli.github.com/manual/gh_repo_create). Consultadas el 16/11/2026.
