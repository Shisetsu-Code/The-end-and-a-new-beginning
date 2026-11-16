# Subir la evidencia sin comprimir ni duplicar la carpeta

## Qué hace

[`tools/upload_evidence_stream.py`](../tools/upload_evidence_stream.py) lee `E:\Backups\Final-copia-2026-11-05` y genera un TAR **sin compresión**, directamente en memoria. Envía bloques de 128 MiB a GitHub Releases y conserva nombres relativos, directorios y contenido de archivos. No guarda un TAR/ZIP en el disco, no clona el repositorio y no crea objetos Git de la carpeta.

El repositorio de destino debe ser **privado**, porque los originales incluyen sesiones y datos de capturas. El script comprueba esa condición. Este auxiliar no forma parte del runtime Rust y no cambia proveedores ni resultados de campañas.

**Estado:** herramienta preparada para ejecutarse en la PC del usuario. La carpeta `E:\...` no está montada en el entorno de desarrollo; publicar el script no significa que los originales hayan sido subidos o recibidos.

## Ejecutar desde PowerShell

Requiere Python **3.10 o posterior** y [GitHub CLI](https://cli.github.com/). `py -3 --version` y `gh --version` permiten comprobarlos. Guardar el script requiere sólo unas decenas de KB; los datos se procesan en RAM. Mantener quieto el respaldo durante la transferencia: cerrar cualquier recolector o proceso que lo esté modificando.

### 1. Autenticarse y crear el destino una sola vez

```powershell
gh auth login --hostname github.com --git-protocol https --web
gh repo create Shisetsu-Code/Evidencia-Backup-20261105 --private --add-readme
```

Si ya existe ese repositorio privado, omitir su creación. `--add-readme` crea un commit remoto para la release; no se usa `--clone`, `git init` ni `git add`. La sesión debe tener permiso de escritura en ese repositorio.

### 2. Descargar el script pequeño y subir

```powershell
$uploaderPath = Join-Path $env:TEMP 'upload_evidence_stream.py'
Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/Shisetsu-Code/The-end-and-a-new-beginning/main/tools/upload_evidence_stream.py' -OutFile $uploaderPath
py -3 -B $uploaderPath --source 'E:\Backups\Final-copia-2026-11-05' --repo 'Shisetsu-Code/Evidencia-Backup-20261105' --tag 'evidencia-final-copia-2026-11-05'
```

El script toma la credencial de la sesión `gh` en memoria. También admite `GH_TOKEN` o `GITHUB_TOKEN`; no necesita copiar un token al chat ni introducirlo como argumento.

### 3. Si se interrumpe

Repetir **el mismo comando Python**, con la misma carpeta, tag y tamaño de bloque. Vuelve a leer el origen para regenerar/verificar los bloques, pero sólo envía los que faltan. Los bloques completos se reutilizan después de comprobar su tamaño y SHA-256 contra lo que informa GitHub.

Si la carpeta cambió, no combinarla con la entrega anterior: el script se detiene. Para transferir intencionadamente otro estado, usar un tag nuevo. No borrar bloques manualmente para forzar una coincidencia.

Al terminar, el script muestra el enlace de la release. Compartir ese enlace; la conexión GitHub utilizada para revisar la evidencia también deberá tener acceso al repositorio privado. El enlace por sí solo no cambia sus permisos.

## Tamaño, memoria y comprobación previa

- Bloque predeterminado: **128 MiB de datos**. El proceso también usa memoria para Python, buffers pequeños y nombres de archivos del directorio que está ordenando. La carpeta completa no se carga en memoria.
- Para reducir el bloque, agregar `--part-mib 32` desde el comienzo. Para una carpeta mayor, se puede aumentar hasta `--part-mib 1024`, a costa de más RAM. Conservar ese valor al reanudar.
- La versión admite **999 bloques más un manifiesto** por release: aproximadamente **124,9 GiB de TAR** con bloques de 128 MiB. El TAR incluye cabeceras y relleno, por lo que su tamaño supera ligeramente la suma de archivos.
- GitHub permite hasta 1000 assets por release, cada uno menor que 2 GiB. El límite anterior corresponde a esta herramienta y al tamaño de bloque, no a una cuota total oficial de 124,9 GiB.
- No hay exclusiones automáticas: se incluyen archivos normales, incluso ocultos, salvo los destinos de enlaces que están fuera del recorrido.

Para contar y calcular el tamaño sin autenticar ni subir:

```powershell
py -3 -B $uploaderPath --source 'E:\Backups\Final-copia-2026-11-05' --repo 'Shisetsu-Code/Evidencia-Backup-20261105' --check
```

La primera pasada puede tardar si hay cientos de miles de archivos. La transferencia lee cada archivo por flujo; la comprobación final vuelve a recorrer sus metadatos.

## Integridad y límites

El TAR conserva los bytes de los archivos, las rutas relativas y los directorios vacíos. Los enlaces simbólicos y junctions se representan como enlaces: **no se recorre su destino** ni se incluye automáticamente lo que apuntan fuera de la carpeta. El resumen informa cuántos enlaces hay. Una entrada no soportada o ilegible detiene la entrega; no se omite silenciosamente.

Se trata de una transferencia de archivos para análisis, no de una imagen NTFS: no conserva ACL, streams alternativos ni todos los atributos de Windows. Los hardlinks se transfieren como archivos con su contenido. La representación no ofrece un snapshot atómico de una carpeta que se esté modificando.

Durante la lectura se revisan los metadatos del archivo y se compara la huella del recorrido al terminar. Un cambio detectado impide generar la marca de entrega completa. Por eso el origen debe permanecer estable, incluso si el programa que escribe una base de datos conserva artificialmente su tamaño o fecha.

Los bloques usan nombres con secuencia y SHA-256. El script no sobreescribe un bloque con contenido diferente. Una respuesta de subida perdida se reconcilia con el listado remoto antes de reintentar; sólo se permite retirar el asset propio exacto que GitHub haya dejado vacío y con estado `starter`. Un digest remoto ausente o incorrecto no cuenta como validación.

La release permanece como borrador mientras falta evidencia. El manifiesto final contiene las partes en orden, sus tamaños y hashes, la identidad del origen y el hash del TAR completo. La publicación ocurre después de esa verificación; si se interrumpe en ese último paso, repetir el comando permite completarlo.

## Cómo retomar el análisis

1. No considerar que se recibió la carpeta hasta observar la release completa y verificar el manifiesto y sus assets.
2. Descargar todas las partes indicadas y comprobar cada SHA-256. Concatenarlas en el orden del manifiesto produce el TAR sin compresión; las partes no son TAR independientes.
3. Comprobar tamaño/hash del TAR completo. Se puede leer el flujo concatenado con `tarfile` sin escribir otra copia del TAR; extraer sólo los originales requeridos por el [manifiesto de recuperación](evidence-recovery-2026-11-16.json).
4. Tratar los archivos como evidencia: no ejecutar scripts contenidos en el respaldo ni seguir enlaces archivados. Conservar originales y hashes.
5. Contrastar los mensajes pendientes de proveedores antes de ampliar contratos. La transferencia por sí misma no cambia el estado **473 completos / 109 parciales**.

## Verificación de la herramienta

```bash
python -B -m unittest discover -s tools/tests -v
python -B tools/upload_evidence_stream.py --help
```

Verificado en `783efe380db46e6ddadf6028f84c29cee6534af6`: [CI 43857546086 correcto](https://github.com/Shisetsu-Code/The-end-and-a-new-beginning/actions/runs/43857546086). El workflow [`evidence-upload-tests.yml`](../.github/workflows/evidence-upload-tests.yml) pasó en Windows y Linux con Python 3.10 y 3.13: Windows ejecutó los 22 casos, incluidas junctions; Linux pasó 21 y omitió sólo esa prueba de plataforma. No accede a la carpeta del usuario, no sube evidencia real y no necesita credenciales de carga.

Script verificado: 21649 bytes; SHA-256 `edfd136574af41fda7f2a3d3fe455d30748b32651d0850d9a48c3308bd0edb18`. La comparación de metadatos contempla las diferencias de CPython Windows entre consultas por ruta y por descriptor sin quitar la detección de cambios durante lectura.

## Fuentes oficiales

- [Límites de GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).
- [API de assets: digest, paginación, subida binaria y estado starter](https://docs.github.com/en/rest/releases/assets).
- [GitHub CLI: crear remoto con README y sin clone](https://cli.github.com/manual/gh_repo_create).
- [GitHub CLI: autenticación con navegador](https://cli.github.com/manual/gh_auth_login).

Consultadas el 16 de noviembre de 2026.
