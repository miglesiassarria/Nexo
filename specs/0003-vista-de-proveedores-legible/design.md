# 0003 · Diseño

## Enfoque

La vista deja de decidir. Hoy `Providers.svelte` agrupa cuentas por tipo de
credencial, ordena por fecha de creación y lleva escrita a mano la lista de tipos de
proveedor: tres decisiones que son del dominio, no de la presentación, y que por eso
se equivocan (Zen duplicado bajo OpenAI). El núcleo gana dos consultas de lectura
—`provider_rows()`, que devuelve la lista ya compuesta y ordenada, y
`connect_options()`, que declara qué se puede añadir y con qué forma de
formulario— y la vista pasa a renderizar filas plegables y un panel de alta sin saber
qué proveedores existen. Como la lógica queda en Rust, los criterios 1, 5 y 6 se
prueban con `cargo test` sin añadir infraestructura de pruebas a la interfaz.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `crates/nexo-core/src/service.rs` | Nuevos `ProviderRow`, `ConnectOption`, `ConnectForm`; métodos `provider_rows()` y `connect_options()`; sus pruebas |
| `src-tauri/src/commands.rs` | Dos comandos de lectura: `provider_rows`, `connect_options` |
| `src-tauri/src/main.rs` | Registro de los dos comandos |
| `src/lib/api.ts` | Tipos `ProviderRow`, `ConnectOption`, `ConnectForm` y sus envoltorios |
| `src/lib/views/Providers.svelte` | Reescritura: lista plegable + panel de alta por forma de formulario |
| `docs/producto.md` | Un párrafo sobre cómo se ve la pestaña, si el texto actual queda desfasado |

Nada de esto toca el modelo de datos, el gateway ni los adaptadores. `provider_presets()`
queda absorbido por `connect_options()` y se retira, porque tener dos comandos que
describen lo mismo es la forma de que uno se quede atrás.

## Decisiones

### D1. La lista de filas se compone y se ordena en Rust

- **Decisión:** un método `Nexo::provider_rows()` que cruza `db.accounts()` con los
  recuentos del catálogo y devuelve `Vec<ProviderRow>` ya ordenado. La vista hace
  `{#each rows}` y nada más.

  ```rust
  pub struct ProviderRow {
      pub account_id: String,
      pub provider_id: String,
      pub credential_kind: String,
      /// Etiqueta de la cuenta: «ChatGPT (correo)», «OpenCode Zen», «LM Studio (url)».
      pub name: String,
      pub status: String,
      /// Cuántos modelos ofrece esta pareja proveedor+credencial, del catálogo ya guardado.
      pub models: usize,
      /// Dirección del servidor cuando la vía la tiene y se puede cambiar.
      pub address: Option<String>,
      pub editable_address: bool,
      pub expires_at: Option<i64>,
      pub created_at: i64,
      /// Si esta fila exige atención del usuario. Decide el orden y el aviso.
      pub needs_attention: bool,
      /// Nota propia de esta vía, si tiene alguna que el usuario deba saber.
      pub note: Option<String>,
  }
  ```

- **Alternativa descartada:** componerlo en la vista con `$derived` sobre `accounts` y
  `grantableRoutes()`, y añadir Vitest para poder probar la función de agrupación.
  Descartada por dos motivos. Uno, la arquitectura declarada del proyecto dice que la
  interfaz «solo orquesta y presenta»: agrupar por eje de credencial y decidir qué
  exige atención es dominio. Dos, obligaría a meter un corredor de pruebas nuevo en
  `package.json` para cubrir una lógica que en Rust ya se prueba gratis, y el fallo que
  motiva esta especificación es precisamente de una decisión de dominio tomada en la
  vista.
- **Consecuencia que hay que asumir:** un texto de interfaz (la nota, la etiqueta) vive
  en Rust. Ya pasa con los mensajes de error, que el proyecto exige en español y con
  instrucciones para el usuario, así que no es un patrón nuevo; pero significa que
  cambiar esa nota obliga a recompilar el núcleo.

### D2. El orden pone delante lo que exige actuar, y esto corrige la especificación

- **Decisión:** primero las filas con `needs_attention` (estado `broken` o `expired`),
  después las activas; dentro de cada grupo, por nombre. `needs_attention` lo decide el
  núcleo, no la vista, porque «qué cuenta está mal» es dominio.
- **Alternativa descartada:** lo activo primero. Descartada porque el riesgo que la
  propia especificación identifica es que plegar esconda una vía rota: ponerla al final
  es exactamente el fallo que se quería evitar.
- **Consecuencia que hay que asumir:** la especificación se contradecía. En «Supuestos
  asumidos» decía «primero lo que funciona, después lo roto o caducado, porque eso es
  lo que exige actuar» —la razón justifica lo contrario de lo que afirma— y en
  «Riesgos» decía «lo roto o caducado se ordena arriba». Se corrige el supuesto en
  `spec.md` y se deja anotado aquí, en lugar de elegir en silencio.

