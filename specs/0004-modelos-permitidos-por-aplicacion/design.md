# 0004 · Diseño

## Enfoque

El almacenamiento ya sirve: `app_grants` tiene `model_pattern` en su clave primaria,
así que un permiso puede ser varias filas —una por modelo marcado— sin tocar el
esquema. Lo que falta es una única función que decida si una aplicación puede usar un
modelo, y que la usen **los dos** sitios que hoy deciden por separado y distinto:
`PolicyEngine::check()` (que sí mira el patrón) y `build_models_for_app()` (que no).
Sobre eso, un comando que reemplaza de golpe el conjunto de modelos de una vía, y en
la interfaz una lista con buscador dentro de cada vía. El resto del cambio es
consecuencia: el alta de una aplicación deja de conceder nada y el catálogo vacío gana
un motivo nuevo.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `crates/nexo-core/src/policy.rs` | `grant_for()` público: la única función que decide. `check()` pasa a usarla |
| `crates/nexo-core/src/apps.rs` | `replace_app_models()`: reemplaza en una transacción los modelos de una vía, con el límite obligatorio incluido |
| `crates/nexo-core/src/service.rs` | `build_models_for_app()` usa `grant_for()`; `create_app_with_access()` deja de conceder; motivo nuevo en `models_for_app()`; `app_route_models()` para la interfaz |
| `src-tauri/src/commands.rs` | `set_app_models` sustituye a `set_app_access`; `app_route_models` nuevo |
| `src-tauri/src/main.rs` | Registro de los comandos |
| `src/lib/api.ts` | Tipos y envoltorios |
| `src/lib/views/Apps.svelte` | Lista de modelos por vía, con buscador y «marcar los visibles» |
| `docs/modelo-datos.md` | Qué significa ahora una fila de `app_grants`, y qué significa `*` |
| `docs/producto.md` | El permiso más fino ya no es la vía |

Sin migración: el esquema no cambia y los datos existentes siguen siendo válidos.

## Decisiones

### D1. Una sola función decide, y los dos caminos la llaman

- **Decisión:** `policy::grant_for(grants, provider_id, kind, public_model) ->
  Option<&Grant>` es el único sitio donde se decide si una aplicación puede usar un
  modelo. La usan `PolicyEngine::check()` y `build_models_for_app()`.
- **Alternativa descartada:** arreglar `build_models_for_app` añadiéndole la misma
  condición que tiene `check`. Descartada porque es exactamente cómo se llegó al
  desajuste actual: dos sitios con la misma regla escrita dos veces, y una se quedó
  atrás. Duplicarla otra vez garantiza que vuelva a pasar.
- **Consecuencia que hay que asumir:** `grant_for` recibe el nombre público del
  modelo, así que el catálogo tiene que resolverlo antes de preguntar. Ya lo tiene:
  `CatalogRow::public_name`.

### D2. Un modelo marcado es una fila; el conjunto se reemplaza entero

- **Decisión:** `replace_app_models(app_id, provider_id, kind, models, flags)` borra en
  una transacción las filas de esa vía e inserta una por modelo marcado. Marcar,
  desmarcar y «marcar los visibles» son la misma operación con distinto conjunto.
- **Alternativa descartada:** un comando por modelo. Descartada porque «marcar los 60
  visibles» serían 60 llamadas y 60 escrituras, con estados intermedios visibles si
  una falla a mitad. Reemplazar el conjunto es atómico y la interfaz no reconcilia nada.
- **Consecuencia que hay que asumir:** dos vistas abiertas sobre la misma aplicación se
  pisan, y gana la última en guardar. Es una aplicación de escritorio de un usuario y
  una ventana: no se añade control de concurrencia para un caso que no existe.

### D3. `*` se conserva y significa «todos los de esta vía»

- **Decisión:** una fila con `model_pattern = '*'` sigue dando acceso a todos los
  modelos de su vía; `model_matches` ya lo hace. La interfaz muestra esa vía como
  «todos», y al guardar una selección concreta la fila `*` se va con las demás.
- **Alternativa descartada:** migrar las filas `*` a filas concretas, una por modelo
  del catálogo actual. Descartada porque cambiaría sin avisar el comportamiento de las
  aplicaciones que hoy funcionan: dejarían de recibir los modelos que el proveedor
  añada en el futuro.
- **Consecuencia que hay que asumir:** hay dos maneras de expresar «todos» —la fila `*`
  heredada y marcar los 60 uno a uno— y no son equivalentes ante un modelo nuevo. La
  interfaz tiene que decir cuál está activa, no esconderlo.

### D4. Los motivos de catálogo vacío se separan, y esto corrige la especificación

- **Decisión:** cuatro motivos distinguibles: `no_grants` (nada concedido),
  `no_account` (hay permisos pero ninguna cuenta activa), `no_models_match` (hay
  modelos marcados pero ninguno existe hoy en el catálogo) y `empty_catalog`.
