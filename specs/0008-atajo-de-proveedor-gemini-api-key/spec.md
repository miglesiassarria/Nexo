# 0008 · Atajo de proveedor: Gemini (API key)

- **Estado:** hecho
- **Creada:** 2026-08-03
- **Pedida por:** el usuario, tras preguntar por la vía de suscripción de Gemini
  y descartarla («implementa el api key de google»)

## Problema

Google no ofrece hoy ninguna vía de suscripción (Google AI Pro/Ultra) usable
desde Nexo: el endpoint de Gemini CLI para cuentas individuales dejó de
servir el 2026-06-18, y su sucesor (Antigravity) es cerrado y prohíbe
explícitamente clientes de terceros — detección activa y cuentas de pago ya
restringidas por usar justo ese patrón. Esa vía queda descartada (ver
conversación previa; se anota como riesgo más abajo, no se repite el análisis
aquí).

Lo que Google sí ofrece, soportado y estable, es una API key normal con un
endpoint compatible con el formato `chat/completions` de OpenAI
(`https://generativelanguage.googleapis.com/v1beta/openai/`, verificado
contra `https://ai.google.dev/gemini-api/docs/openai` el 2026-08-03). Hoy,
para usarlo, el usuario tendría que darse de alta como «Otro servicio
OpenAI-compatible» y teclear esa URL a mano — el mismo problema que ya
resolvió la spec 0006 para OpenRouter.

## Comportamiento esperado

- En «Añadir proveedor», junto a los atajos de OpenCode Zen y OpenRouter,
  aparece uno para Gemini con el nombre y la URL base ya puestos.
- El usuario solo pega su API key de Google AI Studio (y, si quiere, cambia
  el nombre) para conectarlo.
- El proveedor que se crea es un OpenAI-compatible más: mismo adaptador,
  mismas estadísticas, mismos permisos por modelo, sin código específico de
  Gemini en ninguna parte.
- El catálogo de ese proveedor lista los modelos reales de Gemini
  (`gemini-3.6-flash`, etc.) desde la primera conexión.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | `connect_options()` ofrece un atajo «Gemini» con `suggested_name = "Gemini"` y `base_url = "https://generativelanguage.googleapis.com/v1beta/openai"` | `cargo test -p nexo-core -- gemini_shortcut` |
| 2 | El atajo de Gemini convive con los de Zen y OpenRouter; los tres siguen distinguibles y sin identificadores repetidos | `cargo test -p nexo-core -- connect_options_cover_the_four_form_shapes` (test ya existente, no debe romperse) |
| 3 | Conectar el atajo, pegar una API key real de Google AI Studio y refrescar el catálogo devuelve modelos reales de Gemini | **Verificado con clave real**: `cargo test -p nexo-core --test gateway_e2e -- --ignored gemini`, contra la API real |
| 4 | Con esa misma clave, enviar un mensaje de chat (streaming y sin streaming) a un modelo real de Gemini responde con texto y el uso se contabiliza | Mismo test de la fila anterior, extendido a `stream()`; contabilidad verificada como `Accounting::Metered` (es una API key, no suscripción) |
| 5 | Si la clave es inválida o el modelo pedido no existe, el error lo dice con claridad (no un `502` genérico) | `cargo test -p nexo-core --test gateway_e2e -- --ignored gemini_invalid_key` / prueba equivalente con clave real revocada o modelo inexistente |

## Fuera de alcance

- **La vía de suscripción de Google (Google AI Pro/Ultra vía OAuth del
  cliente oficial).** Evaluada y descartada de forma explícita: el endpoint
  de cuentas individuales de Gemini CLI dejó de servir el 2026-06-18, y su
  sucesor Antigravity es cerrado y su ToS prohíbe clientes de terceros con
  detección activa desde 2026-03-25 (cuentas de pago ya restringidas). No
  hay, a diferencia de ChatGPT, una forma de identificarse honestamente como
  Nexo que el proveedor acepte. Se documentará como riesgo revisado en el
  ADR 0001 por separado; no es trabajo de esta especificación.
- **Parámetros específicos de Gemini vía `extra_body`** (`thinking_level`,
  `safety_settings`, grounding con Google Search, generación de vídeo). El
  adaptador genérico no manda ningún campo fuera del vocabulario común de
  `chat/completions` para ningún proveedor — mismo criterio que la spec 0006
  aplicó a las cabeceras de atribución de OpenRouter. Se deja para otra
  especificación si hace falta.
- **Cualquier atajo adicional** (Groq, DeepSeek, Mistral, xAI). Se pidió solo
  Gemini.

## Supuestos asumidos

- `docs_url` del preset apunta a `https://ai.google.dev/gemini-api/docs`
  (documentación pública de la API, criterio análogo al `doc` que
  `models.dev` declara para otros proveedores).
- El identificador de cuenta que genera este atajo (`util::slugify("Gemini")`)
  será `gemini`.
- Reasoning: el endpoint traduce `reasoning_effort` a su propio
  `thinking_level` internamente (documentado por Google); no hace falta
  ningún cambio en `translate/chat_completions.rs` para que esto funcione,
  solo verificarlo contra la API real en el criterio 4.

## Riesgos

