# 0019 · Tareas

Cada tarea cabe en una sesión, dice qué toca y **cómo se comprueba**. El
repositorio queda funcionando después de cada una.

- [x] **T1.** Centralizar las versiones de catálogo candidatas con el orden `99.0.0`, `0.155.0` y la configuración histórica, sin duplicados.
  - Ficheros: `crates/nexo-core/src/auth/chatgpt.rs`
  - Verificación: test unitario del orden y la deduplicación.
  - Evidencia: `cargo test -p nexo-core catalog_` incluye `catalog_versions_are_ordered_and_deduplicated`.

- [x] **T2.** Hacer que el adaptador consulte candidatos hasta encontrar un catálogo utilizable, respetando errores de auth/rate limit y manteniendo Nexo como originator.
  - Ficheros: `crates/nexo-core/src/provider/chatgpt_subscription.rs`
  - Verificación: pruebas del adaptador con servidor HTTP simulado para éxito inicial, fallback por 400/catálogo vacío, y salida inmediata en 401/429.
  - Evidencia: `cargo test -p nexo-core catalog_` → 14 passed, 0 failed; incluye reintento por HTTP 400/catálogo vacío, salida inmediata en 401/429 y conservación del catálogo anterior tras error.

- [x] **T3.** Retirar el control manual de versión del uso normal y explicar en la documentación que el descubrimiento negocia automáticamente.
  - Ficheros: `src/lib/views/Settings.svelte`, `docs/producto.md`, `docs/adr/0001-oauth-de-suscripcion.md`
  - Verificación: `npm run check` y revisión de la sección de catálogo/ADR.
  - Evidencia: `npm run check` → 0 errores y 0 avisos; revisión de interfaz, ADR, documentación de producto y esquema de datos.

- [x] **T4.** Cerrar spec, actualizar el índice y ejecutar la verificación completa de CI local.
  - Ficheros: `specs/README.md`, carpeta `specs/0019-*`
  - Verificación: `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `npm run check` y `git diff --check`.
  - Evidencia: `cargo test --workspace` → 1 + 348 + 36 tests pasaron, 16 ignorados; Clippy pasó; interfaz 0 errores y avisos; `git diff --check` limpio.

## Cierre

- [x] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`
- [ ] PR comprobada, fusionada tras el check requerido y rama local eliminada
- [x] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real
- [x] Documentación actualizada si lo aprendido contradice lo escrito
- [x] `specs/README.md` actualizado
