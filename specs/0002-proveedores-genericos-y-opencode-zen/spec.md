# 0002 · Proveedores genéricos OpenAI-compatible y OpenCode Zen

- **Estado:** spec
- **Creada:** 2026-07-31
- **Pedida por:** Manuel Iglesias — «proveedores que usen el estándar de OpenAI,
  proveedores que usen el estándar de Anthropic, y OpenCode, del que tengo API key».
- **Alcance final, acordado con el usuario:** se añade **un único tipo de proveedor
  nuevo**, el OpenAI-compatible. Los de tipo Anthropic se dejan para más adelante, y
  OpenCode Zen no es un tipo aparte: es ese mismo tipo con la URL ya rellenada.
  El recorte deja las 11 comprobaciones apoyadas en algo medido — lo aplazado era
  justo lo único que no se podía verificar (ningún modelo gratuito de Zen habla
  formato Anthropic y el saldo está a cero).

## Problema

Nexo hoy solo habla con proveedores que conoce por su nombre: ChatGPT, la API de
OpenAI y LM Studio. Cualquier otro servicio que hable el formato más extendido
—`chat/completions` de OpenAI— queda fuera, aunque el usuario tenga ya una clave
para él. Eso incluye proxies, gateways de pago
por uso y servidores propios, y en concreto **OpenCode Zen**, un gateway real con
API key propia que da acceso a 60 modelos de varios fabricantes.

Es el mismo problema que resolvió LM Studio pero al revés: allí Nexo conocía el
servidor y solo faltaba el adaptador. Aquí el servidor lo elige el usuario.

Hay además un problema de segundo orden que esta especificación puede resolver casi
gratis: **Nexo no sabe qué capacidades tiene un modelo que no conoce**. Hoy eso se
resuelve con un manifiesto escrito a mano que se queda obsoleto (ya pasó: el
catálogo de ChatGPT tuvo que pasar a descubrirse del proveedor).

## Lo que se descubrió antes de diseñar (T0)

Probado contra los servicios reales el 2026-07-31, no supuesto. Tres hallazgos que
cambiaron la especificación:

### 1. OpenCode Zen es un proveedor OpenAI-compatible, no tres formatos

La documentación de Zen lista un endpoint distinto por modelo
(`/v1/responses` para los GPT, `/v1/messages` para los Claude,
`/v1/chat/completions` para el resto). Eso invitaba a construir un manifiesto
frágil de formato-por-modelo.

**Es innecesario.** Los tres endpoints declaran `modelList: "full"` en el código de
Zen, y el gateway convierte entre formatos internamente. Verificado:

| Prueba | Resultado |
| --- | --- |
| Claude (documentado como `/v1/messages`) en `/v1/chat/completions` | `CreditsError`, que llega **después** de validar el modelo → la combinación se acepta, solo falta saldo |
| Modelo gratuito (documentado como `chat/completions`) en `/v1/responses` | **HTTP 200**, respuesta correcta |
| Modelo gratuito en `/v1/messages` | HTTP 400 `Upstream request failed` — la única combinación que falló |

Y la prueba definitiva: **`models.dev`, la base de datos de modelos del propio
OpenCode, declara para el proveedor `opencode`**:

```
api: https://opencode.ai/zen/v1
npm: @ai-sdk/openai-compatible
env: [OPENCODE_API_KEY]
```

Su propio catálogo dice que Zen es un proveedor OpenAI-compatible. Es también cómo
lo tiene configurado Msty Studio, que recupera los 60 modelos —Claude incluidos—
apuntando a un único endpoint `https://opencode.ai/zen/v1`.

**Consecuencia:** OpenCode Zen no necesita adaptador propio. Es un caso de uso del
proveedor genérico OpenAI-compatible, con la URL ya rellenada. El punto 3 de la
petición colapsa dentro del punto 1.

### 2. `models.dev` resuelve el problema de las capacidades

Base de datos pública y estructurada (`https://models.dev/api.json`, 3,3 MB), con
**176 proveedores**. Para cada modelo publica:

- `modalities.input` → texto, imagen, pdf (de donde sale la visión)
- `tool_call`, `reasoning`, `temperature` → capacidades
- `limit.context`, `limit.output` → límites
- `cost.input`, `cost.output`, `cost.cache_read`, `cost.cache_write` → **precios**
- `api` y `npm` por proveedor → URL base y formato de cable

