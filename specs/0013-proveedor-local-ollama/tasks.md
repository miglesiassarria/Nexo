# 0013 · Tareas

Cada tarea deja el repositorio compilando y con las pruebas en verde.

- [x] **T1 · Traducción del catálogo nativo**
  `crates/nexo-core/src/provider/ollama.rs` (nuevo): `parse_native_models` y
  `parse_details` sobre la respuesta **real** de `/api/tags`, capturada hoy.
  Verificación: `cargo test -p nexo-core ollama::tests`

- [x] **T2 · El adaptador**
  Mismo fichero: `OllamaAdapter` con `catalog`, `chat`, `stream`, `health` y
  `probe`, delegando el chat en `translate::chat_completions`. `pub mod ollama`
  en `provider/mod.rs`.
  Verificación: `cargo test -p nexo-core ollama`

- [x] **T3 · `Settings.ollama_base_url`**
  `crates/nexo-core/src/config.rs`, con `11434` por defecto y sin migración de
  base de datos (los ajustes son clave-valor).
  Verificación: `cargo test -p nexo-core settings`

- [x] **T4 · Enganche en el servicio**
  `crates/nexo-core/src/service.rs`: registro del adaptador, `detect_ollama`,
  `ollama_status`, `set_ollama_url`, y la vía en `grantable_routes`.
  Verificación: `cargo test -p nexo-core ollama`

- [x] **T5 · e2e por el gateway**
  `crates/nexo-core/tests/gateway_e2e.rs`: servido de punta a punta con token, y
  `401` sin token.
  Verificación: `cargo test -p nexo-core --test gateway_e2e ollama`

- [x] **T6 · Capa de escritorio e interfaz**
  `src-tauri/src/{commands,main}.rs`, `src/lib/api.ts`,
  `src/lib/views/Providers.svelte`.
  Verificación: `cargo clippy -p nexo --all-targets && npm run check`

- [x] **T7 · Documentación**
  `ROADMAP.md` (Ollama pasa a disponible), `specs/README.md`,
  `website/index.html`.
  Verificación: `npm run site:build`

- [x] **T8 · Cierre**
  Verificación completa, `npm run app:install`, y comprobación contra Ollama
  real por el gateway instalado. Repasar los 8 criterios uno por uno.
  Verificación: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check && npm run app:install`

## Cierre (2026-08-20)

- `cargo test --workspace`: 1 + 304 + 32, 0 fallos, 16 ignoradas (las que gastan
  cuota real de proveedor). Pruebas nuevas: 16. `cargo clippy --workspace
  --all-targets` y `npm run check` limpios.
- Compilado e instalado: `Aug 20 00:38:16 2026`, las dos horas iguales.
- Criterios, uno por uno:
  1. **Cumplido.** `ollama_adapter_is_a_local_provider`.
  2. **Cumplido.** `provider::ollama::tests`, 9 pruebas sobre la respuesta real
     de `/api/tags` capturada hoy, con sus rarezas incluidas
     (`context_length: null`, `family: ""`).
  3. **Cumplido.** `ollama_models_are_accounted_as_local` y, contra lo real,
     `ollama_models_are_catalogued_as_local_and_free`.
  4. **Cumplido.** La cuenta que creó la app instalada tiene
     `credential_kind: local` y `keychain_ref` vacío.
  5. **Cumplido.** `grep -c SecretRef provider/ollama.rs` → 0.
  6. **Cumplido contra Ollama real.** `ollama_models_are_served_through_the_gateway`
     hace una conversación de verdad por HTTP con token, comprueba que llega
     contenido y `usage > 0`, y que sin token es `401`.
  7. **Cumplido.**
  8. **Cumplido a medias, y se dice.** La app instalada detectó Ollama sola al
     arrancar y pobló el catálogo con las capacidades correctas —el modelo de
     27B con visión, el de 0,6B sin ella, y `context_max` vacío donde Ollama
     manda `null`—, todo leído de la base de datos real. Lo que **no** se pudo
     comprobar es la conversación por el proceso instalado: hace falta conceder
     Ollama a una aplicación desde el panel, y ese clic sigue bloqueado por el
     permiso de Accesibilidad denegado. La ruta HTTP completa sí queda
     verificada contra Ollama real por el criterio 6, con el mismo código.

## Un hallazgo que no estaba en el diseño

La vista llamaba a `set_lmstudio_url` para **cualquier** servidor local. Con
solo LM Studio funcionaba; al añadir Ollama, pulsar «Ollama» habría cambiado la
dirección de LM Studio. Se arregló por donde el propio código decía que debía
arreglarse —«un proveedor nuevo que encaje en una de las formas no debe tocar la
vista»—: `detect_local_server(provider_id)` y `set_local_server_url(provider_id,
url)` en el núcleo, y `ConnectOption` gana `provider_id` para que la vista no
tenga que partir el `id` por el `:`. Con su prueba, comprobada en rojo antes.