- **`models.dev` no publica una URL `api` para el proveedor `google`** (a
  diferencia de `opencode` y `openrouter`, que sí la declaran). Verificado
  contra `https://models.dev/api.json` el 2026-08-03: la entrada `google`
  tiene `models` pero no `api`. Consecuencia: `provider_id_for_api` nunca
  encontrará este preset por URL, así que el catálogo se enriquecerá por el
  respaldo de `ModelsDevCatalog::lookup` (búsqueda del id de modelo entre
  **todos** los proveedores conocidos cuando no hay coincidencia por pista),
  no por una coincidencia exacta de proveedor. Los ids de Gemini
  (`gemini-*`, `lyria-*`) son suficientemente distintivos para que esto no
  sea un problema práctico hoy, pero es una enriquecimiento menos preciso
  que el de Zen u OpenRouter y podría cruzarse mal si otro proveedor
  publicara algún día un modelo con el mismo id exacto. No se soluciona en
  esta especificación: es el mismo tipo de dato frágil que ya vive en
  `ProviderPreset`, aceptado en la spec 0002.
- Google avisa de que «el soporte de las bibliotecas de OpenAI sigue en
  beta mientras se extiende el soporte de funcionalidades» — el mismo tipo
  de riesgo de rotura unilateral que ya se acepta para Zen y OpenRouter.
- Los límites de la capa gratuita de Gemini no están documentados con cifras
  fijas (dependen del nivel de uso en Google AI Studio) — no cambia nada de
  esta especificación, pero puede producir 429 más agresivos que otros
  proveedores; ya se corrigió por separado (PR #10) que el adaptador
  genérico propague `Retry-After` cuando el proveedor lo envíe.

## Lo que se descubrió al construir

Verificado con una clave real de Google AI Studio el 2026-08-03. Cuatro
sorpresas, ninguna anticipada en `design.md`, las cuatro con prueba real que
las reproduce antes del arreglo:

1. **El catálogo de Gemini devuelve los ids con el espacio de nombres
   delante** (`"models/gemini-2.5-flash"`, no `"gemini-2.5-flash"`) —
   confirmado también que el nombre desnudo, aunque HTTP 200, produce una
   respuesta sin contenido (`completion_tokens: 0`, sin campo `content`) en
   vez de un error: degradación silenciosa del propio Google, no de Nexo.
   Consecuencia real: **`models.dev` guarda sus modelos sin ese prefijo**, así
   que ni siquiera el respaldo entre proveedores de
   `ModelsDevCatalog::lookup` los encontraba. Arreglado en
   `crates/nexo-core/src/catalog/models_dev.rs`: `lookup` reintenta sin el
   prefijo `models/` antes de rendirse. No es específico de Gemini en el
   código (cualquier proveedor con esa convención se beneficia igual), pero
   solo se ha visto en Gemini.
2. **El sobre de error de Gemini no es el que reconoce el resto de
   proveedores.** Usa la forma `google.rpc.Status`
   (`{"error":{"code","message","status"}}`, con `status` en vez de `type`),
   y algunos errores llegan envueltos en un array de un elemento
   (`[{"error": {...}}]`). Una clave inválida da HTTP 400 con
   `status: "INVALID_ARGUMENT"`, no 401/403. Sin reconocerlo, caía al caso
   genérico y el cliente veía un `502`, justo lo que prohíbe el criterio 5.
   Arreglado en `translate/chat_completions.rs::parse_error_envelope`,
   desenvolviendo el array y añadiendo el sobre de Google como segundo caso
   — clasificando solo cuando el mensaje real habla de la clave o del modelo,
   para no confundir un `INVALID_ARGUMENT` cualquiera con un problema de
   credencial.
3. **Gemini 2.5 Flash razona por defecto**, y con un `max_tokens` bajo puede
   gastarlo entero en tokens de razonamiento invisibles sin dejar nada para
   la respuesta visible — no determinista: con `max_tokens: 20`, 3 de 4
   intentos daban contenido nulo en la prueba manual. No es un fallo de
   Nexo (el cuerpo que se manda es idéntico al de cualquier otro proveedor
   compatible), así que se resolvió subiendo el presupuesto en las pruebas
   (`200`), no en el adaptador. Queda anotado como riesgo, no como arreglo.
4. **`add_custom_provider` no propaga el fallo de descubrir el catálogo**
   como `Err` (solo lo registra); esto ya era así antes de esta spec, pero
   se hizo visible al escribir la prueba de clave inválida, que tuvo que
   comprobar el resultado de `refresh_catalog_from_providers` en vez del
   resultado de `add_custom_provider`. No se cambia ese comportamiento aquí:
   es una discrepancia entre lo que la especificación asumía al escribirse y
   cómo funciona hoy el servicio, documentada para quien la lea después.

## Invariantes que esto no puede romper

- **6. Se conserva el dato original del proveedor.** El atajo no cambia cómo
  se guarda ni se muestra el catálogo; solo precarga dos campos de texto.
- **7. Lo frágil vive aislado.** La URL de Gemini vive en la misma constante
  `ProviderPreset` que ya aísla las de Zen y OpenRouter, en
  `crates/nexo-core/src/provider/openai_compat.rs` — un fichero, no varios.
- **8. Nexo se identifica como Nexo.** Es justo la propiedad que hace
  aceptable esta vía y no la de suscripción: la API key no requiere
  suplantar ningún cliente.