### D3. La vista conoce formas de formulario, no proveedores

- **Decisión:** `connect_options()` devuelve qué se puede añadir, y cada opción declara
  su **forma de formulario**. La vista tiene una rama por forma, no por proveedor.

  ```rust
  pub struct ConnectOption {
      /// Identificador estable, del estilo «openai:subscription_oauth».
      pub id: String,
      pub name: String,
      pub summary: String,
      pub form: ConnectForm,
      pub note: Option<String>,
      /// Si ya hay cuenta conectada por esta vía. Se ofrece igual, pero avisando.
      pub already_connected: bool,
  }

  #[derive(Serialize)]
  #[serde(tag = "kind", rename_all = "snake_case")]
  pub enum ConnectForm {
      /// Aviso de riesgo obligatorio y login en el navegador.
      SubscriptionOauth,
      /// Un servidor en la máquina del usuario: solo dirección.
      LocalServer { default_url: String },
      /// Clave y etiqueta opcional, contra un proveedor conocido.
      ApiKey,
      /// Nombre, dirección y clave. Con los dos primeros rellenos cuando es un atajo.
      CompatEndpoint { suggested_name: String, base_url: String },
  }
  ```

  Con esto, Ollama entra como `LocalServer`, Anthropic por clave como `ApiKey` y Gemini
  por OAuth como `SubscriptionOauth`: ninguno toca la vista. Es lo que pide el criterio 8.
- **Alternativa descartada:** que el núcleo devuelva una descripción genérica de campos
  (nombre, tipo, obligatorio) y la vista construya el formulario por reflexión.
  Descartada porque los flujos no se distinguen por sus campos sino por lo que pasa
  alrededor: el de suscripción exige aceptar un aviso y esperar un callback del
  navegador, el local se comprueba antes de guardar. Un constructor genérico de campos
  no expresa eso, y acabaría con condicionales por proveedor dentro de la vista —el
  problema que se quería resolver, escondido.
- **Consecuencia que hay que asumir:** el criterio 8 se cumple para proveedores que
  encajen en una de las cuatro formas. Un proveedor con un flujo de alta genuinamente
  distinto —un QR, un fichero de credenciales— exigirá una forma nueva y, con ella, una
  rama nueva en la vista. Es honesto decirlo: lo que se elimina es la sección por
  proveedor, no la posibilidad de que un flujo nuevo necesite interfaz nueva.

### D4. El recuento de modelos sale del catálogo guardado, no se pregunta al proveedor

- **Decisión:** `provider_rows()` cuenta filas de `catalog_rows()` por proveedor y
  credencial, igual que ya hace `grantable_routes()`. Es una consulta local: abrir la
  pestaña no genera tráfico de red.
- **Alternativa descartada:** consultar el catálogo a cada proveedor al abrir la vista,
  para tener el dato fresco. Descartada porque convierte abrir una pestaña en una
  ronda de peticiones a servicios de pago, y porque el catálogo ya se refresca al
  conectar y bajo demanda. Los health checks por proveedor están en el ROADMAP y son
  otro trabajo.
- **Consecuencia que hay que asumir:** si el catálogo está viejo, el recuento está
  viejo. Aceptable: es el mismo número que usa la pestaña de permisos.

### D5. Una fila desplegada a la vez, con el patrón que ya existe

- **Decisión:** `let expanded = $state<string | null>(null)` y comparación por
  `account_id`, copiando literalmente lo que hace `Apps.svelte` con los permisos.
- **Alternativa descartada:** varias abiertas a la vez, o `<details>` nativo.
  Descartada porque el problema de partida es que todo está abierto, y porque un patrón
  distinto al de la pestaña de al lado es incoherencia gratuita.
- **Consecuencia que hay que asumir:** comparar dos proveedores obliga a plegar y
  desplegar. El dato que se compara —modelos y estado— ya está en la fila plegada.

### D6. Solo hay fila para lo que tiene cuenta

- **Decisión:** las filas nacen de `db.accounts()`. Una vía que está en el catálogo pero
  no tiene cuenta (el proveedor mock, o una vía cuyo proveedor se desconectó) no aparece
  en la lista de conectados; aparece como opción en el panel de alta.
- **Alternativa descartada:** listar todas las vías del catálogo y marcar las no
  conectadas. Descartada porque mezcla otra vez las dos preguntas que esta
  especificación separa: qué tengo y qué puedo añadir.
- **Consecuencia que hay que asumir:** el proveedor mock deja de ser visible en
  Proveedores. Hoy tampoco lo es, porque no tiene cuenta, así que no cambia nada
  observable.

