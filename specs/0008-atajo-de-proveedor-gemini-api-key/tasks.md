# 0008 · Tareas

Cada tarea cabe en una sesión, dice qué toca y **cómo se comprueba**. El
repositorio queda funcionando después de cada una.

- [x] **T1.** Añadir la constante `GEMINI: ProviderPreset` y sumarla a `presets()`.
  - Ficheros: `crates/nexo-core/src/provider/openai_compat.rs`
  - Verificación: `cargo test -p nexo-core --lib provider::openai_compat` → 9 passed, 0 failed.

- [x] **T2.** Prueba de que el atajo llega con nombre y dirección puestos y convive con Zen y OpenRouter (criterios 1 y 2 de `spec.md`).
  - Ficheros: `crates/nexo-core/src/service.rs` (`the_gemini_shortcut_arrives_with_its_name_and_address_filled`)
  - Verificación: `cargo test -p nexo-core --lib -- gemini_shortcut connect_options_cover_the_four_form_shapes` → 2 passed.

- [x] **T3.** Prueba de extremo a extremo: descubrimiento del catálogo real de Gemini y su enriquecimiento con `models.dev` (criterio 3).
  - Ficheros: `crates/nexo-core/tests/gateway_e2e.rs` (`start_with_gemini`, `gemini_discovers_its_real_catalog_and_enriches_it_with_models_dev`); `crates/nexo-core/src/catalog/models_dev.rs` (arreglo real: `lookup` no encontraba los ids de Gemini por el prefijo `models/`)
  - Verificación: `NEXO_TEST_GEMINI_API_KEY=... cargo test -p nexo-core --test gateway_e2e -- --ignored gemini_discovers` → **ok, verificado 3 veces consecutivas contra la API real con clave real.**

- [x] **T4.** Prueba de extremo a extremo: chat sin streaming y con streaming responde con texto y queda contabilizado como `api_key`/`Metered` (criterio 4).
  - Ficheros: `crates/nexo-core/tests/gateway_e2e.rs` (`gemini_chat_with_a_real_model_works_end_to_end`, `gemini_streaming_reassembles_correctly`)
  - Verificación: `NEXO_TEST_GEMINI_API_KEY=... cargo test -p nexo-core --test gateway_e2e -- --ignored gemini_chat gemini_streaming` → **ok, verificado 3 veces consecutivas contra la API real con clave real.**

- [x] **T5.** Prueba de extremo a extremo: clave inválida y modelo inexistente dan un error claro, no un `502` genérico (criterio 5).
  - Ficheros: `crates/nexo-core/tests/gateway_e2e.rs` (`gemini_invalid_key_is_a_clear_auth_error`, `gemini_unknown_model_is_rejected_with_a_pointer_to_the_catalog`); `crates/nexo-core/src/translate/chat_completions.rs` (arreglo real: `parse_error_envelope` no reconocía el sobre `google.rpc.Status` de Gemini y su envoltura en array)
  - Verificación: `NEXO_TEST_GEMINI_API_KEY=... cargo test -p nexo-core --test gateway_e2e -- --ignored gemini_invalid_key gemini_unknown_model` → **ok, verificado 3 veces consecutivas contra la API real con clave real.**

- [x] **T6.** Actualizar `specs/README.md` con la fila de la spec 0008.
  - Ficheros: `specs/README.md`
  - Verificación: lectura visual; la tabla mantiene el formato de las filas existentes.

## Cierre

- [ ] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`
- [ ] Aplicación de macOS compilada **e instalada**: `npm run app:install`, con las dos horas
- [ ] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real
- [ ] Documentación actualizada si lo aprendido contradice lo escrito
- [ ] `specs/README.md` actualizado
