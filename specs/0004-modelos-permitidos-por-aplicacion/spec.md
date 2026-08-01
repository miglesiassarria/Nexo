# 0004 · Elegir qué modelos sirve Nexo a cada aplicación

- **Estado:** spec
- **Creada:** 2026-08-01
- **Pedida por:** el usuario: «ahora quiero poder limitar en las aplicaciones que
  creemos los modelos que sirve cada aplicacion de modo que con el api key podamos
  limitar que es lo que sirve cada aplicacion y no forzosamente servir por ejemplo los
  60 modelos de zen»

## Problema

El permiso más fino que Nexo sabe conceder hoy es una **vía**: proveedor más tipo de
credencial. Conceder «opencode-zen · API key» a una aplicación le da los 60 modelos de
Zen, sin más opción. No hay forma de decir «esta herramienta solo usa dos modelos
baratos» ni «esta no toca los modelos de pago».

Eso importa por tres motivos distintos. **Coste**: con una vía facturada por token,
cualquier modelo caro es alcanzable desde cualquier herramienta que tenga el token.
**Ruido**: un selector con 60 modelos en una herramienta que solo va a usar uno es
peor experiencia que una lista de uno. **Contención**: si mañana una aplicación se
comporta mal, hoy la única respuesta es revocarle la vía entera.

Y hay una parte del problema que ya está en el código, a medio hacer. La tabla
`app_grants` tiene `model_pattern` en su clave primaria desde la primera migración, y
`PolicyEngine::check()` ya lo comprueba con `model_matches()`. Pero **nada lo escribe
nunca**: los cinco sitios que crean permisos ponen `"*"`. Peor: `build_models_for_app`
—lo que responde `GET /v1/models`— **no** filtra por ese patrón. Si hoy alguien
escribiera un patrón estrecho en la base de datos a mano, el catálogo seguiría
anunciando los 60 modelos y el gateway rechazaría 58 de ellos en ejecución. Es el
peor tipo de fallo: el cliente cree que puede y descubre que no cuando ya está
enviando la petición.

Hay además una incoherencia con un principio que el propio proyecto declara.
`policy.rs` dice, en un comentario sobre el hueco donde se busca el permiso: «Sin
fila no hay permiso: el acceso se concede, no se deniega». Pero
`create_app_with_access` concede automáticamente **todas** las vías conectadas al
crear una aplicación, y la interfaz lo promete por escrito: «Nace con acceso a las
vías que ya tengan una cuenta conectada». Una cosa dice el código y otra hace.

## Comportamiento esperado

En los permisos de una aplicación, cada vía muestra **sus modelos**, y el usuario
marca los que esa aplicación puede usar. Lo marcado es lo que se sirve; lo no marcado
no existe para esa aplicación.

- `GET /v1/models` con el token de esa aplicación devuelve **solo** los modelos
  marcados. Lo que la aplicación ve en su selector es exactamente lo que puede pedir.
- Una petición a un modelo no marcado se rechaza con un error que dice que esa
  aplicación no tiene permiso para ese modelo, y que se concede desde Nexo. No se
  sustituye por otro modelo ni se degrada.
- Una vía **sin ningún modelo marcado no sirve nada**. Al conceder una vía nueva se
  empieza así, y la interfaz lo dice en lugar de dejar a la aplicación muda sin
  explicación.
- Con 60 modelos, marcar uno a uno no puede ser el único camino: hay un buscador y un
  «marcar todos los visibles» que actúa sobre lo que el filtro deja a la vista.
- Las aplicaciones que ya funcionan hoy siguen funcionando: los permisos existentes
  valen para todos los modelos de su vía, y esa vía se muestra como «todos», con la
  opción de estrecharla.
- El límite obligatorio de las vías de suscripción sigue siendo por vía, no por
  modelo. Elegir modelos no lo relaja.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | Con dos modelos marcados de una vía de 60, `GET /v1/models` devuelve dos | Prueba de extremo a extremo por HTTP contra el proveedor mock, con una vía de varios modelos |
| 2 | Una petición a un modelo no marcado se rechaza con un error que nombra el modelo, y **no** se sirve otro | Prueba de extremo a extremo: el cuerpo del error cita el modelo y el código no es 200 |
| 3 | Una petición a un modelo marcado funciona igual que antes | Prueba de extremo a extremo, misma vía, modelo marcado |
| 4 | El catálogo y el gateway aplican **la misma** regla: nada listado es rechazable, nada rechazable es listado | Prueba que recorre todo el catálogo de la aplicación y comprueba que cada modelo listado pasa el control de permisos |
| 5 | Un catálogo vacío deja rastro con un motivo que distingue los casos: nada concedido, sin cuenta activa, modelos marcados que ya no existen, o catálogo del proveedor vacío | Pruebas de `models_for_app` para los cuatro motivos. *(Corregido al diseñar: este criterio pedía distinguir «vía concedida sin modelos marcados» de «vía no concedida», y son el mismo estado —cero filas— por el supuesto de que conceder y marcar son la misma acción. El caso que sí existe y merecía motivo propio es el de los modelos huérfanos. Ver D4 del diseño.)* |
| 6 | Un permiso existente con `*` sigue dando acceso a todos los modelos de su vía tras actualizar | Prueba: se crea un permiso con `*`, se leen los modelos y salen todos los de la vía |
| 7 | Al crear una aplicación nueva no se concede nada, y la interfaz explica que hay que elegir | Prueba de `create_app`: cero permisos; y revisión del texto de la interfaz, que hoy promete lo contrario |
| 8 | El límite obligatorio de una vía de suscripción se sigue aplicando aunque solo haya un modelo marcado | Prueba: marcar un modelo de la vía de suscripción crea el límite obligatorio |
| 9 | Marcar y desmarcar 60 modelos no exige 60 clics: hay buscador y «marcar los visibles» | Recorrido en la aplicación instalada, con la vía real de Zen |
| 10 | Repositorio verde y aplicación instalada | `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check` y `npm run app:install`, con las dos horas |

