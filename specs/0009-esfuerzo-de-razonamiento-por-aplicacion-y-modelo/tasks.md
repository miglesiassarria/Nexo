# 0009 · Tareas

Cada tarea cabe en una sesión, dice qué toca y **cómo se comprueba**. El
repositorio queda funcionando después de cada una.

Orden elegido: primero lo que no rompe nada (T1, T2 añaden datos sin usarlos),
luego el arreglo latente por separado (T3, para que su prueba no se mezcle con
el resto), después el almacenamiento (T4, T5), la aplicación del defecto (T6,
T7) y por último la interfaz (T8, T9).

- [x] **T1.** `Capabilities` gana `reasoning_levels: Vec<String>` con
  `#[serde(default)]`, y `ReasoningEffort` gana `parse()`. Nadie lo usa
  todavía: solo tiene que compilar y no romper la deserialización de las filas
  de catálogo que ya existen en disco.
  - Ficheros: `crates/nexo-core/src/provider/mod.rs`
  - Verificación: `cargo test -p nexo-core --lib provider::` y una prueba nueva
    `capabilities_without_reasoning_levels_still_deserialise` que parsea el JSON
    de `caps` **sin** ese campo (el que hay hoy en las bases de datos reales) y
    comprueba que llega con la lista vacía, no con un error

- [x] **T2.** El catálogo de la vía de suscripción conserva los nombres de
  `supported_reasoning_levels` en lugar de contarlos (**criterio 1**).
  - Ficheros: `crates/nexo-core/src/provider/chatgpt_subscription.rs`
  - Verificación: `cargo test -p nexo-core --lib -- reasoning_levels`, contra el
    fixture real ya existente en ese fichero: `gpt-5.6-sol` debe llegar con
    `["low","medium","high","xhigh"]`, y el modelo sin razonamiento con la lista
    vacía y `reasoning: false`

- [x] **T3.** `grant_for` prefiere la coincidencia más específica (exacta >
  prefijo más largo > `*`) en lugar de la primera que encuentra (**D2**, arreglo
  del fallo latente).
  - Ficheros: `crates/nexo-core/src/policy.rs`
  - Verificación: prueba nueva
    `grant_for_prefers_the_most_specific_pattern_in_the_same_route` con `*` y un
    modelo exacto en la **misma** vía, más
    `cargo test -p nexo-core --lib policy::` entero en verde (los tests
    existentes no deben cambiar de resultado)

- [x] **T4.** Migración `V3`: columna `reasoning_effort TEXT` en `app_grants`
  (**criterio 3**).
  - Ficheros: `crates/nexo-core/src/db/migrations.rs`
  - Verificación: prueba nueva `migration_v3_adds_the_effort_column_and_keeps_grants`
    que crea el esquema en versión 2, inserta un permiso, aplica migraciones y
    comprueba que el permiso sigue ahí con nivel nulo; más
    `cargo test -p nexo-core --lib db::migrations`

- [x] **T5.** `Grant` gana `reasoning_effort: Option<String>`; `grants()` lo lee,
  `set_grant` lo escribe y `replace_app_models` recibe el nivel por modelo en
  lugar de una lista de nombres (**criterio 2**, **D1**, **D6**).
  - Ficheros: `crates/nexo-core/src/apps.rs`; corrección del comentario de la
    línea 338, que deja de ser cierto
  - Verificación: prueba nueva `reasoning_effort_roundtrip` con **dos** modelos y
    niveles distintos, comprobando que cada fila conserva el suyo (con un solo
    modelo la prueba pasaría con el fallo dentro), más otra
    `marking_another_model_keeps_the_efforts_already_configured`; y
    `cargo test -p nexo-core --lib apps::` en verde

- [x] **T6.** `prepare_inner` aplica el defecto: deja de descartar el
  `PolicyDecision` e inyecta `req.reasoning` solo si el cliente no mandó nivel,
  el permiso tiene uno y el modelo lo sigue declarando (**D3**, **D4**).
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: `cargo test -p nexo-core --lib service::` en verde (nada de lo
    que ya existe debe cambiar de resultado; el comportamiento nuevo se prueba
    de extremo a extremo en T7)

- [x] **T7.** Pruebas de extremo a extremo del comportamiento acordado
  (**criterios 4, 5, 6 y 7**), contra el proveedor mock para no gastar cuota.
  - Ficheros: `crates/nexo-core/tests/gateway_e2e.rs`
  - Verificación: `cargo test -p nexo-core --test gateway_e2e -- configured_effort the_client_wins no_configured_effort an_effort_no_longer_supported`, con
    una prueba por criterio:
    `configured_effort_is_used_when_the_client_sends_none`,
    `the_client_wins_over_the_configured_effort`,
    `no_configured_effort_changes_nothing` y
    `an_effort_no_longer_supported_is_kept_and_flagged`

- [x] **T8.** La orden y los tipos de la interfaz transportan el nivel:
  `set_app_models` recibe el nivel por modelo y `RouteModel` expone
  `configured_effort`.
  - Ficheros: `src-tauri/src/commands.rs`, `crates/nexo-core/src/service.rs`
    (`RouteModel`), `src/lib/api.ts`
  - Verificación: `cargo build --workspace && npm run check` en verde

- [x] **T9.** Selector de nivel por modelo marcado en la vista de permisos
  (**criterio 8**): solo en modelos que declaran razonamiento, solo con los
  niveles que Nexo sabe enviar, con «sin especificar» siempre, y marcando como
  huérfano un nivel que el modelo ya no admite.
  - Ficheros: `src/lib/views/Apps.svelte`
  - Verificación: `npm run check` en verde y comprobación en la aplicación
    instalada con la cuenta de suscripción real

## Cierre

- [x] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`
- [x] Aplicación de macOS compilada **e instalada**: `npm run app:install`, con las dos horas
- [x] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real
- [x] Documentación actualizada si lo aprendido contradice lo escrito
- [x] `specs/README.md` actualizado

## Resultado real de la verificación

- T1-T7 verificadas con pruebas ejecutadas. Tres de ellas se comprobaron **en
  rojo antes del arreglo** desactivando temporalmente el código y volviéndolo a
  poner, para demostrar que tienen dientes: la de `grant_for` (devolvía `*` en
  vez del modelo exacto), la del criterio 4 (llegaba «ninguno» en vez de
  «high») y la del criterio 7 (llegaba «xhigh» al proveedor).
- T8 y T9: `cargo build --workspace`, `npm run check` y `cargo clippy` limpios.
- Repositorio: `cargo test --workspace` → 289 + 32 pruebas, 0 fallos.
- Instalado: compilado e instalado `Aug 4 10:57:26 2026`.
- **Criterio 8 NO verificado con la cuenta real.** Ver «Lo que se descubrió al
  construir» en `spec.md`.
