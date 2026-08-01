# 0004 · Tareas

Cada tarea cabe en una sesión, dice qué toca y **cómo se comprueba**. El
repositorio queda funcionando después de cada una.

El orden va de dentro afuera: primero la función que decide, que es donde está el
fallo real, y con ella el catálogo deja de discrepar del gateway antes de que exista
ninguna interfaz nueva. Nada queda a medias entre tareas porque hasta T4 el
comportamiento observable no cambia: todos los permisos siguen siendo `*`.

- [x] **T1.** `policy::grant_for()`: extraer la decisión a una función pública y hacer
      que `check()` la use. Sin cambio de comportamiento todavía. Pruebas de
      `model_matches` con los casos raros: nombre con `/`, nombre con `*` literal,
      patrón con prefijo.
  - Ficheros: `crates/nexo-core/src/policy.rs`
  - Verificación: `cargo test -p nexo-core policy`

- [x] **T2.** `build_models_for_app()` pasa a usar `grant_for()`. Aquí se arregla el
      desajuste: el catálogo empieza a aplicar la misma regla que el gateway. Con la
      prueba del criterio 4, que recorre el catálogo entero de una aplicación y exige
      que cada modelo listado pase el control y ninguno de los no listados lo pase.
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: `cargo test -p nexo-core -- catalog_and_gateway`

- [x] **T3.** `replace_app_models()` en `apps.rs`: reemplazo transaccional del conjunto
      de modelos de una vía, con el límite obligatorio de suscripción incluido. Pruebas:
      el conjunto vacío borra las filas, marcar un solo modelo de la vía de suscripción
      crea su límite, y reemplazar no deja filas de la selección anterior.
  - Ficheros: `crates/nexo-core/src/apps.rs`
  - Verificación: `cargo test -p nexo-core replace_app_models`

- [x] **T4.** Los cuatro motivos de catálogo vacío, incluido `no_models_match` para los
      huérfanos, y el mensaje de `no_grants` hablando de modelos y no solo de vías.
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: `cargo test -p nexo-core -- empty_catalog_reason`

- [x] **T5.** `create_app_with_access` deja de conceder y se renombra a `create_app`.
      Prueba: una aplicación nueva nace con cero permisos.
  - Ficheros: `crates/nexo-core/src/service.rs`, `src-tauri/src/commands.rs`
  - Verificación: `cargo test -p nexo-core -- a_new_app_is_born_with_no_access`

- [x] **T6.** `app_route_models()`: para una aplicación y una vía, los modelos del
      catálogo con si están marcados, más los marcados que ya no existen. Es lo que
      pinta la interfaz.
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: `cargo test -p nexo-core app_route_models`

- [x] **T7.** Comandos `set_app_models` y `app_route_models`, retirando `set_app_access`.
  - Ficheros: `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`
  - Verificación: `cargo clippy --workspace --all-targets` sin avisos

- [x] **T8.** Tipos y envoltorios en la interfaz.
  - Ficheros: `src/lib/api.ts`
  - Verificación: `npm run check`

- [x] **T9.** Interfaz: dentro de cada vía, la lista de modelos con casilla, buscador,
      «marcar los visibles» y «desmarcar los visibles», el recuento «3 de 60», el aviso
      de «todos» heredado y la marca de los huérfanos. Y el texto del alta corregido.
  - Ficheros: `src/lib/views/Apps.svelte`
  - Verificación: `npm run check`

- [x] **T10.** Pruebas de extremo a extremo por HTTP de los criterios 1, 2, 3 y 6 con el
      proveedor mock: catálogo recortado, modelo no marcado rechazado nombrando el
      modelo, modelo marcado funcionando, y permiso heredado con `*` dando todos.
  - Ficheros: `crates/nexo-core/tests/gateway_e2e.rs`
  - Verificación: `cargo test -p nexo-core --test gateway_e2e`

- [ ] **T11.** Comprobar en la aplicación instalada el criterio 9 con la vía real de Zen
      y sus 60 modelos, y que Studio sigue funcionando con su permiso heredado.
  - Ficheros: ninguno
  - Verificación: `npm run app:install` y recorrido manual

- [x] **T12.** Documentación: qué significa ahora una fila de `app_grants` y qué
      significa `*`; el permiso más fino ya no es la vía. Y anotar en el diseño lo que
      la realidad haya corregido.
  - Ficheros: `docs/modelo-datos.md`, `docs/producto.md`, `specs/0004-modelos-permitidos-por-aplicacion/design.md`
  - Verificación: lectura; los nombres de tabla, columna y comando citados existen

## Cierre

- [ ] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`
- [ ] Aplicación de macOS compilada **e instalada**: `npm run app:install`, con las dos horas
- [ ] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real
- [ ] Documentación actualizada si lo aprendido contradice lo escrito
- [ ] `specs/README.md` actualizado