## Fuera de alcance

- **Patrones escritos a mano** (`claude-*`). El almacenamiento los soporta y se
  seguirán respetando los que existan, pero la interfaz no ofrecerá escribirlos: con
  una lista de modelos reales delante, un patrón es una forma de equivocarse sin
  enterarse. Si hace falta, va en otra especificación.
- **Listas de exclusión** («todos menos estos»). El usuario eligió marcar los que sí.
  Un modelo nuevo del proveedor no se sirve hasta que se marca, y eso es la
  consecuencia buscada, no un efecto secundario.
- **Límites por modelo.** Los límites siguen siendo por vía. Un tope de gasto por
  modelo es otro problema.
- **Capacidades por modelo** (herramientas o multimodal para unos sí y otros no).
  Siguen siendo por vía, como hoy.
- **Elegir modelos al crear la aplicación.** El alta sigue siendo un paso; los
  permisos se conceden después, en su panel.

## Supuestos asumidos

- **Se aplica a todas las vías**, no solo a las de API key. El usuario nombró la API
  key porque Zen es su caso, pero restringir solo ahí sería arbitrario: la vía de
  suscripción es justo donde más importa quién puede gastar la cuota.
- **`*` se conserva como «todos los modelos de esta vía»**, no se expande a filas
  concretas al actualizar. Expandirlo dejaría a las aplicaciones existentes sin los
  modelos que el proveedor añada en el futuro, que es un cambio de comportamiento
  silencioso sobre algo que hoy funciona. Con esto, el criterio 6 se cumple sin tocar
  los datos del usuario.
- **La unidad que se marca es el nombre público del modelo** (`opencode-zen/…`), el
  mismo que viaja en la petición y con el que ya trabaja `model_matches`.
- **Conceder una vía y elegir sus modelos son la misma acción.** No hay «vía
  concedida con cero modelos» como estado aparte: marcar el primer modelo concede la
  vía y desmarcar el último la retira. Un estado intermedio que no sirve nada sería
  una trampa para el usuario.
- **La promesa de la interfaz cambia.** «Nace con acceso a las vías que ya tengan una
  cuenta conectada» deja de ser cierta y hay que reescribirla, no dejarla mintiendo.

## Riesgos

- **Crear una aplicación pasa de un paso a dos**, y una aplicación recién creada no
  sirve nada hasta que se le marcan modelos. Es el coste de la opción elegida, y se
  mitiga con lo que ya existe: el aviso «esta aplicación no tiene ninguna vía
  concedida» que explica por qué el cliente ve la lista vacía. Ese aviso tiene que
  aparecer también en el caso nuevo.
- **Un cliente que cachea el catálogo** puede seguir ofreciendo un modelo que acaba
  de dejar de estar permitido. Nexo no lo puede evitar, pero el error de ejecución
  dirá exactamente qué pasa en lugar de fallar de forma opaca.
- **Marcar modelos uno a uno con 60 en pantalla se puede volver inusable**, y sería
  un problema nuevo creado al resolver este. Criterio 9.
- **Que el catálogo y el gateway se separen otra vez.** Ya están separados hoy: uno
  filtra por vía y el otro por vía y patrón. Si el arreglo no comparte una única
  función de decisión, volverá a pasar en el siguiente cambio. Criterio 4.
- **El catálogo cambia bajo los pies del usuario.** Si se desconecta un proveedor y se
  vuelve a conectar, sus modelos pueden cambiar de identificador y los marcados
  dejarían de existir. Hay que decidir si esas filas huérfanas se limpian o se
  conservan; se resolverá en el diseño.

## Invariantes que esto no puede romper

- **Nunca degradar en silencio** (2). Un modelo no permitido es un error explícito que
  nombra el modelo. Jamás se sustituye por otro que sí esté permitido.
- **Los límites por aplicación son obligatorios en las vías de suscripción** (4) y el
  [ADR 0001](../../docs/adr/0001-oauth-de-suscripcion.md). Elegir modelos no es una
  forma de saltarse el límite: sigue siendo por vía. Criterio 8.
- **El eje de credencial es de primer nivel** (5). Los modelos se marcan por vía, no
  por proveedor: el mismo modelo por dos vías son dos decisiones distintas.
- **Consultas de catálogo vacías dejan rastro con su motivo** (lo que resolvió la
  spec 0001 tras costar tres intentos de diagnóstico). El motivo nuevo —modelos
  marcados que ya no están en el catálogo— tiene que distinguirse de «nada concedido»
  y de «el catálogo del proveedor está vacío». Criterio 5.
