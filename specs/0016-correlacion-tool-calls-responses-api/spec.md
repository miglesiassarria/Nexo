# 0016 · Correlación de identificadores en llamadas a herramientas de Responses API

- **Estado:** hecho
- **Creada:** 2026-08-21
- **Pedida por:** el usuario al observar que clientes OpenAI-compatibles estrictos como Msty Studio fallan con `Expected 'id' to be a string.` al usar modelos OpenAI servidos vía `subscription_oauth`.

## Problema

Al servir modelos de OpenAI mediante la vía `subscription_oauth`, Nexo traduce los eventos SSE de la Responses API al formato SSE de Chat Completions.

En la Responses API de OpenAI:
- `response.output_item.added` entrega un objeto `item` con `id: "fc_xxx"` y `call_id: "call_xxx"`.
- `response.function_call_arguments.delta` identifica la llamada utilizando `item_id: "fc_xxx"`.
- `response.function_call_arguments.done` y `response.output_item.done` completan la llamada.

Anteriormente, `translate::responses::translate_event` era una función sin estado que extraía `call_id` en el inicio pero pasaba `item_id` en los deltas de argumentos. Como resultado:
1. `ChunkBuilder` en el gateway recibía identificadores distintos (`"call_xxx"` vs `"fc_xxx"`).
2. `ChunkBuilder` asignaba `index: 0` al inicio de la llamada y luego `index: 1` a los fragmentos de argumentos.
3. Los fragmentos con índice `1` no llevaban el campo `id` (por ser deltas).
4. El cliente (como Msty Studio) interpretaba el índice `1` como una nueva llamada a herramienta sin identificador y abortaba.

## Solución

1. Implementar un traductor de eventos con estado (`ResponsesEventTranslator`) para el stream de Responses API.
2. Mantener un mapeo bidireccional / tabla de correlación entre `item_id` y su `call_id` canónico durante toda la respuesta.
3. En cualquier evento posterior (`function_call_arguments.delta`, `function_call_arguments.done`, `output_item.done`), resolver `item_id` a su `call_id` canónico antes de emitir los eventos de gateway (`ChatEvent::ToolCallDelta`, `ChatEvent::ToolCallEnd`, etc.).
4. Si `call_id` no viene informado o coincide con `item_id`, usar `item_id` de forma transparente.
5. Garantizar que múltiples llamadas simultáneas o secuenciales mantengan sus índices asignados sin mezclar argumentos.

## Criterios de aceptación

1. Una prueba de regresión reproduce el flujo real donde `item.id = "fc_123"` e `item.call_id = "call_123"`, demostrando que falla antes del arreglo y pasa después.
2. Todos los chunks de `tool_calls` para una misma llamada utilizan `index: 0`.
3. El primer chunk contiene `id: "call_123"` y el nombre de la función; los chunks de deltas siguientes usan `index: 0` y contienen solo los argumentos sin duplicar llamadas.
4. Múltiples llamadas a herramientas independientes reciben índices `0`, `1`, etc., de forma diferenciada y coherente.
5. No se rompe la retrocompatibilidad con proveedores que entreguen el mismo identificador en todos los eventos.
6. La suite completa de pruebas (`cargo test --workspace`, `cargo clippy`, `npm run check`) pasa al 100%.