Cobertura verificada contra el catálogo real de Zen: **60 de 60 modelos con
metadatos, ninguno huérfano.** Incluye también `openai`, `anthropic`, `google`,
`openrouter` (335 modelos), `deepseek`, `groq`, `mistral`, `xai` y `lmstudio`.

**Consecuencia:** los proveedores genéricos pueden tener capacidades y precios
reales en lugar de «solo texto» por defecto, y el manifiesto local escrito a mano
puede dejar de crecer.

### 3. Un fallo real en el traductor compartido, ya corregido

Probando *tool calling* real contra un modelo gratuito de Zen apareció un defecto
que llevaba desde la primera versión de `openai_apikey.rs`: el `id` de una llamada a
herramienta solo llega en el primer fragmento del stream —comportamiento estándar de
OpenAI, no una rareza de Zen— y los fragmentos de argumentos siguientes no lo
repiten. El código inventaba un id para esos fragmentos, que nunca coincidía, así que
los argumentos no se juntaban nunca con su llamada.

Corregido en `translate::chat_completions` con seguimiento de `id` por índice
(`ToolCallIds`), con una prueba que reproduce la secuencia exacta capturada. De paso
se cubrió que el campo de razonamiento se llama `reasoning_content` en unos backends
y `reasoning` en otros, y que Zen manda un chunk de telemetría tras `[DONE]`.

Como el módulo es compartido, esto corrige el mismo fallo latente en la API key de
OpenAI y en LM Studio. **Ya está en el repositorio: 182 pruebas en verde.**

## Comportamiento esperado

1. El usuario añade un **proveedor OpenAI-compatible** desde *Proveedores*: nombre,
   URL base y API key. Puede añadir varios, cada uno con su nombre.
2. **OpenCode Zen aparece como atajo**, con su URL ya puesta: el usuario solo pega
   su clave.
3. Nexo descubre el catálogo contra el endpoint de modelos del proveedor, y **cruza
   los identificadores con `models.dev`** para obtener capacidades, límites y precios
   reales. Lo que no esté en `models.dev` se ofrece solo como texto, y el usuario
   puede ampliarlo a mano.
4. Los modelos aparecen como `<nombre-elegido>/<modelo>`.
5. Los errores del proveedor llegan traducidos a la taxonomía de Nexo, no como `502`
   genérico. En el caso de Zen eso exige leer el cuerpo, no el HTTP status.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | Un proveedor genérico OpenAI-compatible se crea con nombre, URL y clave, y sirve chat real con streaming | prueba de extremo a extremo contra OpenCode Zen real con un modelo gratuito |
| 2 | Se pueden tener dos proveedores genéricos del mismo tipo a la vez, con catálogos separados y sin colisión de nombres | prueba de servicio con dos cuentas y modelos de igual `api_id` |
| 3 | Crear un segundo proveedor con el mismo nombre se rechaza con un error claro, no sobrescribe el primero | prueba unitaria |
| 4 | El catálogo se cruza con `models.dev`: un modelo conocido obtiene visión, herramientas, contexto y precio reales | prueba con la respuesta de `models.dev` capturada, sin red |
| 5 | Un modelo que no está en `models.dev` se ofrece como solo texto, sin prometer capacidades sin dato | misma prueba |
| 6 | El precio de `models.dev` produce coste `estimated`, nunca `reported` | prueba unitaria: el proveedor no informa del coste, solo de tokens |
| 7 | Los errores de OpenCode Zen se traducen por el cuerpo (`error.type`), no por el HTTP status, que siempre es 401 | prueba unitaria con los tres cuerpos reales capturados (`CreditsError`, `ModelError`, `AuthError`) |
| 8 | «Saldo insuficiente» llega al cliente como error de crédito comprensible, no como «clave inválida» | misma prueba; es el caso que el usuario vio en Msty |
| 9 | OpenCode Zen se conecta desde un atajo con la URL ya puesta, y descubre sus 60 modelos | prueba contra Zen real |
| 10 | Ninguno de los proveedores nuevos exige límite por aplicación (son `ApiKey`, no suscripción) | prueba de política |
| 11 | El repositorio verde y la aplicación instalada | `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`, luego `npm run app:install` |

