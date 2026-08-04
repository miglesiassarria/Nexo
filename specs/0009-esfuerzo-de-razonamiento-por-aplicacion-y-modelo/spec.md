# 0009 · Esfuerzo de razonamiento por aplicación y modelo

- **Estado:** build
- **Creada:** 2026-08-03
- **Pedida por:** el usuario: «me gustaría poder configurar a nivel de aplicación
  para openai · Suscripción el nivel de esfuerzo para cada modelo y así disponer
  de esa flexibilidad»

## Problema

El nivel de esfuerzo de razonamiento solo se puede elegir hoy **desde la
aplicación cliente**, mandando `reasoning_effort` en cada petición. Nexo lo
recibe, lo traduce y lo reenvía ([wire.rs:125](../../crates/nexo-core/src/gateway/wire.rs)),
pero no tiene ninguna opinión propia sobre él.

El problema es que **la mayoría de clientes no exponen ese ajuste**. Msty, por
ejemplo, elige un modelo y manda la petición; si no incluye `reasoning_effort`,
el proveedor aplica su valor por defecto y el usuario no tiene ningún sitio
donde cambiarlo. Quien paga la suscripción no puede decidir que una aplicación
concreta razone poco (rápido y barato en cuota) y otra razone mucho, aunque
usen el mismo modelo por la misma vía.

Duele especialmente en la vía de suscripción, que es la que reparte **una única
cuota personal** entre todas las aplicaciones (ADR 0001, riesgo 3): el esfuerzo
de razonamiento es justo la palanca que más afecta a cuánta de esa cuota se
consume por petición, y es la que hoy no se puede tocar desde Nexo.

Hay además un dato que se está perdiendo: el catálogo real de la vía de
suscripción publica `supported_reasoning_levels` por modelo, con los nombres de
los niveles admitidos, y
[chatgpt_subscription.rs:254](../../crates/nexo-core/src/provider/chatgpt_subscription.rs)
hace `.map(|a| a.len())` y se queda solo con el número. Hoy Nexo sabe *si* un
modelo razona, no *qué niveles acepta* — así que ni podría ofrecer una lista
honesta si quisiera.

## Comportamiento esperado

- En los permisos de una aplicación, en la vía «openai · Suscripción», cada
  modelo marcado que admita razonamiento muestra un selector de nivel de
  esfuerzo.
- El selector ofrece **solo los niveles que ese modelo declara admitir**, según
  el catálogo real del proveedor. Un modelo sin razonamiento no muestra
  selector.
- Hay una opción explícita «sin especificar» (el estado por defecto), que es
  lo mismo que hay hoy: Nexo no manda nada y decide el proveedor.
- Lo configurado es un **valor por defecto, no una imposición**: se aplica solo
  cuando la petición del cliente no trae `reasoning_effort`. Si el cliente lo
  manda, gana el cliente, siempre.
- El resto de vías (API key de OpenAI, Gemini, Zen, OpenRouter, LM Studio) no
  cambian en nada: el selector no aparece donde no se conocen los niveles.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | El catálogo de la vía de suscripción conserva los **nombres** de los niveles admitidos por modelo, no solo su número | `cargo test -p nexo-core -- reasoning_levels` contra el fixture real ya existente en `chatgpt_subscription.rs` (`low`, `medium`, `high`, `xhigh`) |
| 2 | Un nivel configurado para una pareja aplicación+modelo se guarda y se recupera igual | `cargo test -p nexo-core -- reasoning_effort_roundtrip` |
| 3 | Una base de datos creada **antes** de esta especificación se abre sin error y sin nivel configurado en ningún permiso | `cargo test -p nexo-core -- migration_v3` (abre un esquema en versión 2, aplica migraciones, comprueba que los permisos existentes siguen y con nivel nulo) |
| 4 | Si la petición del cliente **no** trae `reasoning_effort`, se aplica el nivel configurado | `cargo test -p nexo-core --test gateway_e2e -- configured_effort_is_used_when_the_client_sends_none` |
| 5 | Si la petición del cliente **sí** trae `reasoning_effort`, gana el del cliente y el configurado se ignora | `cargo test -p nexo-core --test gateway_e2e -- the_client_wins_over_the_configured_effort` |
| 6 | Sin nada configurado, el comportamiento es **idéntico** al de hoy: no se añade `reasoning_effort` a la petición al proveedor | `cargo test -p nexo-core --test gateway_e2e -- no_configured_effort_changes_nothing` |
| 7 | Un nivel configurado que el modelo deja de admitir tras refrescar el catálogo **se conserva y se muestra como huérfano**, no se borra en silencio ni se manda al proveedor | `cargo test -p nexo-core -- an_effort_no_longer_supported_is_kept_and_flagged` |
| 8 | La interfaz ofrece solo los niveles declarados por el modelo, y «sin especificar» siempre | `npm run check` en verde + comprobación en la aplicación instalada con una cuenta de suscripción real |

