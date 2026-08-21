# 0016 · Tareas: Correlación de identificadores en llamadas a herramientas de Responses API

- **Estado:** hecho
- **Fichero:** `specs/0016-correlacion-tool-calls-responses-api/tasks.md`
- **Spec:** [`spec.md`](spec.md)
- **Diseño:** [`design.md`](design.md)

## Tareas

- [x] **1. Prueba de regresión (TDD)**
  - Ficheros: `crates/nexo-core/src/translate/responses.rs`
  - Añadir prueba con secuencia real de Responses API (`item.id: fc_123`, `item.call_id: call_123`).
  - Demostrar que con el flujo SSE actual `ChunkBuilder` produce `index: 0` para start y `index: 1` para deltas.
  - Verificación: `cargo test -p nexo-core --lib -- responses::tests` (la prueba de regresión falló demostrando el error).

- [x] **2. Implementación de `ResponsesEventTranslator`**
  - Ficheros: `crates/nexo-core/src/translate/responses.rs`, `crates/nexo-core/src/provider/chatgpt_subscription.rs`
  - Implementar struct `ResponsesEventTranslator` con mapa de correlación `item_id -> call_id`.
  - Conectar `ResponsesEventTranslator` en el stream de `chatgpt_subscription.rs`.
  - Verificación: `cargo test -p nexo-core --lib -- responses::tests` (la prueba de regresión ahora pasa).

- [x] **3. Pruebas exhaustivas de concurrencia y casos borde**
  - Ficheros: `crates/nexo-core/src/translate/responses.rs`
  - Prueba de dos llamadas a herramientas secuenciales y concurrentes (`index: 0` e `index: 1`).
  - Prueba con `call_id` ausente o idéntico a `item_id`.
  - Verificación: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`.
