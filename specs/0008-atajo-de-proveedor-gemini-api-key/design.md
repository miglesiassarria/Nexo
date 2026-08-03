# 0008 · Diseño

## Enfoque

Añadir Gemini como un tercer `ProviderPreset` en
`crates/nexo-core/src/provider/openai_compat.rs`, exactamente al lado de
`OPENCODE_ZEN` y `OPENROUTER` (spec 0006). No hace falta ningún adaptador,
traductor ni ruta nueva: el endpoint compatible de Google habla el mismo
`chat/completions` que ya sirve `OpenAiCompatAdapter`, verificado contra
`https://ai.google.dev/gemini-api/docs/openai` el 2026-08-03 (Bearer,
`GET /models`, SSE con `choices[0].delta`). El único código nuevo son las
pruebas: la constante del preset y las pruebas de extremo a extremo contra la
API real, siguiendo el patrón que ya deja la spec 0006 para OpenRouter.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `crates/nexo-core/src/provider/openai_compat.rs` | Nueva constante `GEMINI: ProviderPreset`, añadida a `presets()` |
| `crates/nexo-core/src/service.rs` | Ninguno — `connect_options()` ya itera `openai_compat::presets()` en un bucle; un preset más no toca este fichero |
| `crates/nexo-core/tests/gateway_e2e.rs` | Pruebas `#[ignore]` nuevas contra la API real de Gemini, análogas a las de OpenRouter (`start_with_gemini`, descubrimiento de catálogo, chat con y sin streaming, clave/modelo inválidos) |
| `specs/README.md` | Fila para la spec 0008 |

Confirmado leyendo `service.rs` (no de memoria): el bucle de `connect_options()`
sobre `openai_compat::presets()` (línea ~1148) no tiene nada específico de Zen
ni de OpenRouter — añadir un preset no requiere tocar ese fichero, a
diferencia de lo que sí tocó la spec 0006 (el texto del atajo genérico, que
citaba a OpenRouter como ejemplo). Este atajo no aparece en ese texto, así que
tampoco hace falta ese cambio aquí.

## Decisiones

### D1. Base URL sin barra final: `https://generativelanguage.googleapis.com/v1beta/openai`

- **Decisión:** igual que `OPENROUTER.base_url` y `OPENCODE_ZEN.base_url`, sin
  `/` final, porque `OpenAiCompatAdapter::base_url()` concatena `{base}/models`
  y `{base}/chat/completions` directamente.
- **Alternativa descartada:** guardar la URL con barra final y confiar en que
  `base_url()` la recorta al leer la credencial — funcionaría igual (esa
  función ya hace `trim_end_matches('/')`), pero los otros dos presets no
  llevan barra final y mezclar convenciones dentro del mismo fichero es
  confuso sin ganar nada.
- **Consecuencia que hay que asumir:** ninguna; es solo consistencia visual
  entre las tres constantes.

### D2. No añadir `google` a `provider_by_api` a mano

- **Decisión:** no tocar `ModelsDevCatalog::parse` para inyectar una
  coincidencia de URL que `models.dev` no declara. El enriquecimiento del
  catálogo caerá al respaldo de `ModelsDevCatalog::lookup`, que busca el id
  del modelo entre **todos** los proveedores conocidos cuando la pista de
  proveedor no encuentra coincidencia — comportamiento ya existente y
  probado (`exact_provider_match_is_preferred_over_cross_provider_fallback`).
- **Alternativa descartada:** hardcodear en `openai_compat.rs` un mapeo
  `"gemini" -> "google"` para forzar la pista correcta. Se descarta porque
  añadiría una excepción de una sola línea que solo existe para compensar un
  dato que falta en una fuente externa, y el respaldo ya cubre el caso en la
  práctica (verificado: los ids `gemini-*` no colisionan con ningún otro
  proveedor en el `models.dev` real descargado el 2026-08-03). Si algún día
  colisionan, el síntoma es un precio o límite de contexto equivocado, no una
  petición rota — y se corrige entonces, no antes.
- **Consecuencia que hay que asumir:** el enriquecimiento es menos exacto que
  el de Zen/OpenRouter (coincidencia por id de modelo entre proveedores, no
  por proveedor exacto). Ya está en `spec.md`, sección Riesgos.