- **Alternativa descartada:** un motivo `no_models_selected` para «vía concedida sin
  modelos marcados», que es lo que pedía el criterio 5 de la especificación. **No se
  puede implementar, y la especificación se contradecía al pedirlo:** su propio
  supuesto dice que conceder una vía y marcar sus modelos son la misma acción, así que
  «concedida con cero modelos» no existe como estado —son cero filas, indistinguible
  de «no concedida»—. Lo que sí existe y merece motivo propio es el caso huérfano:
  filas marcadas que apuntan a modelos que ya no están en el catálogo. Se corrige el
  criterio 5 en `spec.md`.
- **Consecuencia que hay que asumir:** el mensaje de `no_grants` tiene que hablar de
  modelos y no solo de vías, porque ahora cubre los dos casos que el usuario percibe
  como distintos.

### D5. Las selecciones huérfanas se conservan y se muestran, no se limpian

- **Decisión:** si un modelo marcado desaparece del catálogo, su fila se queda. La
  interfaz la muestra marcada y señalada como «ya no está en el catálogo», y el motivo
  `no_models_match` la hace diagnosticable desde el panel.
- **Alternativa descartada:** borrarlas al refrescar el catálogo. Descartada porque son
  intención declarada del usuario, y un proveedor que se cae o devuelve un catálogo
  incompleto durante un minuto borraría permisos para siempre. Perder intención del
  usuario por un fallo transitorio del proveedor no es aceptable.
- **Consecuencia que hay que asumir:** con el tiempo pueden acumularse filas de modelos
  que ya no existen. Se ven y se pueden desmarcar, y no afectan al funcionamiento
  porque `grant_for` solo dice sí a lo que además está en el catálogo.

### D6. El alta de una aplicación deja de conceder, y el texto cambia

- **Decisión:** `create_app_with_access` deja de recorrer las cuentas concediendo vías.
  Se renombra a `create_app`, porque el nombre describía justo lo que deja de hacer. El
  texto de la interfaz pasa a decir que hay que elegir modelos, y el aviso de «sin vía
  concedida» que ya existe cubre el estado recién creado.
- **Alternativa descartada:** conceder todas las vías con `*` al crear, y aplicar el
  «empezar con ninguno» solo a las vías añadidas después. Descartada por incoherente:
  el valor por defecto sería distinto según cómo llegaste a él.
- **Consecuencia que hay que asumir:** crear una aplicación y ponerla a funcionar son
  dos pasos. Es lo que el usuario eligió sabiéndolo, y alinea el comportamiento con lo
  que `policy.rs` ya declaraba: «el acceso se concede, no se deniega».

### D7. Las capacidades siguen siendo por vía, aunque se guarden por fila

- **Decisión:** `allow_tools`, `allow_multimodal` y `log_content` se escriben iguales en
  todas las filas de una vía y se leen de la fila que haya casado. La interfaz los
  presenta una vez por vía.
- **Alternativa descartada:** llevarlas a su propia tabla por vía, que es donde
  conceptualmente van. Descartada por ahora: exige migración y no resuelve nada de lo
  pedido. Queda como deuda anotada.
- **Consecuencia que hay que asumir:** una desnormalización real. Si alguien escribiera
  filas de la misma vía con capacidades distintas, ganaría la primera que case, que es
  arbitrario. Ningún camino del código lo permite, pero la base de datos sí.

## Qué puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| Que el catálogo y el gateway vuelvan a discrepar | Prueba que recorre el catálogo de una aplicación y exige que cada modelo listado pase `grant_for`, y que ninguno de los no listados pase (criterio 4) |
| Que una aplicación existente con `*` pierda acceso al actualizar | Prueba que crea el permiso con `*` y comprueba que salen todos los modelos de la vía (criterio 6) |
| Que el límite obligatorio de suscripción se pierda por el camino nuevo | Prueba: marcar un solo modelo de la vía de suscripción crea el límite (criterio 8) |
| Que desmarcar el último modelo deje un estado intermedio que no sirve nada y no se explique | Prueba: el conjunto vacío borra las filas y el motivo registrado es `no_grants` |
| Que un nombre de modelo con `*` o `/` rompa la comparación | Pruebas de `model_matches` con un nombre que contenga `*` literal y con nombres con `/` |
| Que la interfaz mande el `api_id` en lugar del nombre público | Prueba de extremo a extremo: se marca por nombre público y la petición usa ese mismo nombre |

## ¿Hace falta un ADR?

No. Refuerza el ADR 0001 en lugar de tocarlo: el límite obligatorio por aplicación
sigue siendo por vía y no se relaja al poder elegir modelos. Sí hay que actualizar
`docs/modelo-datos.md`, porque el significado de una fila de `app_grants` cambia —de
«una vía concedida» a «un modelo concedido de una vía»— y eso es un contrato.

## Qué queda pendiente de descubrir

- **Si con 60 modelos la lista es usable de verdad.** El buscador y «marcar los
  visibles» son la apuesta; solo se sabrá con la vía real de Zen delante.
- **Si conviene ordenar por algo más útil que el nombre** (los marcados primero, o los
  gratuitos primero). Se verá con la lista real.
- **Si el aviso de «esta aplicación no sirve nada» se lee a tiempo** en el flujo nuevo,
  o si hace falta llevar al usuario a elegir modelos justo después de crear la
  aplicación.
