# 0009 · Diseño

## Enfoque

El nivel configurado vive como una columna nueva en `app_grants`, es decir **en
la fila del permiso**, que es exactamente la granularidad que se pidió: una fila
por modelo marcado. `grant_for` ya elige la fila que autoriza cada petición, así
que no hace falta ninguna lógica nueva de resolución — pero sí arreglar que hoy
elige la *primera* que coincide y no la *más específica*, algo inocuo mientras
todas las filas de una vía llevan los mismos valores y decisivo en cuanto una
lleva un valor propio. La aplicación del valor por defecto ocurre en un único
punto de `prepare_inner`, después de obtener el permiso, y solo cuando la
petición del cliente no trae nada. Los niveles que cada modelo admite dejan de
descartarse al parsear el catálogo de suscripción y viajan hasta la interfaz
dentro de `Capabilities`, que ya llega a la vista de permisos.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `crates/nexo-core/src/provider/mod.rs` | `Capabilities` gana `reasoning_levels: Vec<String>` con `#[serde(default)]`. `ReasoningEffort` gana `parse()` |
| `crates/nexo-core/src/provider/chatgpt_subscription.rs` | Deja de hacer `.map(\|a\| a.len())`: conserva los nombres de `supported_reasoning_levels` |
| `crates/nexo-core/src/db/migrations.rs` | `V3`: `ALTER TABLE app_grants ADD COLUMN reasoning_effort TEXT` |
| `crates/nexo-core/src/apps.rs` | `Grant` gana `reasoning_effort: Option<String>`; `grants()` lo lee; `replace_app_models` recibe el nivel por modelo; `set_grant` lo escribe |
| `crates/nexo-core/src/policy.rs` | `grant_for` prefiere la coincidencia más específica (exacta > prefijo > `*`) |
| `crates/nexo-core/src/service.rs` | `prepare_inner` aplica el defecto; `RouteModel` gana `configured_effort` |
| `src-tauri/src/commands.rs` | `set_app_models` recibe el nivel por modelo |
| `src/lib/api.ts` | `Capabilities.reasoning_levels`, `RouteModel.configured_effort`, forma de `setAppModels` |
| `src/lib/views/Apps.svelte` | Selector de nivel por modelo marcado |

## Decisiones

### D1. El nivel vive en la fila del permiso, no en una tabla nueva

- **Decisión:** columna `reasoning_effort TEXT` en `app_grants`, cuya clave
  primaria ya es `(app_id, provider_id, credential_kind, model_pattern)` — la
  granularidad exacta que pide la especificación, incluido el eje de credencial
  (invariante 5).
- **Alternativa descartada:** una tabla `app_model_settings` aparte, porque
  duplicaría la clave y obligaría a mantener dos sitios en sincronía al borrar
  una aplicación o retirar una vía — `ON DELETE CASCADE` y el borrado por vía de
  `replace_app_models` ya funcionan sobre `app_grants`.
- **Consecuencia que hay que asumir:** es el **primer valor por modelo** en esa
  tabla. El comentario de `apps.rs:338` («las capacidades son de la vía, no del
  modelo, así que se escriben iguales en todas sus filas») deja de ser cierto y
  hay que corregirlo, no solo el código.

### D2. `grant_for` pasa a preferir la coincidencia más específica

- **Decisión:** ordenar los candidatos por especificidad (coincidencia exacta
  primero, luego prefijo más largo, `*` al final) en lugar del `.find()` actual.
- **Alternativa descartada:** dejarlo como está y añadir un `ORDER BY` en
  `grants()`. Se descarta porque haría que la corrección dependa del orden que
  devuelva SQLite, que no está garantizado; y porque `grant_for` está
  documentado como «el único sitio donde se decide», así que la regla debe
  estar ahí y no repartida entre una consulta y una función.
