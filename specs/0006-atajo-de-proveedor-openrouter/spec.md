# 0006 · Atajo de proveedor: OpenRouter

- **Estado:** build
- **Creada:** 2026-08-02
- **Pedida por:** el usuario, tras terminar la spec 0005: «ahora quiero añadir el
  proveedor de open router dejandolo listo solo para ponerle nombre y el api key»

## Problema

OpenRouter ya se puede conectar hoy, pero solo por el camino genérico «Otro
servicio OpenAI-compatible»: hay que teclear a mano el nombre y la URL base
exacta (`https://openrouter.ai/api/v1`). Un carácter mal puesto en esa URL no
solo rompe la conexión, sino que además desalinea el cruce con `models.dev`
(que compara la URL declarada byte a byte), dejando el catálogo sin precios ni
capacidades aunque la conexión funcione. OpenCode Zen no tiene este problema
porque ya es un atajo con la dirección precargada; OpenRouter, siendo un
proveedor igual de conocido y con 336 modelos en `models.dev`, no lo es.

## Comportamiento esperado

- En «Añadir proveedor», junto al atajo de OpenCode Zen, aparece uno para
  OpenRouter con el nombre y la URL base ya puestos.
- El usuario solo pega su API key (y, si quiere, cambia el nombre) para
  conectarlo — igual que ya ocurre con Zen.
- El proveedor que se crea es un OpenAI-compatible más: mismo adaptador, mismas
  estadísticas, mismos permisos por modelo, sin código específico de
  OpenRouter en ninguna parte.
- El catálogo de ese proveedor cruza correctamente con `models.dev` desde la
  primera conexión, con precios y capacidades reales, no solo como texto sin
  enriquecer.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | `connect_options()` ofrece un atajo «OpenRouter» con `suggested_name = "OpenRouter"` y `base_url = "https://openrouter.ai/api/v1"` | `cargo test -p nexo-core -- openrouter_shortcut` |
| 2 | El atajo de OpenRouter convive con el de Zen; ambos siguen distinguibles y sin identificadores repetidos | `cargo test -p nexo-core -- connect_options_cover_the_four_form_shapes` (test ya existente, no debe romperse) |
| 3 | La URL del preset coincide exactamente (recortando solo la barra final) con la que `models.dev` declara para `openrouter`, así que `provider_id_for_api` la reconoce | Comprobado contra `https://models.dev/api.json` real antes de escribir el valor; queda anotado en `design.md` para que quien lo lea no tenga que volver a comprobarlo |
| 4 | El texto de la opción genérica «Otro servicio OpenAI-compatible» ya no cita a OpenRouter como ejemplo de servicio sin atajo propio | Lectura de `service.rs`; `cargo test -p nexo-core -- the_generic_compatible_option` sigue en verde |
| 5 | Conectar el atajo, pegar una API key real y refrescar el catálogo deja modelos de OpenRouter con precio y capacidades, no solo como texto sin enriquecer | **Verificado con clave real**: `cargo test -p nexo-core --test gateway_e2e -- --ignored openrouter`, contra la API real y el modelo gratuito `poolside/laguna-s-2.1:free` |

## Fuera de alcance

- **Cabeceras opcionales de OpenRouter** (`HTTP-Referer`, `X-Title`, que
  OpenRouter usa para atribuir uso en su propio panel). El adaptador genérico
  no las manda hoy para ningún proveedor y añadirlas sería una capacidad nueva
  del adaptador, no un atajo — se deja para otra especificación si hace falta.
- **Cualquier atajo adicional** (Groq, DeepSeek, Mistral, xAI) que `design.md`
  de la spec 0002 también anticipó. Se pidió solo OpenRouter; los demás se
  añaden igual de barato el día que se pidan, uno a uno.
- **Arreglar la condición de carrera de `models.dev` en el arranque real**
  (ver «Lo que se descubrió al construir» más abajo). Es un fallo del
  producto, no de este atajo; se trata aparte, con su propia prueba de
  reproducción, según exige `CLAUDE.md` para arreglos de fallo.

## Supuestos asumidos

- `docs_url` del preset apunta a `https://openrouter.ai/models` (el valor
  `doc` que declara `models.dev`, mismo criterio que se usaría para cualquier
  otro atajo).
- El ejemplo que se retira del texto genérico (criterio 4) se sustituye por
  «Groq» — uno de los otros dos que `design.md` 0002 ya citaba junto a
  OpenRouter, para no dejar ese texto sin ningún ejemplo.
- El identificador de cuenta que genera este atajo (`util::slugify("OpenRouter")`)
  será `openrouter`, igual que la clave que usa `models.dev` — coincidencia
  útil para leer trazas y estadísticas, pero no es algo de lo que dependa
  ninguna lógica (el cruce con `models.dev` va por URL, no por este slug).

## Riesgos

- Si `models.dev` cambiara en el futuro la URL `api` que declara para
  `openrouter`, el preset quedaría desalineado hasta que alguien lo note y
  actualice esa única constante — mismo riesgo que ya existe hoy con el
  preset de Zen, aceptado en la spec 0002.
## Lo que se descubrió al construir

Al escribir la prueba del criterio 5 contra la API real, el catálogo llegó
**sin enriquecer** (`priced: false`, `context_max` nulo) la primera vez, a
pesar de que la caché de `models.dev` en disco sí tenía el precio y los
límites del modelo. Causa real: `Nexo::new()` deja `models_dev` vacío a
propósito, y nada en el arranque de la app garantiza que
`refresh_models_dev()` termine antes de que `refresh_catalog_from_providers()`
descubra los proveedores — `src-tauri/src/main.rs` lanza los dos como tareas
de fondo independientes, sin ningún orden entre ellas. En la prueba se
resolvió llamando a `refresh_models_dev()` antes de `add_custom_provider()`.
En la aplicación real, esto significa que **cualquier proveedor** (Zen,
OpenRouter, o el que sea) añadido en los primeros instantes tras abrir Nexo
—mientras `models.dev` todavía se está cargando— puede quedarse con el
catálogo sin enriquecer hasta el próximo refresco manual o reinicio. No es un
fallo de esta spec ni de OpenRouter: es un fallo de orden de arranque que ya
existía para Zen, solo que ninguna prueba anterior comprobaba el enriquecimiento
lo bastante a fondo para notarlo. Anotado en «Fuera de alcance» para
arreglarlo aparte.

## Invariantes que esto no puede romper

- **6. Se conserva el dato original del proveedor.** El atajo no cambia cómo
  se guarda ni se muestra el catálogo; solo precarga dos campos de texto.
- **7. Lo frágil vive aislado.** El valor de la URL de OpenRouter vive en la
  misma constante `ProviderPreset` que ya aísla el de Zen, en
  `crates/nexo-core/src/provider/openai_compat.rs` — un fichero, no varios.
