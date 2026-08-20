# 0013 · Proveedor local: Ollama

- **Estado:** build
- **Creada:** 2026-08-20
- **Pedida por:** el usuario: «sabes si el endpoint de ollama es compatible con el
  de open ai y por extensión si nexo puede servir los modelos de ollama que tenga
  en local?». Se le explicó que hoy solo se puede con el apaño de darlo de alta
  como proveedor genérico con una API key inventada, y por qué eso queda mal por
  dentro; aprobó hacerlo bien: «adelante».

## Problema

Ollama corre modelos en la máquina del usuario y publica una superficie
compatible con OpenAI. Nexo no lo conoce. Hoy solo se puede usar por el atajo de
darlo de alta como proveedor OpenAI-compatible genérico, y eso obliga a
inventarse una API key —`add_custom_provider` rechaza la vacía
([service.rs:255](../../crates/nexo-core/src/service.rs))— con tres consecuencias
que el producto promete evitar:

1. Entra como `credential_kind: api_key` en lugar de `local`. Por el eje de
   credencial (invariante 5) aparece entre los proveedores de pago.
2. La contabilidad lo marca `metered` y `priced`, e intenta ponerle precio contra
   `models.dev`. Son modelos que corren en el portátil del usuario y cuestan
   cero. Confundir eso es exactamente lo que la invariante 3 promete no hacer.
3. Deja una clave falsa en el almacén seguro, para siempre, que no autentica
   nada.

Además, por la vía genérica el catálogo saldría de `/v1/models`, que solo da
identificadores. El endpoint nativo de Ollama publica capacidades reales, y
tirarlas para asumir «solo texto» es perder información que sí existe.

## Lo que se comprobó contra Ollama real antes de escribir esto

Ollama **0.32.14**, en la máquina del usuario, el 2026-08-20. Nada de esto se
supone:

| Comprobación | Resultado |
| --- | --- |
| `GET /v1/models` | `200`, sobre OpenAI (`object: list`, `data[].id`) |
| `POST /v1/chat/completions` | `200`; con modelo inexistente, `404` y sobre de error OpenAI (`error.message`, `error.type: not_found_error`) |
| Cabecera `Authorization` | **Se ignora por completo**: responde `200` con un Bearer inventado y sin cabecera |
| `usage` sin streaming | Sí: `prompt_tokens`, `completion_tokens`, `total_tokens` |
| Streaming | SSE con `Content-Type: text/event-stream`, termina en `[DONE]`, y con `stream_options.include_usage` manda un último chunk solo con `usage` |
| `tools` | Funcionan de verdad: `finish_reason: tool_calls` y `tool_calls[].function.arguments` bien formados |
| `GET /api/tags` (nativo) | Publica por modelo: `capabilities` (`completion`, `tools`, `thinking`, `vision`), `details.context_length`, `parameter_size`, `quantization_level`, `family` |
| `GET /api/ps` | Los modelos actualmente cargados en memoria |
| Campos ausentes | `context_length` viene `null` en algún modelo (`qwen3.8:27b-mlx`), y `family` vacía. No se puede asumir presente |

Dos diferencias con LM Studio que importan: el catálogo nativo de Ollama está en
`/api/tags` (no `/api/v0/models`) y da las capacidades como **lista de
etiquetas**, no como campos booleanos.

## Comportamiento esperado

1. Ollama es un proveedor **local**, con la misma forma que LM Studio:
   `provider_id: "ollama"`, `CredentialKind::Local`, sin credencial y sin nada
   que guardar en el almacén seguro.
2. Se detecta solo al arrancar en `http://127.0.0.1:11434`, igual que LM Studio,
   y la dirección es configurable por si el usuario lo tiene en otro puerto.
3. El catálogo sale del endpoint nativo `/api/tags`, y de ahí se derivan las
   capacidades **declaradas por el propio Ollama**: `tools` → herramientas,
   `vision` → visión, `thinking` → razonamiento. Lo que Ollama no declara, no se
   promete (invariante 2).
4. La contabilidad es `local`: coste marginal cero, sin precio de `models.dev`,
   sin inventar cifras.
5. El chat y el streaming van por la superficie compatible con OpenAI, con
   `usage` real del proveedor (`usage_source: reported`, no estimado).
6. Los permisos y los límites por aplicación funcionan igual que para cualquier
   otra vía, indexados por proveedor y tipo de credencial.
7. Si Ollama no está en marcha, no aparece y no rompe nada: mismo trato que LM
   Studio apagado.

## Criterios de aceptación