- **Consecuencia que hay que asumir:** es un cambio de comportamiento que hoy no
  se observa (todas las filas de una vía llevan valores iguales) pero es un
  arreglo real: con un `*` heredado y un modelo marcado a la vez, la fila
  elegida era arbitraria. Lleva su propia prueba, y el test existente
  `grant_for_is_the_single_place_that_decides` no la cubría porque usa
  proveedores distintos, así que no hay expectativa previa que romper.

### D3. El defecto se aplica en `prepare_inner`, después del permiso, verificando el catálogo

- **Decisión:** en `prepare_inner`, tras `policy.check` (que hay que dejar de
  descartar: hoy es `self.policy.check(...)?;` y pasa a `let decision = …`), se
  inyecta `req.reasoning` **solo si** las tres cosas se cumplen: la petición del
  cliente no traía nivel, el permiso tiene uno configurado, y el modelo lo sigue
  declarando en `reasoning_levels` de su descriptor.
- **Alternativa descartada:** inyectarlo antes de `check_capabilities` para que
  pase por esa comprobación. Se descarta porque entonces un nivel obsoleto
  produciría un `422` al cliente por algo que el cliente no pidió: el fallo
  sería de la configuración de Nexo y se le echaría la culpa a la petición.
- **Consecuencia que hay que asumir:** el nivel inyectado no pasa por
  `check_capabilities`, así que la comprobación contra `reasoning_levels` de la
  propia inyección es lo único que impide mandar un nivel que el modelo no
  admite. Es el punto que la prueba del criterio 7 tiene que cubrir de verdad.

### D4. Un nivel configurado que el modelo ya no admite no se aplica, y eso no viola la invariante 2

- **Decisión:** si el catálogo deja de declarar ese nivel, no se inyecta; la
  petición sigue exactamente como si no hubiera nada configurado, y la interfaz
  lo marca como huérfano (mismo trato que un modelo marcado que desaparece del
  catálogo, `RouteModel.missing`).
- **Alternativa descartada:** rechazar la petición con `422`. Se descarta porque
  el cliente no pidió ese nivel: convertir una configuración obsoleta de Nexo en
  un fallo de la aplicación cliente es echar la culpa al sitio equivocado, y
  dejaría la aplicación sin servicio por un dato que el usuario no ha vuelto a
  tocar.
- **Consecuencia que hay que asumir:** parece un incumplimiento de la invariante
  2 y no lo es, y conviene tenerlo escrito: la invariante prohíbe eliminar en
  silencio **una capacidad que la petición solicitaba**. Aquí la petición no
  solicitaba nada, el resultado es idéntico al de no tener configuración, y el
  estado obsoleto se muestra en la interfaz. No hay nada silencioso.

### D5. El selector ofrece solo niveles que Nexo sabe enviar

- **Decisión:** los niveles se guardan tal cual llegan del proveedor (invariante
  6), pero el selector solo ofrece los que `ReasoningEffort::parse()` reconoce.
  Uno desconocido se muestra, sin poder elegirse, indicando que Nexo todavía no
  lo soporta.
- **Alternativa descartada:** permitir elegir cualquier nivel publicado,
  añadiendo `ReasoningEffort::Other(String)`. Se descarta por su coste real:
  `ReasoningEffort` es `Copy`, y una variante con `String` lo rompe y obliga a
  tocar todos sus usos para ganar una capacidad hipotética. La otra alternativa
  —cambiar `ChatRequest.reasoning` a `Option<String>`— pierde el tipado en la
  ruta más importante del producto.
- **Consecuencia que hay que asumir:** esto **corrige a la baja un supuesto de
  `spec.md`**, que decía que el usuario podría elegir un nivel nuevo antes de
  que Nexo lo conozca. No podrá elegirlo; lo verá y sabrá que existe. El coste
  de soportarlo pasa a ser una línea en el enum, visible y con nombre, en lugar
  de un valor que se manda sin que nadie lo haya probado. El alcance y el
  problema resuelto no cambian, así que no se vuelve a `/spec`.

### D6. `set_app_models` recibe el nivel junto a cada modelo, no en un mapa aparte

