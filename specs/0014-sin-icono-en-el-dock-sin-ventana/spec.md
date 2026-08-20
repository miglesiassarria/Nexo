# 0014 · Sin icono en el Dock cuando no hay ventana

- **Estado:** build
- **Creada:** 2026-08-20
- **Pedida por:** el usuario: «el icono de la app no debe aparecer en la barra
  inferior si no tengo la ventana activa. Si no tengo una ventana activa de
  Nexo, se debe quedar trabajando en segundo plano y el único icono que debe
  tener es el de la barra superior».

## Problema

Nexo está pensado para vivir en segundo plano: cerrar la ventana oculta el panel
y el gateway sigue sirviendo. Pero el icono se queda en el Dock igual, ocupando
sitio como si fuera una aplicación abierta que el usuario está usando. Molesta,
y contradice lo que el producto es: un servicio con panel opcional, no una
aplicación de ventana.

El icono de la barra de estado ya existe y ya es el punto de acceso permanente.

## Comportamiento esperado

1. Con el panel **visible**, Nexo aparece en el Dock como cualquier aplicación.
   Sin esto no habría forma de traerlo al frente con el conmutador de
   aplicaciones ni de usar su menú.
2. Al **cerrar la ventana** (botón rojo o `⌘W`), el panel se oculta —como ya
   hace hoy— y el icono del Dock **desaparece**. Solo queda el de la barra de
   estado.
3. Al **abrir el panel** desde la barra de estado, el icono del Dock vuelve y la
   ventana coge el foco.
4. El gateway no se entera de nada: sigue sirviendo en los dos estados. Cerrar
   la ventana nunca ha detenido el servicio y esto no lo cambia.

## Criterios de aceptación

1. **Con ventana visible, el sistema clasifica Nexo como aplicación de primer
   plano.** `lsappinfo` informa `ApplicationType="Foreground"`.
2. **Sin ventana, deja de serlo.** Tras cerrar el panel, `lsappinfo` informa un
   tipo distinto de `Foreground` (`UIElement` o equivalente), que es lo que en
   macOS significa «sin icono en el Dock».
3. **El gateway sigue sirviendo sin ventana.** `GET /healthz` responde `200` con
   el panel cerrado y el icono del Dock ausente.
4. **Volver a abrir el panel restaura el icono y el foco.** Tras usar «Abrir
   panel de Nexo» en la barra de estado, `lsappinfo` vuelve a `Foreground` y la
   ventana está visible.
5. **La verificación del repositorio pasa y la app queda instalada.**
   `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check && npm run app:install`

Los criterios 1 a 4 se comprueban contra la aplicación instalada, con
`lsappinfo` y `curl`, informando de la salida real.

## Fuera de alcance

- **Que sea configurable.** Es el comportamiento que el usuario quiere, no una
  preferencia; añadir un interruptor es inventar una decisión que nadie ha
  pedido. Si algún día hace falta, se añade entonces.
- **Arrancar sin ventana.** Nexo sigue abriendo el panel al iniciarse. Arrancar
  oculto es otra petición, y hoy sería una sorpresa desagradable la primera vez
  que se instala.
- **`⌘H` (ocultar aplicación).** Es un gesto del sistema con su propio
  significado y su propio comportamiento en el Dock; no se toca.
- **Windows y Linux.** El Dock es de macOS. En el resto la llamada no existe y
  el comportamiento no cambia.
- **Reaccionar al clic en el icono del Dock cuando existe.** macOS ya trae la
  ventana al frente; no hace falta añadir nada.

## Riesgos

- **Que la ventana no coja el foco al volver.** Es el efecto secundario clásico
  de cambiar la política de activación en macOS: sin icono en el Dock la
  aplicación no puede activarse igual. Se comprueba en el criterio 4, contra la
  aplicación real, no razonando.
- **Que el menú de la aplicación desaparezca y no vuelva.** macOS retira la
  barra de menús junto con el icono del Dock. Lo cubre el mismo criterio 4.

## Supuestos declarados

- «Ventana activa» se interpreta como **ventana visible**, no como ventana con
  el foco. Quitar el icono del Dock mientras hay una ventana abierta pero sin
  foco dejaría al usuario sin forma de volver a ella con `⌘Tab`, que es peor que
  el problema que se arregla.
- Se usa `AppHandle::set_dock_visibility`, verificada en el código de Tauri
  2.11.5 antes de escribir esto: existe, está limitada a macOS y delega en la
  política de activación del sistema.
