# 0002 · Tareas

El repositorio queda funcionando después de cada tarea.

- [x] **T0. Averiguar lo que falta antes de diseñar.**
  Probar los tres endpoints de Zen con modelos gratuitos y de pago, comprobar si el
  formato depende del modelo, y buscar de dónde saca Msty las capacidades.
  - Verificación: hallazgos anotados en `spec.md`; ya hecho, y cambió el diseño
    entero (Zen no necesita adaptador propio) y destapó un fallo real en el traductor
    compartido, ya corregido

- [x] **T1. Migración v2 y CRUD de proveedores añadidos.**
  Tabla `custom_providers (id, name, base_url, created_at)` con el slug como clave
  primaria, y alta, listado y baja.
  - Ficheros: `crates/nexo-core/src/db/migrations.rs`, `crates/nexo-core/src/db/mod.rs`
  - Verificación: `cargo test -p nexo-core custom_provider` — la migración es
    idempotente, y el nombre duplicado se rechaza (criterio 3)

- [x] **T2. Cliente y caché de `models.dev`.**
  Descarga con caducidad de 7 días a fichero, parseo a `ModelDescriptor` con
  capacidades, límites y precio. Respaldo solo-texto si no hay datos.
  - Ficheros: `crates/nexo-core/src/catalog/models_dev.rs`, `catalog.rs` → módulo
  - Verificación: `cargo test -p nexo-core models_dev` con la respuesta real
    capturada de Zen; cubre criterios 4, 5 y 6

- [x] **T3. Clasificación de errores por el cuerpo.**
  `classify_http_error` lee el sobre `{"error":{"type":...}}` antes de mirar el
  status.
  - Ficheros: `crates/nexo-core/src/translate/chat_completions.rs`
  - Verificación: `cargo test -p nexo-core classify` con los tres cuerpos reales de
    Zen (`CreditsError`, `ModelError`, `AuthError`), todos con HTTP 401; cubre
    criterios 7 y 8

- [x] **T4. El adaptador genérico OpenAI-compatible.**
  Sin estado por proveedor: dirección y clave desde la credencial. `catalog()` contra
  `{base}/models` cruzado con `models.dev`, `stream()` y `health()`.
  - Ficheros: `crates/nexo-core/src/provider/openai_compat.rs`,
    `crates/nexo-core/src/provider/mod.rs`
  - Verificación: `cargo test -p nexo-core openai_compat`

- [x] **T5. Resolución del adaptador con respaldo genérico.**
  Cuando no hay adaptador integrado para el `provider_id` y existe en
  `custom_providers`, se usa el genérico. Alta y baja de proveedor en el servicio,
  con su cuenta y su clave en el Keychain.
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: `cargo test -p nexo-core -- custom` incluye dos proveedores
    simultáneos con el mismo `api_id` sin colisión (criterio 2)

- [x] **T6. Comandos y preajuste de Zen.**
  Alta, baja y listado; constante `OPENCODE_ZEN` expuesta a la interfaz.
  - Ficheros: `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`
  - Verificación: `cargo check --workspace`

- [x] **T7. Interfaz.**
  Sección propia de OpenCode Zen con URL ya rellena —solo pedir la clave— y
  formulario genérico para cualquier otro. Listado con baja.
  - Ficheros: `src/lib/api.ts`, `src/lib/views/Providers.svelte`
  - Verificación: `npm run check`; cubre criterio 9

- [x] **T8. Prueba de extremo a extremo contra OpenCode Zen real.**
  Conectar con la clave del usuario, descubrir los 60 modelos, y chat con y sin
  streaming contra un modelo gratuito.
  - Ficheros: `crates/nexo-core/tests/gateway_e2e.rs`, marcada `#[ignore]` porque
    necesita una clave
  - Verificación: `cargo test -p nexo-core --test gateway_e2e -- --ignored zen`;
    cubre criterios 1 y 10

- [x] **T9. Documentación.**
  `docs/modelo-datos.md` con la tabla nueva, `docs/contrato-proveedor.md` con la nota
  de que un adaptador sirve a varios proveedores, `README.md` y `ROADMAP.md`.
  - Verificación: los enlaces resuelven y nada afirma lo contrario de lo medido

## Cierre

- [ ] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`
- [ ] Aplicación de macOS compilada **e instalada**: `npm run app:install`, con las dos horas
- [ ] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real
- [ ] Documentación actualizada si lo aprendido contradice lo escrito
- [ ] `specs/README.md` actualizado
