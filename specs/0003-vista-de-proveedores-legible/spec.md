# 0003 · Una vista de Proveedores que se lee de un vistazo

- **Estado:** spec
- **Creada:** 2026-08-01
- **Pedida por:** el usuario, con una captura: «La vista de proveedores hay que
  mejorarla, aparece todo muy mezclado, para crear proveedores y los proveedores ya
  creados estan todo expandidos. su visualizacion es dificil y poco amigable»

## Problema

La vista de Proveedores tiene cuatro secciones escritas a mano —ChatGPT por
suscripción, LM Studio, OpenAI por API key, otros OpenAI-compatible— y las cuatro
están abiertas siempre. Cada una mezcla en el mismo bloque dos cosas que el usuario
hace en momentos distintos: **mirar lo que ya tiene conectado** y **dar de alta algo
nuevo**. Con cuatro proveedores conectados hay que bajar por tres formularios vacíos
y dos párrafos de ayuda para llegar al final.

El desorden tapa un fallo de verdad, visible en la captura que motivó esta
especificación: **OpenCode Zen aparece dos veces**, una bajo «OpenAI por API key» y
otra en su sección propia. La vista agrupa las cuentas por tipo de credencial
(`credential_kind === "api_key"`) sin mirar de qué proveedor son, así que cualquier
proveedor nuevo con API key se cuela en la caja de OpenAI. El dato para distinguirlos
ya existe: `Account.provider_id`. El fallo es solo de la vista.

Y hay un coste que crece: cada proveedor nuevo obliga a escribir otra sección. Es el
mismo defecto que `CLAUDE.md` prohíbe en el núcleo —«si para añadir un proveedor hay
que tocar el router, el catálogo o las estadísticas, el contrato está mal»— pero en
la interfaz. Ya hay tres proveedores esperando en el ROADMAP (Ollama, Gemini,
Anthropic) y con el diseño actual son tres secciones más.

Al mismo tiempo, la vista **no responde a la pregunta que el usuario se hace**: ¿esto
está funcionando y cuántos modelos me da? Solo LM Studio lo dice. El núcleo ya lo
sabe: `grantable_routes()` devuelve el recuento de modelos y si está conectada cada
pareja proveedor+credencial, y la vista no lo usa.

## Comportamiento esperado

Al abrir Proveedores se ve **una lista de lo que hay conectado**, una fila por pareja
proveedor+credencial, compacta y de una sola línea: nombre, vía de acceso, estado y
cuántos modelos ofrece. Nada más. Si no hay nada conectado, un texto que lo diga y el
botón de añadir.

Cada fila se **despliega** al pulsarla y muestra su detalle: cuándo se conectó, hasta
cuándo vale el token si caduca, la dirección del servidor cuando es editable, las
notas propias de ese proveedor y el botón de desconectar. Solo una desplegada a la
vez, como ya funcionan los permisos en la pestaña Aplicaciones.

Un único botón **«Añadir proveedor»** abre un panel en la misma página con los tipos
disponibles. Al elegir uno aparece solo su formulario: ChatGPT lleva su aviso de
riesgo y el login en el navegador; LM Studio, la dirección del servidor; OpenAI por
API key, la clave y una etiqueta; OpenCode Zen, nombre y dirección ya rellenos y solo
la clave por pegar; otro OpenAI-compatible, los tres campos. Los formularios de los
tipos que no se han elegido no se ven.

La lista de tipos que ofrece ese panel **se deriva de lo que el núcleo declara**, no
de una lista escrita en la interfaz. Añadir un proveedor a Nexo no debe obligar a
tocar esta vista.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | Un proveedor propio con API key aparece **una sola vez**, en su propia fila, y nunca dentro de OpenAI por API key | Prueba de la función de agrupación con una cuenta `opencode-zen`/`api_key` y otra `openai`/`api_key`: cada una cae en su fila |
| 2 | Con lo que el usuario tiene conectado hoy (ChatGPT, LM Studio, Zen), la vista no muestra ningún formulario de alta hasta pulsar «Añadir proveedor» | Inspección en la aplicación instalada, con captura |
| 3 | Cada fila plegada dice nombre, vía, estado y nº de modelos, en una línea | Ídem, contra los datos reales de la máquina |
| 4 | Solo una fila desplegada a la vez | Ídem: al desplegar una segunda, la primera se cierra |
| 5 | El nº de modelos de cada fila coincide con el que devuelve el núcleo | Comparar la fila de Zen con `grantable_routes()`; hoy son 60 modelos |
| 6 | Una vía del catálogo sin cuenta conectada no ocupa fila en la lista de conectados | Prueba de la función de agrupación |
| 7 | El aviso de riesgo de ChatGPT sigue apareciendo, y hay que aceptarlo, antes del primer login de suscripción | Recorrido en la aplicación: elegir ChatGPT en «Añadir proveedor» y comprobar que el botón de login está deshabilitado hasta marcar la casilla |
| 8 | La lista de tipos del panel de alta sale del núcleo: añadir un tipo nuevo no obliga a editar la vista | Revisión del código: no hay lista de tipos escrita a mano en `Providers.svelte` |
| 9 | Todo lo que hoy se puede hacer se sigue pudiendo hacer: conectar y desconectar cada vía, cambiar la dirección de LM Studio, cambiar la dirección de un proveedor propio, comprobar LM Studio | Recorrido en la aplicación instalada, uno por uno |
| 10 | Repositorio verde y aplicación instalada | `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check` y `npm run app:install`, informando de las dos horas |

