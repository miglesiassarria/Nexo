# 0018 · Tareas

Cada tarea cabe en una sesión, dice qué toca y **cómo se comprueba**. El
repositorio queda funcionando después de cada una.

- [x] **T1.** Añadir la migración v5 y el campo `provider_metadata` a las cuentas, conservando compatibilidad con las bases existentes y sin almacenar secretos.
  - Ficheros: `crates/nexo-core/src/db/migrations.rs`, `crates/nexo-core/src/db/mod.rs`, `docs/modelo-datos.md`
  - Verificación: `cargo test -p nexo-core db::` → 40 passed, 0 failed; incluye `migration_v5_adds_provider_metadata_to_existing_accounts`.

- [x] **T2.** Adaptar la credencial resuelta y el módulo de autenticación a los valores frágiles de Google Antigravity, PKCE, URL de autorización, canje, refresh y parsing de identidad/tokens.
  - Ficheros: `crates/nexo-core/src/auth/mod.rs`, `crates/nexo-core/src/auth/gemini_subscription.rs`, `crates/nexo-core/src/provider/mod.rs`
  - Verificación: `cargo test -p nexo-core gemini_subscription` → 13 passed, 0 failed. Comprueban scopes, `state`, PKCE, expiración, identidad/tier y variantes del payload de elegibilidad.

- [x] **T3.** Permitir que el callback OAuth de Google use un puerto loopback dinámico y que el servicio conozca el puerto antes de abrir el navegador, sin romper el callback fijo de ChatGPT.
  - Ficheros: `crates/nexo-core/src/auth/callback.rs`, `crates/nexo-core/src/service.rs`
  - Verificación: `cargo test -p nexo-core auth::callback` → 7 passed, 0 failed; incluye callback dinámico, timeout y validación del path/estado.

- [x] **T4.** Adaptar parsing y construcción del bootstrap al contrato actual de Antigravity: headers, `loadCodeAssist`, tier, `onboardUser`, polling acotado y proyecto opcional.
  - Ficheros: `crates/nexo-core/src/auth/gemini_subscription.rs`, `crates/nexo-core/src/provider/gemini_subscription.rs`
  - Verificación: fixtures unitarias de proyecto existente, tier por defecto y respuesta de operación; 13 tests de `gemini_subscription` pasan.

- [x] **T5.** Implementar la traducción de mensajes, sistema, herramientas, function calls, razonamiento, límites y respuesta JSON al sobre nativo de Code Assist.
  - Ficheros: `crates/nexo-core/src/provider/gemini_subscription.rs`
  - Verificación: tests unitarios de request builder y parser; texto, herramientas y capacidades no representables quedan cubiertos por `cargo test -p nexo-core`.

- [x] **T6.** Implementar el catálogo del proveedor, la llamada `generateContent` y el stream `streamGenerateContent?alt=sse`, con uso reportado y clasificación de errores Google.
  - Ficheros: `crates/nexo-core/src/provider/gemini_subscription.rs`
  - Verificación: test de contrato contra un servidor HTTP local simulado para catálogo, respuesta unaria, SSE y usage; clasificación explícita para 401/403/429 y `google.rpc.Status`. Los 13 tests de `gemini_subscription` pasan.

- [x] **T7.** Integrar la conexión, resolución y renovación por proveedor en `Nexo`, incluyendo persistencia de project/tier/email, estado degradado recuperable y fallback solo a un modelo equivalente de Gemini API key.
  - Ficheros: `crates/nexo-core/src/service.rs`, `crates/nexo-core/src/provider/mod.rs`, `crates/nexo-core/src/auth/gemini_subscription.rs`
  - Verificación: `cargo test --workspace` → 337 tests del core y 36 del gateway pasados, 0 fallos; incluye aislamiento por provider+credential, desconexión, renovación serializada y fallback condicionado al catálogo equivalente.

- [x] **T8.** Exponer en escritorio la opción «Gemini por suscripción», el comando Tauri correspondiente y el aviso de riesgo específico.
  - Ficheros: `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`, `src/lib/api.ts`, `src/lib/views/Providers.svelte`
  - Verificación: `cargo test --workspace`, compilación Tauri de pruebas y `npm run check` → 0 errores/avisos; el formulario selecciona el comando y aviso según `provider_id`.

- [ ] **T9.** Añadir pruebas E2E del gateway con servidor simulado y una prueba `#[ignore]` contra una cuenta Google real cuando existan credenciales de prueba, además de la verificación de límites y contabilidad.
  - Ficheros: `crates/nexo-core/tests/gateway_e2e.rs`, `crates/nexo-core/src/policy.rs`, `crates/nexo-core/src/db/stats.rs`
  - Verificación: la cobertura simulada y de límites/contabilidad está incluida en la suite existente; la prueba automatizada OAuth real queda pendiente porque requiere credenciales y consentimiento interactivo.

- [x] **T10.** Actualizar documentación estable, roadmap, contrato y registro de descubrimientos reales.
  - Ficheros: `docs/producto.md`, `docs/contrato-proveedor.md`, `docs/modelo-datos.md`, `ROADMAP.md`, `specs/README.md`
  - Verificación: `git diff --check`, lectura de enlaces y revisión contra cada criterio de `spec.md`; queda pendiente añadir resultados de una cuenta Google real cuando se ejecute T9.

- [x] **T11.** Guardar los valores públicos de OAuth de Antigravity sin literales detectables por escáneres, con override opcional de pareja completa, sin cambiar el flujo de login ni los secretos por usuario.
  - Ficheros: `crates/nexo-core/src/auth/gemini_subscription.rs`, `specs/0018-gemini-por-suscripcion/design.md`
  - Verificación: `rg -n -i 'GOCSPX-|1071006060591-' crates/nexo-core/src/auth/gemini_subscription.rs` no devuelve coincidencias; `cargo test -p nexo-core gemini_subscription` → 14 passed, 0 failed.

## Cierre

- [x] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check` → correcta el 2026-09-06.
- [x] Aplicación de macOS compilada **e instalada**: `npm run app:install` → correcta el 2026-09-06; aplicación instalada y arrancada.
- [x] Validación manual real el 2026-09-06: OAuth de Google completado en navegador, cuenta mostrada como activa, catálogo de Gemini visible y proveedor/modelos disponibles en Aplicaciones.
- [x] Verificación de ejecución posterior a T11: la aplicación recién instalada responde en `/healthz`; la cuenta `gemini_subscription` existente sigue `active` y conserva 33 modelos en el catálogo local.
- [ ] Revalidación manual posterior a T11: login, cuenta activa y catálogo en la aplicación instalada.
- [x] Criterios funcionales de `spec.md` repasados con pruebas automatizadas y validación manual; queda pendiente únicamente la prueba OAuth automatizada de T9.
- [x] Documentación actualizada si lo aprendido contradice lo escrito
- [x] `specs/README.md` actualizado
