# 0017 · Tareas: Límite de tamaño de peticiones de chat y archivos

- **Estado:** hecho
- **Fichero:** `specs/0017-limite-tamano-peticiones/tasks.md`
- **Spec:** [`spec.md`](spec.md)
- **Diseño:** [`design.md`](design.md)

## Tareas

- [x] **1. Prueba de regresión (TDD): fallo 413 con cuerpos superiores a 2 MiB**
  - Ficheros: `crates/nexo-core/tests/gateway_e2e.rs`
  - Crear prueba que envíe una petición válida con payload > 2 MiB (p.ej. ~3,5 MB).
  - Demostrar que falla con el límite predeterminado de 2 MiB de Axum.
  - Verificación: `cargo test --test gateway_e2e -- request_body_exceeding_two_megabytes` (falló con 413 demostrando el problema).

- [x] **2. Persistencia y configuración del límite en `Db`**
  - Ficheros: `crates/nexo-core/src/db/mod.rs`, `crates/nexo-core/src/config.rs`
  - Implementar `max_request_body_bytes` (32 MiB por defecto) y `set_max_request_body_bytes` con validaciones de rango (1 MiB a 5 GiB o `None`).
  - Pruebas unitarias de lectura/escritura/valores límite y `NULL`.
  - Verificación: `cargo test -p nexo-core -- db::tests::max_request_body_bytes` (pasa con éxito).

- [x] **3. Ingestión segura de cuerpo (memoria/disco), pre-autenticación y error 413 OpenAI**
  - Ficheros: `crates/nexo-core/src/gateway/body.rs`, `crates/nexo-core/src/gateway/routes.rs`, `crates/nexo-core/src/service.rs`
  - Implementar pre-verificación de token y comprobación de pausa antes de leer el cuerpo.
  - Implementar lectura en streaming con límite configurable y almacenamiento temporal en disco si supera 4 MiB.
  - Implementar respuesta 413 estructurada en JSON (`invalid_request_error / request_too_large`).
  - Limpieza de temporales huérfanos al arrancar `Nexo`.
  - Verificación: `cargo test --test gateway_e2e` (pasan todas las pruebas de límites, 413 estructurado, pausa y sin límite).

- [x] **4. Comandos Tauri e interfaz de usuario (Configuración)**
  - Ficheros: `src-tauri/src/commands.rs`, `src/lib/api.ts`, `src/lib/views/Settings.svelte`
  - Validar límites en `save_settings`.
  - Crear sección *Peticiones y archivos* con campo numérico, unidad (MiB/GiB), botón de restaurar 32 MiB, confirmación de «sin límite» y avisos contextuales.
  - Verificación: `npm run check` (0 errores, 0 advertencias).

- [x] **5. Verificación integral y despliegue en Aplicaciones**
  - Ejecutar `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`.
  - Ejecutar `npm run app:install` para instalar en `/Applications/Nexo.app`.
  - Validar funcionamiento con la app real.