### D3. Nombre del atajo: «Gemini», no «Google» ni «Google AI»

- **Decisión:** `suggested_name = "Gemini"`, que es como el usuario conoce el
  producto y como lo nombran los modelos reales (`gemini-*`) que verá en el
  catálogo.
- **Alternativa descartada:** «Google AI Studio» (el nombre de la consola
  donde se genera la clave) — más preciso técnicamente, pero es el nombre de
  la herramienta para *obtener* la clave, no del proveedor que el usuario
  reconoce al elegir un modelo.
- **Consecuencia que hay que asumir:** el identificador de cuenta generado
  (`util::slugify("Gemini")` = `gemini`) no coincide con la clave `google` que
  usa `models.dev` — mismo desajuste de nombres que ya tiene Zen (`opencode-zen`
  vs. `opencode`), resuelto igual: por el respaldo de D2, no porque coincidan
  los slugs.

## Qué puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| Google cambia la forma de su capa de compatibilidad OpenAI (sigue en beta según su propia documentación) | Los tests `#[ignore]` contra la API real fallarían en la próxima ejecución manual; sin ellos pasando, no se puede cerrar ninguna especificación futura que dependa de Gemini |
| Un modelo de otro proveedor en `models.dev` publica algún día un id idéntico a uno de Gemini (p. ej. `gemini-...` fuera de `google`) | El catálogo mostraría precio o límites de otro proveedor para un modelo de Gemini; no hay alarma automática — es el mismo riesgo aceptado, sin detección, que ya tiene cualquier `ProviderPreset` (spec 0002) |
| Límites de la capa gratuita de Gemini más agresivos que otros proveedores | Ya cubierto de forma general por el PR #10 (propagación de `Retry-After`); no es específico de esta spec |
| El usuario intenta usar `extra_body` (grounding, `thinking_level` explícito, etc.) esperando que funcione porque «ya tiene Gemini» | Fuera de alcance declarado en `spec.md`; si llega, el cuerpo de la petición simplemente no lleva esos campos y Gemini los ignora o devuelve su comportamiento por defecto — no hay error confuso, solo la funcionalidad ausente |

## ¿Hace falta un ADR?

No para esta spec. La API key es una vía estándar, estable y ya cubierta por
las invariantes existentes (eje de credencial de primer nivel, dato original
conservado, nada de secretos en SQLite). No introduce ninguna decisión de
arquitectura nueva.

Sí queda pendiente, **fuera de esta especificación**, revisar el ADR 0001 a la
luz de lo descubierto evaluando la vía de suscripción de Google (Gemini CLI
cortado el 2026-06-18, Antigravity cerrado y con detección activa desde
2026-03-25): el disparador de revisión que el propio ADR 0001 declara
(«evidencia de que el patrón de multiplexación provoca bloqueos de cuenta»)
ya se ha cumplido, aunque en otro proveedor. Se anota aquí para no perderlo,
no se resuelve en esta spec.

## Qué queda pendiente de descubrir

- Si la respuesta real de streaming de Gemini usa exactamente
  `choices[0].delta.content` y `finish_reason` como los documenta Google, o
  si (como pasó con Zen y `reasoning_content`/`reasoning`) hay algún campo
  con nombre distinto que `translate/chat_completions.rs` no reconozca hoy.
  Solo se sabrá con una clave real, en `/build`.
- Si `GET /models` de Gemini devuelve el mismo sobre
  `{"object":"list","data":[{"id":...}]}` que asume `parse_model_ids`, o si
  Google envuelve algo distinto pese a llamarse «compatible con OpenAI».
  También solo se sabe probando.
- Si el catálogo de Gemini incluye modelos que no son de texto (imagen, TTS,
  vídeo — vistos en la muestra de `models.dev`) y cómo se comportan al
  pedirles chat: probablemente falla su capacidad, ya cubierto por el flujo
  genérico de «solo texto sin capacidades» cuando `models.dev` no confirma
  una capacidad — pero conviene confirmarlo con la clave real y no asumirlo.