## Fuera de alcance

- **Proveedores de tipo Anthropic-compatible.** Aplazado a petición del usuario. Era
  la única parte no verificable de esta especificación: los únicos modelos de Zen en
  formato `Messages` son Claude y Qwen, ambos de pago, y el saldo está a cero. Se
  retomará cuando haya una forma de probarlo contra un servidor real (saldo en Zen o
  una clave directa de Anthropic). El traductor no se escribe «por si acaso»: código
  sin verificar es deuda, no adelanto.
- **Adaptador propio para OpenCode Zen.** Descartado por T0: es un proveedor
  OpenAI-compatible y su propia base de datos lo declara así. Un adaptador dedicado
  sería código duplicado que además habría que mantener.
- **Manifiesto de formato-por-modelo para Zen.** Descartado por T0: los tres
  endpoints aceptan todos los modelos.
- **Usar `/v1/messages` o `/v1/responses` de Zen.** Se sirve todo por
  `chat/completions`, que es la superficie que acepta los 60 modelos y la única
  verificada de punta a punta. Si algún día aparece una pérdida de fidelidad en
  modelos con *thinking* o caché, se reevalúa; no se anticipa sin dato.
- **Modelos de Google/Gemini dentro de Zen por su formato nativo.** No hace falta:
  van por `chat/completions` como el resto.
- **Exponer una superficie `/v1/messages` en el gateway de Nexo** para que clientes
  que hablan Anthropic usen Nexo como destino. Es lo contrario de esto (entrada, no
  salida) y ya está en el ROADMAP.
- **Descargar `models.dev` en cada arranque.** Son 3,3 MB; se cachea con su fecha y
  se refresca a demanda o cuando caduque, no en cada consulta de catálogo.
- **Sustituir el manifiesto local por `models.dev` en los proveedores existentes.**
  ChatGPT por suscripción ya descubre su catálogo del proveedor y funciona; cambiarlo
  ahora sería tocar lo que no está roto. Queda anotado como posible simplificación.

## Supuestos asumidos

- Cada proveedor genérico recibe un `provider_id` propio derivado del nombre que le
  da el usuario (slugificado). Es lo que permite varios simultáneos sin colisión.
- `models.dev` se consulta por `<provider>/<model>` y, si el proveedor no coincide,
  se intenta por identificador de modelo en cualquier proveedor. Es una heurística
  declarada: mejor un dato probable que ninguno, siempre que el coste salga marcado
  como estimado.
- El descubrimiento de catálogo usa `GET {base}/models` para la forma OpenAI. Si no
  responde, el catálogo depende de lo que el usuario añada a mano.

## Riesgos

| Riesgo | Consecuencia |
| --- | --- |
| `models.dev` es un servicio de terceros que puede caer o cambiar de forma | Se cachea; si falla, se cae al comportamiento actual (solo texto) en lugar de quedarse sin catálogo |
| Un identificador de modelo puede existir en varios proveedores de `models.dev` con capacidades distintas | Se prefiere la coincidencia exacta de proveedor; la heurística por nombre solo se usa como respaldo y el coste queda `estimated` |
| El HTTP status de Zen no es fiable (todo devuelve 401) | El clasificador lee `error.type`, probado con los tres casos reales |
| Un servidor genérico puede mentir sobre qué formato habla | Es responsabilidad del usuario al añadirlo; el error de traducción (`Malformed`) lo hace evidente |

## Invariantes que esto no puede romper

- **Nunca degradar en silencio** (nº2): un modelo sin metadatos no promete visión ni
  herramientas; se ofrece como texto y el usuario amplía si lo sabe.
- **Cuatro estados de contabilidad, no dos** (nº3): el precio de `models.dev` no lo
  informa el proveedor, así que el coste es `estimated`. Nunca `reported`.
- **El eje de credencial es de primer nivel** (nº5): todos son
  `CredentialKind::ApiKey`; ninguno exige límite porque ninguno multiplexa una
  suscripción personal.
- **Ningún secreto en SQLite** (nº1): las claves de los proveedores genéricos van al
  Keychain como las demás.
- **Se conserva el dato original** (nº6): el `raw` de uso se guarda igual.