## Qué puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| Perder una acción que hoy existe (guardar dirección de LM Studio, comprobar ahora, editar la URL de un proveedor propio) | Criterio 9: inventario comprobado uno a uno en la aplicación instalada. Es el fallo que ya ocurrió con la lista de vías escrita a mano |
| Que el nombre mostrado deje de identificar la cuenta cuando hay dos del mismo proveedor | Prueba de `provider_rows()` con dos cuentas de proveedores propios distintos: nombres distintos y una fila cada una |
| Que `needs_attention` no se marque para un estado nuevo que aparezca en el futuro | La prueba enumera los estados conocidos (`active`, `broken`, `expired`) y exige que cualquier estado no reconocido cuente como que exige atención, no lo contrario |
| Saltarse el aviso de riesgo de ChatGPT al reorganizar el flujo de alta | Criterio 7 en la aplicación instalada: el botón de login sigue deshabilitado hasta marcar la casilla. El núcleo además ya guarda `risk_ack_at` |
| Que `connect_options()` y los comandos de alta se desincronicen (una opción que no se puede completar) | Prueba que recorre las opciones y comprueba que cada `form` tiene su comando de alta correspondiente registrado |
| Dejar de mostrar una cuenta en estado `broken` porque la vista solo pinta las activas | La prueba de orden usa una cuenta `broken` y exige que salga primera |

## ¿Hace falta un ADR?

No. No cambia ninguna decisión de arquitectura: refuerza la que ya está escrita —el
dominio vive en Rust y la interfaz presenta— y respeta el ADR 0001, cuyo aviso previo
al primer login sigue siendo obligatorio (criterio 7). Sí conviene, al cerrar, revisar
si `docs/producto.md` describe la pestaña de una forma que ya no será cierta.

## Lo que se corrigió al construir

Tres cosas que este diseño no tenía bien. Se anotan porque un documento que
contradice al código es peor que no tener documento.

- **`editable_address: bool` no bastaba: la fila necesita declarar cómo se gestiona.**
  Al escribir la vista quedó claro que desconectar un proveedor añadido por el usuario
  **no** es `disconnect_account`: eso borra la cuenta y deja su definición huérfana en
  `custom_providers`, donde reaparecería sin cuenta. Y cambiar la dirección es
  `update_custom_provider_url` para un proveedor propio y `set_lmstudio_url` para el
  servidor local: dos comandos distintos. `editable_address` se sustituye por
  `RowManage { Account | LocalServer | CustomProvider }`, que dice de qué comandos
  dispone cada fila. Cubierto por `each_row_declares_how_it_is_managed`.
- **El atajo no debe fijar sus campos.** `ConnectForm::CompatEndpoint` llevaba un
  `editable: bool` para que el atajo de Zen bloqueara nombre y dirección. Es una
  restricción sin ganancia: si Zen cambia su dirección, el usuario no podría
  corregirla, y el valor del atajo es que viene relleno, no que esté cerrado. El campo
  se elimina.
- **La nota de una vía tenía dos orígenes.** El aviso de los 14 segundos de LM Studio
  quedó escrito a la vez en `connect_options()` y a mano en el detalle de la fila, que
  es la manera de que uno de los dos se quede atrás. Ahora los dos salen de
  `route_note(provider_id, kind)`, y `ProviderRow` lleva su nota. Cubierto por
  `the_note_of_a_route_has_a_single_source`.

## Deuda que este diseño deja, y conviene no olvidar

Los comandos de alta y de comprobación siguen siendo específicos de un proveedor
aunque la forma del formulario se comparta: `connect_chatgpt` para
`SubscriptionOauth`, y `set_lmstudio_url` / `detect_lmstudio` para `LocalServer`. Hoy
no se nota porque hay un solo proveedor de cada forma, pero el día que entre Ollama
—que es otro `LocalServer`— la vista llamaría a la comprobación de LM Studio. Está
anotado en el código, en `connectSubscription`. Es la parte del criterio 8 que se
cumple a medias, y decirlo es más útil que aparentar que no existe.

## Qué queda pendiente de descubrir

- **Si la fila de una línea aguanta el ancho real.** «OpenCode Zen · API key · Activa ·
  60 modelos» cabe; «LM Studio (http://127.0.0.1:1234) · Local · Servidor apagado · 3
  modelos» es más larga. Se verá en la aplicación instalada y puede obligar a acortar
  la etiqueta o a mover la dirección al detalle.
- **Si el recuento de modelos de la vía de suscripción resulta confuso** cuando el
  catálogo descubierto y el manifiesto local difieren. Se verá contra los datos reales
  de la máquina.
- **Si «Añadir proveedor» debe ofrecer una vía ya conectada.** Se devuelve
  `already_connected` para poder decidirlo con el caso delante en lugar de adivinarlo
  ahora: para LM Studio o un proveedor propio tiene sentido añadir otro, para la
  suscripción de ChatGPT probablemente no.
