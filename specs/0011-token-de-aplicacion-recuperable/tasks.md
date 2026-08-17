# 0011 · Tareas

Cada tarea cabe en una sesión, dice qué toca y **cómo se comprueba**. El
repositorio queda funcionando después de cada una.

Orden elegido: primero mover `revoke_app`/`delete_app` a `Nexo` sin cambiar su
comportamiento (T1), para que el resto se construya sobre una base ya
correcta; luego la escritura al crear (T2) y la limpieza al revocar/borrar
(T3, T4), que son las que de verdad dependen de T1; el comando de lectura
(T5); y por último la interfaz (T6, T7).

- [x] **T1.** Mover `revoke_app` y `delete_app` de `commands.rs` (que llaman a
  `state.nexo.db()`) a métodos de `Nexo` que hacen exactamente lo mismo que
  hoy, sin tocar el almacén seguro todavía. Es el criterio 7, aislado del
  resto para que un fallo de compilación no se mezcle con la lógica nueva.
  - Ficheros: `crates/nexo-core/src/service.rs`, `src-tauri/src/commands.rs`
  - Verificación: `cargo test --workspace` en verde (ningún test existente
    cambia de resultado: `apps.rs` sigue probando `Db::revoke_app`/`delete_app`
    directamente, que no se tocan)

- [x] **T2.** `Nexo::create_app` escribe el token en claro en el almacén
  seguro tras crear la aplicación, con `tracing::warn!` y continuar si falla
  (**criterio 1**, **D2**).
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: prueba nueva `create_app_stores_the_token_in_the_secret_store`
    con `MemorySecretStore`, comprobando que `secrets.get(&SecretRef::app_token(id))`
    devuelve exactamente `issued.token`

- [x] **T3.** `Nexo::revoke_app` borra también el secreto (**criterio 2**).
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: prueba nueva `revoking_an_app_deletes_its_secret`: crea,
    comprueba que el secreto existe, revoca, comprueba que
    `secrets.get(...)` devuelve `None`

- [x] **T4.** `Nexo::delete_app` borra también el secreto (**criterio 3**).
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: prueba nueva `deleting_an_app_deletes_its_secret`, mismo
    patrón que T3 con `delete_app`

- [x] **T5.** Comando `app_token_secret` (**criterio 4**).
  - Ficheros: `crates/nexo-core/src/service.rs` (`Nexo::app_token_secret`),
    `src-tauri/src/commands.rs`, `src-tauri/src/main.rs` (registro en
    `invoke_handler`)
  - Verificación: dos pruebas en `service.rs`:
    `app_token_secret_returns_none_when_there_is_nothing_stored` (aplicación
    recién creada con un `SecretStore` que falla a propósito, o revocada) y
    `app_token_secret_returns_the_stored_token` (aplicación recién creada,
    devuelve lo mismo que `issued.token`); `cargo build --workspace` en verde

- [x] **T6.** `api.ts`: `appTokenSecret(appId)`.
  - Ficheros: `src/lib/api.ts`
  - Verificación: `npm run check` en verde

- [x] **T7.** La lista de Aplicaciones: clic copia la clave completa si hay
  secreto, el prefijo con aviso distinto si no, y las aplicaciones revocadas
  muestran el prefijo como texto plano, sin botón (**criterios 5, 6**, **D3**,
  **D4**).
  - Ficheros: `src/lib/views/Apps.svelte`
  - Verificación: `npm run check` en verde + comprobación manual en la
    aplicación instalada: una aplicación creada después de este cambio copia
    la clave completa (se pega y funciona contra el gateway); una aplicación
    anterior (o cualquiera de antes de este cambio, si queda alguna) copia
    solo el prefijo con el aviso; una revocada no tiene botón

## Cierre

- [x] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`
- [x] Aplicación de macOS compilada **e instalada**: `npm run app:install`, con las dos horas
- [x] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real
- [x] Documentación actualizada si lo aprendido contradice lo escrito
- [x] `specs/README.md` actualizado

## Resultado real de la verificación

- T1-T6: `cargo test --workspace` → 296 + 32 pruebas, 0 fallos (5 nuevas de
  esta spec, contra `MemorySecretStore` — mismas rutas de código que usa la
  app real).
- **Llavero real de macOS verificado con un ejemplo desechable**
  (`cargo run --example`, borrado después): escribir, leer y borrar una
  entrada bajo el mismo servicio `com.nexo.gateway` que usa Nexo, confirmado
  también con `security find-generic-password` que no queda rastro.
- `cargo clippy --workspace --all-targets` limpio; `npm run check` 0 errores.
- Instalado: compilado e instalado `Aug 17 21:34:06 2026`.
- **Criterios 5 y 6 (clic real en la interfaz) NO verificados por mí.**
  `osascript`/System Events no tiene permiso de Accesibilidad para pulsar en
  la ventana real de Nexo — el mismo bloqueo que ya apareció en la spec 0007.
  La lógica que se ejecuta al pulsar está probada (T2-T6) y tipada
  (`npm run check`), pero el clic en sí lo tiene que dar el usuario.
