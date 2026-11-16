# Evidence Stream Upload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Entregar una subida reanudable de la carpeta Windows a GitHub Releases sin compresión ni archivos temporales grandes.

**Architecture:** Un script Python convierte archivos en TAR PAX determinista y entrega bloques de RAM a la API de GitHub. Un borrador fija identidad de transferencia; el manifiesto y la publicación indican finalización verificada.

**Tech Stack:** Python 3.10+, stdlib, GitHub REST, sesión de GitHub CLI, unittest.

**Spec:** `docs/superpowers/specs/2026-11-16-evidence-stream-upload-design.md`.

## Global Constraints

- No modificar originales ni escribir TAR/ZIP/copias Git en disco.
- Sólo repositorios privados; credenciales fuera de logs y manifiestos.
- 128 MiB por bloque predeterminado; 1–1024 MiB configurables; hasta 999 bloques más manifiesto.
- Symlinks y junctions se conservan como enlaces, sin recorrer sus destinos.
- No cambiar runtime, cobertura ni resultados de las campañas.
- Las pruebas no equivalen a haber subido la carpeta del usuario.

## Review Focus

- Cientos de miles de archivos: no acumular TarInfo ni un inventario global.
- Windows: Unicode, rutas largas y junctions no deben excluir archivos silenciosamente ni causar recursión.
- Respuesta de POST perdida: reutilizar el asset verificado, no sobrescribirlo.
- Origen cambiado: no marcar completo ni mezclar bloques de dos estados.
- Más de una página de assets y release ya completa: reanudar sin omitir controles ni mutar datos publicados.

## Task 1: Script y pruebas de comportamiento

**Files:** crear `tools/upload_evidence_stream.py`, `tools/tests/test_upload_evidence_stream.py` y `tools/tests/test_upload_evidence_http.py`.

**Interfaces:** `scan_source(source)`, `stream_tar(source, sink, expected_scan)`, `upload(source, repo, tag, part_bytes, client, progress)` y CLI `main()`. El cliente encapsula el límite remoto para probarlo sin red real.

- [x] Escribir pruebas de reconstrucción TAR con bytes conocidos, Unicode, vacíos y archivo que atraviesa bloques; interrupción y reanudación; cambios de origen; privacidad y hashes remotos.
- [x] Ejecutarlas antes de implementar y registrar el fallo por funcionalidad ausente.
- [x] Implementar recorrido determinista, flujo acotado, controles de integridad y cliente API con paginación y reconciliación.
- [x] Completar los casos de revisión: escala de metadatos, enlaces, POST ambiguos, paginación y entrega publicada.
- [x] Ejecutar `python -B -m unittest discover -s tools/tests -v`; resultado esperado: todos los casos ejecutados correctos, omisiones de plataforma identificadas.

## Task 2: Uso, continuidad y verificación Windows

**Files:** crear `docs/EVIDENCE-UPLOAD.md` y `.github/workflows/evidence-upload-tests.yml`; actualizar `docs/HANDOFF.md`.

**Interfaces:** invocar la CLI de Task 1; documentar formato del manifiesto implementado y comandos PowerShell para la ruta del usuario.

- [x] Documentar preparación remota, subida, reanudación y límites concretos; no afirmar transferencia real.
- [x] Añadir CI de Python en Windows/Linux con fixtures y sin credenciales de carga.
- [x] Revisar la implementación con otro agente y corregir fallos materiales con prueba reproducible.
- [x] Verificar CLI, suite y diff; publicar los archivos mediante actualización de referencia sin force y comprobar CI antes de atribuirle éxito.

## Registro de ejecución

Base: `e693c455518f84570390a6a0c018645af5178b33`. Rama local: `feat/evidence-stream-upload-20261116`. El usuario ya fijó la restricción de disco; los detalles de empaquetado/implementación se resolvieron como decisiones rutinarias, sin pedir otra ronda de aprobaciones. Estado final y evidencia de verificación se registran al completar el trabajo.

Verificación local: 19 pruebas, 18 correctas y una omitida por requerir junctions Windows; CLI y diff correctos. El ciclo inicial observó doce fallos antes de la implementación. Se corrigieron con prueba reproducible la descarga repetida del buffer TAR al fallar, cambios durante el último bloque, respuesta HTTP/JSON truncada y Retry-After con fecha. Dos pruebas adicionales usaron HTTP real en loopback. La revisión independiente no dejó hallazgos materiales; el resultado del CI posterior figura a continuación.

CI inicial `89fd579` / run `43857101962`: Linux 3.10/3.13 correcto; Windows 3.13 reveló la diferencia de `ctime` entre lstat/fstat, y Windows 3.10 una expectativa incorrecta sobre el prefijo del enlace. Se corrigió la comparación cruzada Windows de ctime y bits ejecutables (CPython #157671 y update_st_mode_from_path); se mantienen identidad y metadatos completos en las comparaciones de una misma API. Tres regresiones nuevas y fixtures .exe/.bat: suite local 22 casos, 21 correctos y una omisión por junction Windows. Resultado del nuevo CI registrado a continuación.

**Completado:** corrección publicada y verificada en `783efe380db46e6ddadf6028f84c29cee6534af6`, CI `43857546086` correcto en las cuatro combinaciones Windows/Linux × Python 3.10/3.13. Windows: 22 casos correctos, incluidas junctions. Linux: 21 correctos y una omisión de plataforma. Comando: `python -B -m unittest discover -s tools/tests -v`; CLI `--help` correcta. URL raw publicada contrastada byte a byte con el script verificado. No se accedió a la carpeta Windows del usuario ni se subió evidencia real.
