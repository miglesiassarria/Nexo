# 0014 · Tareas

- [x] **T1 · Esconder el icono al cerrar la ventana**
  `src-tauri/src/main.rs`, en el `CloseRequested` que ya oculta el panel.
  Verificación: `cargo clippy -p nexo --all-targets`

- [x] **T2 · Devolverlo al abrir desde la barra de estado**
  `src-tauri/src/tray.rs`, en `show_panel`, antes de mostrar y enfocar.
  Verificación: `cargo clippy -p nexo --all-targets`

- [x] **T3 · Cierre**
  Verificación completa, `npm run app:install`, y los cuatro criterios contra la
  aplicación instalada con `lsappinfo` y `curl`, con su salida real anotada.
  Verificación: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check && npm run app:install`

## Cierre (2026-08-20)

- `cargo test --workspace`: 1 + 308 + 32, 0 fallos. Clippy y `npm run check`
  limpios. Sin pruebas nuevas, por el motivo del diseño: aquí no hay nada puro
  que comprobar, solo estado del sistema de ventanas de macOS.
- Compilado e instalado: `Aug 20 10:19:55 2026`.
- Criterios, contra la aplicación instalada:
  1. **Cumplido.** Con la ventana visible, `lsappinfo` informa
     `ApplicationType="Foreground"`.
  2. **Cumplido.** Con el panel cerrado por el usuario,
     `ApplicationType="UIElement"` — que es lo que en macOS significa «sin icono
     en el Dock». El usuario lo confirmó además a la vista: el icono desaparece.
  3. **Cumplido.** En ese mismo estado, sin ventana y sin icono:
     `GET /healthz` → `200`, y `GET /v1/models` con el token de una aplicación
     real → `200`. El gateway no se enteró.
  4. **Cumplido.** El usuario abrió el panel desde la barra de estado y funciona.
  5. **Cumplido.**

## Lo que salió mal al verificarlo, y conviene no repetir

Para no depender del clic del usuario se instrumentó una copia del binario que
ocultaba la ventana sola a los 4 segundos. La copia arrancó con el puerto por
defecto —el `update` del puerto en su base temporal no surtió efecto— y reservó
`127.0.0.1:8787` **al lado** del `*:8787` de la instancia real del usuario.

Por el reparto que la spec 0012 documentó, el socket más específico gana: durante
unos 40 segundos, todo lo que un cliente local mandara al 8787 lo habría
atendido la copia, con una base vacía, respondiendo `401`. Se detectó al leer su
log, se mató el proceso y se comprobó que la instancia real volvía a ser la
única dueña del puerto y que los datos del usuario seguían intactos.

Lección: una copia de diagnóstico tiene que fallar si no consigue el puerto que
se le pidió, no caer al de siempre. Verificar el puerto **antes** de dejarla
correr, no después.
