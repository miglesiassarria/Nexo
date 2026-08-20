# 0014 · Diseño

Dos llamadas en dos sitios que ya existen. Lo único que merece diseño es dónde
va cada una y qué se comprueba, porque el riesgo no está en el código sino en
cómo reacciona macOS.

## Ficheros

| Fichero | Qué cambia |
| --- | --- |
| `src-tauri/src/main.rs` | al ocultar la ventana en `CloseRequested`, esconder el icono del Dock |
| `src-tauri/src/tray.rs` | `show_panel` lo devuelve antes de mostrar y enfocar |

## Decisiones

### 1. `set_dock_visibility`, no `LSUIElement` en el `Info.plist`

**Alternativa descartada:** declarar la aplicación como accesoria en el
`Info.plist`. Es una línea, pero quita el icono **siempre**, también con el
panel abierto: no habría `⌘Tab`, ni menú de aplicación, ni forma de traer la
ventana al frente salvo por la barra de estado. El usuario pidió que el icono no
esté *cuando no hay ventana*, no que no esté nunca.

### 2. `set_dock_visibility`, no `set_activation_policy`

Las dos existen en Tauri 2.11.5 y la primera delega en la segunda. Se usa la que
dice en su nombre lo que se quiere conseguir; si el día que se lea este código
hay que entender por qué, «visibilidad en el Dock» se explica solo y «política
de activación accesoria» hay que ir a buscarla.

### 3. El orden importa al volver: primero el icono, después el foco

`show_panel` pasa a ser: devolver el icono → `show()` → `unminimize()` →
`set_focus()`. Al revés, la ventana pide el foco mientras la aplicación es
todavía accesoria, que es justo la situación en la que macOS puede no
concederlo. Este orden es el riesgo principal de la especificación y por eso
tiene su propio criterio de aceptación, comprobado contra la aplicación real.

### 4. Nada de esto toca el gateway

El servicio vive en `nexo-core` y no sabe que existe una ventana. La
verificación lo comprueba igualmente con `curl` (criterio 3), porque es la
promesa central del producto y conviene que se rompa en una prueba antes que en
el uso.

## Lo que puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| La ventana no coge el foco al volver | criterio 4, con `lsappinfo` y la ventana real |
| El menú de aplicación no vuelve | mismo criterio, mirando la ventana |
| El gateway se detiene al ocultar | criterio 3, `curl` con el panel cerrado |
| Romper la compilación en Windows o Linux | las llamadas van bajo `cfg(target_os = "macos")`; lo vería el CI |

## Sin pruebas automáticas, y por qué

No hay forma honesta de comprobar esto con `cargo test`: la política de
activación es estado del sistema de ventanas de macOS, no de una función pura.
Escribir una prueba que compruebe «se llamó a la función» solo comprobaría que
la línea existe, que es lo mismo que leerla. Se verifica contra la aplicación
instalada con `lsappinfo`, que es el sistema respondiendo, y queda anotado en
`tasks.md` con su salida real.

## ADR

No hace falta: no cambia ninguna invariante. La 9 y la 10 no se tocan, y el
gateway sigue sirviendo con la ventana cerrada, que es lo que el ADR 0002 fijó
como razón de ser del icono de la barra de estado.