## Fuera de alcance

- **Forzar o limitar el esfuerzo** (que Nexo sobrescriba o rechace lo que pide
  el cliente). Decidido explícitamente con el usuario: sobrescribir hacia abajo
  degradaría en silencio una capacidad solicitada, lo que prohíbe la
  invariante 2, y el único equivalente compatible (rechazar con `422`, como ya
  hacen `allow_tools` y `allow_multimodal` en
  [policy.rs:126](../../crates/nexo-core/src/policy.rs)) sería una protección de
  cuota, no la flexibilidad que se pidió. Si algún día se quiere el techo, es
  otra especificación con su propio criterio.
- **Presupuesto numérico de razonamiento** (`budget_tokens` / `thinking_budget`).
  `models.dev` lo publica para algunos modelos como
  `reasoning_options: [{"type":"budget_tokens","min":0,"max":24576}]`, y es una
  forma distinta —un entero, no un nivel— con su propia traducción por
  proveedor. Otra especificación.
- **El selector en las demás vías.** El mecanismo de almacenamiento y de
  aplicación es genérico (no cuesta más hacerlo así), pero solo la vía de
  suscripción publica hoy los niveles reales por modelo, así que solo ahí
  aparece el control. Extenderlo a Gemini (`thinking_level` vía `extra_body`,
  ya fuera de alcance en la spec 0008) o a lo que `models.dev` declare para
  otros proveedores es trabajo aparte.
- **Un nivel por defecto global de Nexo** (para todas las aplicaciones a la
  vez). Se pidió por aplicación; un global es fácil de añadir después y difícil
  de quitar si resulta que nadie lo usa.

## Supuestos asumidos

- **El esfuerzo por modelo solo aplica a modelos marcados uno a uno.** Un
  permiso con comodín (`*`) cubre toda la vía en una sola fila, incluidos los
  modelos que el proveedor añada mañana, así que no hay dónde colgar un valor
  por modelo: tendrá un único nivel para toda la vía. Declarado por mí, no
  preguntado.
- «Sin especificar» se representa como ausencia de valor (nulo), no como un
  nivel más del enum: así una base de datos vieja y un permiso nuevo sin tocar
  significan exactamente lo mismo, y el criterio 6 puede comprobar que no se
  manda nada.
- El nivel se guarda por (aplicación, proveedor, vía, modelo), que es
  precisamente la clave primaria que ya tiene `app_grants` — no hace falta una
  tabla nueva, solo una columna.
- Los niveles que el proveedor publica se guardan tal cual llegan (cadenas),
  sin filtrarlos por el enum `ReasoningEffort` de Nexo: conservar el dato
  original manda (invariante 6).

  **Corregido en `/design` (D5):** este supuesto decía además que el usuario
  podría *elegir* un nivel que Nexo todavía no conozca. No podrá: el selector
  solo ofrece los que Nexo sabe enviar, y uno desconocido se muestra sin poder
  elegirse. El motivo es el coste real de lo contrario — `ReasoningEffort` es
  `Copy` y una variante con `String` lo rompe, y cambiar
  `ChatRequest.reasoning` a `Option<String>` pierde el tipado en la ruta más
  importante del producto. El dato se sigue conservando y mostrando; lo que se
  retira es la promesa de poder usarlo sin que nadie lo haya probado.

## Riesgos

