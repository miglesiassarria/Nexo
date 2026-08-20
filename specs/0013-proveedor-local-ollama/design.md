# 0013 · Diseño

El patrón ya existe: LM Studio es este mismo caso resuelto. El diseño consiste
en repetirlo donde aplica y separarse donde Ollama es distinto, que son dos
sitios concretos.

## Ficheros que se tocan

| Fichero | Qué pasa |
| --- | --- |
| `crates/nexo-core/src/provider/ollama.rs` | **nuevo**: adaptador y traducción del catálogo nativo |
| `crates/nexo-core/src/provider/mod.rs` | `pub mod ollama` |
| `crates/nexo-core/src/config.rs` | `Settings` gana `ollama_base_url` |
| `crates/nexo-core/src/service.rs` | registro del adaptador, `detect_ollama`, `ollama_status`, `set_ollama_url`, y la vía en `grantable_routes` |
| `src-tauri/src/main.rs` | detección al arrancar, junto a la de LM Studio |
| `src-tauri/src/commands.rs` + `main.rs` | comandos `detect_ollama`, `ollama_status`, `set_ollama_url` |
| `src/lib/api.ts`, `src/lib/views/Providers.svelte` | la vía en la interfaz |
| `crates/nexo-core/tests/gateway_e2e.rs` | e2e con el adaptador |
| `ROADMAP.md`, `specs/README.md`, `website/index.html` | Ollama pasa de «siguiente» a «disponible» |

## Decisiones

### 1. Módulo propio, no un modo de `openai_compat`

**Alternativa descartada:** extender `OpenaiCompatAdapter` con un modo «local,
sin clave». Se descarta por dos razones. Una, mete un condicional de tipo de
credencial en el adaptador que ya comparten OpenRouter, Zen, Gemini y todo
proveedor que añada el usuario: un cambio ahí los afecta a los cuatro. Dos, y
más importante, el catálogo saldría de `/v1/models`, que solo da
identificadores; se perderían las capacidades que Ollama sí publica, y asumir
«solo texto» cuando el dato existe es tirar información.

**Qué puede romperse:** nada de lo existente; es código nuevo. El riesgo es
duplicar lo que `lmstudio.rs` ya hace, y se acepta a conciencia: el chat son
cuatro líneas que delegan en `translate::chat_completions`, y la parte que de
verdad ocupa —traducir el catálogo nativo— es distinta en cada uno. Fusionarlos
crearía una abstracción sobre dos casos, que es cuando peor salen.

### 2. Las capacidades salen de `/api/tags`, tal como Ollama las declara

Ollama las da como lista de etiquetas, no como campos:

```
capabilities: ["completion", "tools", "thinking"]        → texto, herramientas, razonamiento
capabilities: ["completion", "vision", "tools", "thinking"] → además visión
```

La traducción es directa: `completion` → `text`, `tools` → `tools`, `vision` →
`vision`, `thinking` → `reasoning`. `streaming` y `json_mode` a `true`, que es
lo que la superficie compatible ofrece y se verificó. `embeddings` se deja en
`false` a propósito: Nexo no tiene ruta de embeddings, y declararla haría que el
catálogo prometa algo que el gateway no sirve (invariante 2).

`reasoning_levels` queda vacío: Ollama dice que el modelo razona, no que acepte
niveles. Vacío es exactamente lo que la spec 0009 definió para ese caso.

**Alternativa descartada:** deducir las capacidades del nombre o de la familia
del modelo. Es adivinar teniendo el dato delante.

**Qué puede romperse:** que `/api/tags` cambie de forma. Detección: si la lista
llega vacía o con otra forma, se cae al respaldo `/v1/models` con un `warn`, y
los modelos quedan como solo texto. La versión verificada (**0.32.14**,
2026-08-20) queda anotada en la cabecera del módulo, por la invariante 7.

### 3. Sin credencial en ningún punto

`CredentialKind::Local`, `keychain_ref: None`, y ni una llamada a `SecretRef` en
el módulo. La cabecera `Authorization` no se manda: se verificó que Ollama la
ignora, y mandar una clave falsa para que la tiren es teatro.

**Alternativa descartada:** permitir una API key opcional «por si acaso» para
Ollama tras un proxy con autenticación. Eso es un proveedor OpenAI-compatible
normal, que Nexo ya sabe añadir; no hace falta contaminar la vía local con un
caso que ya tiene su camino.

### 4. La dirección es configurable, con `11434` por defecto

Mismo trato que `lmstudio_base_url`: campo en `Settings`, valor por defecto de
fábrica, y cambiarlo vuelve a detectar. Nada nuevo que diseñar.

### 5. La contabilidad es `Accounting::Local` y `pricing: None`

Es la razón de ser de esta especificación frente al apaño. `Accounting::Local`
ya significa «sin coste ni cuota» en el contrato, y `catalog` no consulta
`models.dev` para esta vía. Un modelo que corre en el portátil no lleva precio,
ni cero: no lleva.

## Lo que puede romperse, en conjunto

| Riesgo | Cómo se detecta |
| --- | --- |
| `/api/tags` cambia de forma | pruebas de `parse_native_models` con la respuesta real capturada; en ejecución, `warn` y respaldo a `/v1/models` |
| Ollama apagado rompe el arranque | la detección va en tarea de fondo y su error es un `warn`, igual que LM Studio; criterio 7 de la spec |
| El modelo tarda en la primera petición | sin tiempo máximo, decisión ya tomada y documentada para LM Studio |
| Colisión de nombres con LM Studio | el prefijo de proveedor y el eje de credencial ya los separan; lo cubre el e2e |
| La interfaz no ofrece la vía nueva | `grantable_routes` sale del catálogo, no de una lista escrita a mano: es el arreglo que ya se hizo cuando LM Studio quedó sin poder autorizarse ([service.rs:1134](../../crates/nexo-core/src/service.rs)) |

## ADR

No hace falta. No cambia ninguna invariante ni ninguna decisión de arquitectura:
es el segundo caso del patrón «proveedor local» que el ADR 0002 y el contrato de
proveedor ya previeron. Si añadir Ollama hubiera exigido tocar el router, el
catálogo o las estadísticas, el contrato estaría mal — y no ha hecho falta.