- **Decisión:** la lista de modelos pasa de `Vec<String>` a una lista de
  `(nombre, nivel)`. La interfaz ya reenvía la selección completa de la vía en
  cada cambio (`saveModels(app, route, [...current])`), así que mandar el nivel
  con cada entrada encaja con el flujo que ya hay.
- **Alternativa descartada:** una orden nueva `set_app_model_effort` que
  actualice una sola fila. Se descarta porque `replace_app_models` borra y
  reinserta todas las filas de la vía por contrato («no deja la selección
  anterior detrás»): con el nivel viviendo en esas filas, cualquier marcado de
  casilla borraría los niveles configurados salvo que se hiciera una
  lectura-fusión-escritura dentro de la transacción. Eso es pérdida silenciosa
  de datos esperando a que alguien añada un camino que no fusione.
- **Consecuencia que hay que asumir:** cambia la firma de una orden pública y de
  `replace_app_models`, con sus ~8 sitios de llamada en pruebas. Es trabajo
  mecánico guiado por el compilador, no riesgo.

## Qué puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| El nivel de un modelo se escribe en las filas de los demás (el fallo natural de D1, porque hasta hoy todas las filas se escribían iguales) | Prueba del criterio 2 con **dos** modelos y niveles distintos, comprobando que cada fila conserva el suyo. Sin dos modelos, la prueba pasaría con el fallo dentro |
| Marcar o desmarcar una casilla borra los niveles ya configurados de los otros modelos | Prueba que configura un nivel, marca otro modelo distinto y vuelve a leer el primero |
| El cambio de `grant_for` (D2) altera qué permiso gana en una combinación no prevista | Prueba nueva con `*` y modelo exacto en la **misma** vía, más los tests existentes de `policy.rs` en verde |
| Una base de datos existente no arranca tras la migración | Criterio 3, abriendo un esquema en versión 2 y aplicando migraciones |
| `supported_reasoning_levels` cambia de forma y el selector se queda vacío | Degrada a «sin especificar», que es el comportamiento de hoy. Se detecta porque desaparecerían los niveles de **todos** los modelos a la vez; la prueba del criterio 1 contra el fixture real falla si cambia la forma que conocemos |
| Un nivel obsoleto se manda al proveedor pese a D3/D4 | Criterio 7, con un descriptor cuyo `reasoning_levels` no contiene el nivel guardado, comprobando que la petición sale sin `reasoning_effort` |

## ¿Hace falta un ADR?

No. No cambia ninguna decisión de arquitectura: el eje de credencial, los
estados de contabilidad y el límite obligatorio de suscripción siguen intactos,
y la invariante 2 se respeta por construcción (D3, D4). El ADR 0001 se
menciona pero no se modifica: esta funcionalidad opera **dentro** de sus
mitigaciones, no las relaja.

Sí hay que corregir un **comentario** que pasa a ser falso: el de `apps.rs:338`
sobre que las capacidades son de la vía y se escriben iguales en todas las
filas. Un comentario que contradice al código es peor que no tenerlo.

## Qué queda pendiente de descubrir

- **Si el catálogo real sigue publicando `supported_reasoning_levels` con la
  forma del fixture.** El fixture se capturó al construir la vía de
  suscripción; la prueba del criterio 1 lo verifica contra el fixture, no
  contra la red. Solo una cuenta de suscripción real, en `/build`, dirá si
  el catálogo de hoy trae lo mismo — y con qué niveles por modelo.
- **Si el proveedor acepta de verdad cada nivel que declara.** Que el catálogo
  diga `xhigh` no demuestra que una petición con `reasoning_effort: "xhigh"`
  funcione. Comprobable solo con la cuenta real.
- **Si `minimal` aparece en algún modelo real.** Está en el enum de Nexo pero no
  en el fixture (`low`, `medium`, `high`, `xhigh`): puede que sea un nivel de la
  API pública y no de esta vía, en cuyo caso el selector simplemente no lo
  ofrecerá en ningún modelo de suscripción.