- **La forma de `supported_reasoning_levels` puede cambiar sin aviso.** Es la
  vía frágil del ADR 0001 (riesgo 1). Si cambia, el selector se quedaría vacío
  y habría que volver a `sin especificar` — que es exactamente el
  comportamiento de hoy, así que degrada a lo actual, no a algo roto. Se
  detecta porque el catálogo dejaría de traer niveles para todos los modelos a
  la vez.
- **Este es el primer valor por modelo en `app_grants`.** El comentario de
  [apps.rs:338](../../crates/nexo-core/src/apps.rs) dice literalmente «las
  capacidades son de la vía, no del modelo, así que se escriben iguales en
  todas sus filas», y `replace_app_models` escribe hoy los mismos valores en
  todas las filas de una vía. Romper ese supuesto es el cambio estructural de
  esta especificación, y el sitio donde es más fácil introducir un fallo:
  guardar el nivel de un modelo en las filas de los demás.
- **Un nivel más alto consume más cuota.** La funcionalidad hace más fácil
  gastar la cuota de la suscripción sin darse cuenta. No es un fallo, es el
  objetivo, pero el límite obligatorio por aplicación (invariante 4) sigue
  siendo la protección — y esta funcionalidad no lo relaja.

## Lo que se descubrió al construir

### 1. El catálogo de suscripción guardado hoy no tiene niveles: es el manifiesto

Con la aplicación instalada y la cuenta de suscripción real conectada, los tres
modelos de esa vía (`openai/gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`) están en la
base de datos con `reasoning_levels: []`. Y con `reasoning: true`, que **es
imposible con el código nuevo** (ahora la capacidad se deriva de la lista).

La explicación, comprobada: son exactamente los tres modelos del manifiesto
local (`catalog/mod.rs:49`) con las capacidades de `caps_full()`, escritos al
arrancar. El descubrimiento real del catálogo de esa vía no ha sustituido esas
filas.

**Consecuencia práctica: el selector no aparecerá en «openai · Suscripción»
hasta que el catálogo de esa vía se vuelva a descubrir contra el proveedor con
esta versión** — y solo si el proveedor publica `supported_reasoning_levels`
para esos modelos, algo que el fixture confirma para `gpt-5.6-sol` pero que no
se ha podido confirmar para los modelos que esta cuenta ve hoy.

No es un fallo introducido aquí: el estado del catálogo es anterior. Pero
invalida la parte del criterio 8 que dependía de datos reales.

### 2. No se pudo diagnosticar más lejos sin la interfaz

Se intentó arrancar el binario compilado con `NEXO_DATA_DIR` apuntando a una
copia de la base de datos real y un puerto distinto (8799), para forzar un
refresco de catálogo y leer los niveles reales. El proceso arrancó y cargó
`models.dev` desde caché, pero `refresh_catalog_from_providers` **no dejó
ninguna traza** ni de éxito ni de error en ~100 s, y las filas quedaron con el
manifiesto.

La hipótesis más probable es que `resolve_credential` se queda esperando el
acceso al almacén de claves del sistema, que sin interfaz gráfica no se puede
conceder. Es una limitación del método de diagnóstico, no un defecto del
producto — pero significa que **este criterio necesita una comprobación con la
ventana abierta**: pulsar «Refrescar catálogo» en Proveedores y volver a mirar.

## Invariantes que esto no puede romper

- **2. Nunca degradar en silencio.** Es la invariante que decide el diseño
  entero: el valor configurado es un defecto, jamás sobrescribe lo que el
  cliente pidió. Criterio 5.
- **3. Cuatro estados de contabilidad.** Sin cambios: el esfuerzo afecta a
  cuántos tokens de razonamiento se gastan, que ya se registran
  (`reasoning_tokens`), y la vía sigue siendo `subscription` con cuota
  desconocida.
- **4. Los límites por aplicación son obligatorios en las vías de
  suscripción.** Esta funcionalidad no los toca ni los relaja.
- **5. El eje de credencial es de primer nivel.** El nivel se guarda por
  proveedor **y** vía, no solo por modelo: el mismo modelo por dos vías puede
  tener niveles distintos, y admitir niveles distintos.
- **6. Se conserva el dato original del proveedor.** Es la razón de dejar de
  tirar los nombres de `supported_reasoning_levels`, y de guardarlos como
  cadenas sin filtrarlos por el enum interno.
