# 0003 · Tareas

Cada tarea cabe en una sesión, dice qué toca y **cómo se comprueba**. El
repositorio queda funcionando después de cada una.

El orden importa: el núcleo primero, con sus pruebas, y la vista al final. Así los
criterios que se pueden probar (1, 5, 6) están cubiertos antes de tocar nada visual, y
la interfaz vieja sigue funcionando mientras se construye la nueva.

- [x] **T1.** `ProviderRow` y `provider_rows()`: cruzar cuentas con los recuentos del
      catálogo, decidir `needs_attention` y ordenar. Con pruebas que cubren los
      criterios 1, 5 y 6: dos cuentas de API key de proveedores distintos caen en filas
      distintas, el recuento coincide con el catálogo, una vía sin cuenta no genera
      fila, una cuenta `broken` sale primera, y un estado desconocido cuenta como que
      exige atención.
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: `cargo test -p nexo-core provider_rows`

- [x] **T2.** `ConnectOption`, `ConnectForm` y `connect_options()`: declarar las vías
      que se pueden añadir con su forma de formulario, absorbiendo lo que hoy hace
      `provider_presets()`. Con pruebas: aparecen las cuatro formas, el atajo de Zen
      llega con nombre y dirección, `already_connected` refleja la realidad, y cada
      forma tiene su comando de alta.
  - Ficheros: `crates/nexo-core/src/service.rs`, `crates/nexo-core/src/provider/openai_compat.rs`
  - Verificación: `cargo test -p nexo-core connect_options`

- [x] **T3.** Exponer los dos comandos y retirar `provider_presets`, que queda absorbido.
  - Ficheros: `src-tauri/src/commands.rs`, `src-tauri/src/main.rs`
  - Verificación: `cargo clippy --workspace --all-targets` sin avisos

- [x] **T4.** Tipos y envoltorios en la interfaz, con `ConnectForm` como unión
      discriminada para que TypeScript obligue a cubrir las cuatro formas.
  - Ficheros: `src/lib/api.ts`
  - Verificación: `npm run check`

- [x] **T5.** Reescribir la vista: lista de filas plegables (una desplegada a la vez) y
      panel de alta con una rama por forma de formulario. Sin lista de tipos escrita a
      mano y sin perder ninguna acción del criterio 9.
  - Ficheros: `src/lib/views/Providers.svelte`
  - Verificación: `npm run check`, y revisión del código para el criterio 8

- [ ] **T6.** Comprobar en la aplicación instalada los criterios que no se pueden
      automatizar (2, 3, 4, 7, 9), con la máquina del usuario y sus tres proveedores
      conectados de verdad.
  - Ficheros: ninguno
  - Verificación: `npm run app:install` y recorrido manual, informando de lo que se ve

- [x] **T7.** Llevar a `docs/` lo que quede desfasado o aprendido, y anotar en el diseño
      lo que la realidad haya corregido.
  - Ficheros: `docs/producto.md`, `specs/0003-vista-de-proveedores-legible/design.md`
  - Verificación: lectura; los enlaces y los nombres de comando citados existen

## Cierre

- [ ] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`
- [ ] Aplicación de macOS compilada **e instalada**: `npm run app:install`, con las dos horas
- [ ] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real
- [ ] Documentación actualizada si lo aprendido contradice lo escrito
- [ ] `specs/README.md` actualizado