1. **El adaptador se identifica como local.** `AdapterId` es
   `("ollama", CredentialKind::Local)`.
   `cargo test -p nexo-core ollama_adapter_is_a_local_provider`
2. **El catálogo nativo se traduce con las capacidades que Ollama declara.** Un
   `/api/tags` con `capabilities: ["completion","tools","thinking"]` produce un
   modelo con herramientas y razonamiento, y **sin** visión; uno con `vision` la
   declara. `context_length` ausente no rompe ni se inventa.
   `cargo test -p nexo-core ollama::tests`
3. **La contabilidad es local, no medida.** El modelo aparece con
   `accounting: local` y sin precio.
   `cargo test -p nexo-core ollama_models_are_accounted_as_local`
4. **Se detecta y se da de alta sin credencial.** `detect_ollama()` crea la
   cuenta con `credential_kind: local`, `keychain_ref: None`, y no escribe nada
   en el almacén seguro.
5. **No hace falta ninguna API key en ningún punto.** Ni al detectar, ni al
   servir. `! grep -n "SecretRef" crates/nexo-core/src/provider/ollama.rs`
6. **El gateway lo sirve de punta a punta.** Con el proveedor concedido a una
   aplicación, `POST /v1/chat/completions` responde `200`, y sin token `401`.
   `cargo test -p nexo-core --test gateway_e2e ollama`
7. **Verificación del repositorio y app instalada.**
   `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check && npm run app:install`
8. **Contra Ollama real, con la app instalada.** Una conversación por
   `http://127.0.0.1:8787/v1` con el modelo `ollama/qwen3:0.6b` responde con
   texto y con `usage` reportado; el streaming reensambla el mismo texto; y la
   fila queda registrada con `accounting: local` y coste cero.
   Comprobación manual con `curl`, informando de la salida real.

## Fuera de alcance

- **Arrancar, parar o descargar modelos desde Nexo.** Nexo consume lo que haya;
  gestionar el ciclo de vida de los modelos es de Ollama. Misma frontera que se
  fijó con LM Studio.
- **Embeddings.** Ollama los soporta y algunos modelos los declaran, pero Nexo
  no tiene todavía la ruta `/v1/embeddings`. Es su propia especificación y está
  en el roadmap; aquí se ignora la etiqueta `embedding` en lugar de prometerla.
- **El endpoint nativo `/api/chat`.** El chat va por la superficie compatible,
  que ya está verificada. Usar la nativa solo se justificaría si la compatible
  perdiera algo, y no lo hace.
- **Mapear `thinking` a niveles de esfuerzo.** Se declara que el modelo razona,
  pero los niveles (`low`/`medium`/`high` de la spec 0009) no existen en Ollama;
  no se inventan.
- **Migrar a nadie que ya tenga Ollama dado de alta como proveedor genérico.**
  Si el usuario lo hizo con una clave inventada, se le dice cómo quitarlo; no se
  toca su configuración por su cuenta.
- **Windows y Linux.** El adaptador es agnóstico, pero la verificación contra lo
  real es en macOS, como todo lo demás hasta hoy.

## Riesgos

- **`/api/tags` cambia de forma en una versión futura.** Es el punto frágil, y
  por la invariante 7 vive en un único fichero con la versión verificada
  anotada. El respaldo es `/v1/models`, que da identificadores sin capacidades:
  degradar a «solo texto» es honesto, prometer lo que no se sabe no.
- **Un modelo declara `tools` pero el modelo concreto los hace mal.** Nexo
  transmite lo que el proveedor declara; si miente, miente el proveedor. Se
  conserva el dato original (invariante 6) para poder auditarlo.
- **Colisión de identificadores con LM Studio.** Los dos pueden servir
  `qwen3:0.6b`. No es un problema nuevo: el eje de credencial y el prefijo de
  proveedor (`ollama/…` frente a `lmstudio/…`) ya los distinguen.

## Supuestos declarados

- El puerto por defecto es `11434`, el de Ollama de fábrica.
- Los identificadores públicos llevan el prefijo del proveedor, como el resto:
  `ollama/qwen3:0.6b`. Los dos puntos del nombre de Ollama no se tocan; el
  original se conserva (invariante 6).
- La etiqueta `completion` de `capabilities` significa chat de texto, y es la
  base: sin ella el modelo no se ofrece para chat.
- Los tiempos de la primera petición pueden ser largos porque Ollama carga el
  modelo en ese momento, igual que LM Studio. No se impone tiempo máximo, por la
  misma razón ya documentada en `provider/lmstudio.rs`.