## Fuera de alcance

- **Rediseñar las otras pestañas.** El usuario señaló Proveedores. Modelos y
  Aplicaciones tienen sus propios problemas y merecen su propia especificación.
- **Pausar un proveedor sin desconectarlo.** Es funcionalidad nueva, no un problema de
  legibilidad.
- **Health checks por proveedor.** Están en el ROADMAP (fase 3) y son trabajo de
  núcleo. Esta vista muestra el estado que ya existe (`active`, `broken`, `expired`),
  no uno nuevo.
- **Cambiar la etiqueta o el nombre de una cuenta ya conectada.** Hoy no se puede y
  seguirá sin poderse.
- **Proveedores nuevos.** Ollama, Gemini y Anthropic siguen fuera. Esto solo hace que
  el día que entren no cuesten una sección más.
- **Ventanas modales.** La aplicación no usa ninguna hoy; el panel de alta va dentro
  de la página para no introducir un patrón nuevo por una vista.

## Supuestos asumidos

- **Solo cambia la interfaz y, si hace falta, el comando que le da los datos.** No se
  toca el modelo de datos, ni el gateway, ni los adaptadores. Si al diseñar resulta
  que falta un dato que el núcleo no expone, se añade un comando de lectura y se dice
  en el diseño.
- **Una fila por pareja proveedor+credencial**, no por proveedor. Es la unidad que ya
  usan el catálogo, los permisos, los límites y las estadísticas (invariante 5). El
  mismo proveedor conectado por dos vías son dos filas.
- **Las notas de ayuda de hoy son valiosas y se conservan**, movidas al detalle
  desplegado o a su formulario: la de los 14 segundos de LM Studio y la del Keychain
  nacieron de confusiones reales.
- **Orden de la lista**: primero lo roto o caducado, porque es lo que exige actuar;
  después lo que funciona. Dentro de cada grupo, por nombre. *(Corregido al diseñar:
  este supuesto decía lo contrario de lo que su propia razón justificaba, y contradecía
  la mitigación escrita en «Riesgos». Ver D2 del diseño.)*

## Riesgos

- **Esconder algo detrás de un despliegue puede hacerlo invisible.** El caso peor es
  una vía rota: si el usuario no despliega, no se entera. Mitigación: el estado va en
  la fila plegada, no en el detalle, y lo roto o caducado se ordena arriba.
- **Un rediseño puede perder funciones por descuido.** Ya pasó con la lista de vías
  escrita a mano, que se quedó sin `lmstudio` y dejó los modelos locales imposibles
  de autorizar. Mitigación: el criterio 9 es un inventario explícito de lo que hoy se
  puede hacer, comprobado uno a uno en la aplicación instalada.
- **Derivar los tipos de alta del núcleo puede quedarse a medias.** ChatGPT y LM
  Studio tienen flujos propios (aviso de riesgo, detección) que no encajan en un
  formulario genérico. Mitigación: el diseño debe decir qué parte se deriva y qué
  parte sigue siendo específica, en lugar de fingir que todo es igual.
- **No hay pruebas automáticas de interfaz en este repositorio.** `npm run check`
  comprueba tipos, no comportamiento. Mitigación: la agrupación y el orden se extraen
  a una función pura y se prueban; lo visual se verifica a mano en la aplicación
  instalada, y así se declara.

## Invariantes que esto no puede romper

- **Ningún secreto en SQLite** (1). Las claves se siguen guardando por los mismos
  comandos, en el Keychain. Esta vista no toca dónde viven.
- **Los límites por aplicación son obligatorios en las vías de suscripción** (4). No
  se toca: se conceden en Aplicaciones, no aquí.
- **El eje de credencial es de primer nivel** (5). Es justo lo que arregla el criterio
  1: la fila es proveedor **y** tipo de credencial, no una de las dos.
- El aviso del [ADR 0001](../../docs/adr/0001-oauth-de-suscripcion.md): el
  consentimiento informado antes del primer login de suscripción no es un paso
  opcional del flujo de alta. Criterio 7.
