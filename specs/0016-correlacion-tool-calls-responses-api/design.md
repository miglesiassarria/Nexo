# 0016 · Diseño: Correlación de identificadores en llamadas a herramientas de Responses API

- **Estado:** hecho
- **Fichero:** `specs/0016-correlacion-tool-calls-responses-api/design.md`
- **Spec:** [`spec.md`](spec.md)

## Componentes Afectados

### 1. `crates/nexo-core/src/translate/responses.rs`
- Introducir `ResponsesEventTranslator`:
  ```rust
  #[derive(Default)]
  pub struct ResponsesEventTranslator {
      item_to_call_id: HashMap<String, String>,
  }

  impl ResponsesEventTranslator {
      pub fn new() -> Self {
          Self::default()
      }

      pub fn translate_event(&mut self, event_name: &str, data: &Value) -> Translated {
          // Lógica de traducción correlacionando item_id -> canonical call_id
      }
  }
  ```
- Al recibir `response.output_item.added`:
  - Extraer `item.id` (p. ej. `"fc_123"`) y `call_id` canónico (`call_id(item)` $\to$ `"call_123"`).
  - Registrar en `item_to_call_id`: `item_id -> call_id`.
  - Emitir `ChatEvent::ToolCallStart { id: canonical_id, name }`.
- Al recibir `response.function_call_arguments.delta`:
  - Extraer `item_id` de `data`.
  - Resolver `id = self.resolve_call_id(item_id)`.
  - Emitir `ChatEvent::ToolCallDelta { id, args_json }`.
- Al recibir `response.function_call_arguments.done`:
  - Extraer `item_id` de `data`.
  - Resolver `id = self.resolve_call_id(item_id)`.
  - Emitir `ChatEvent::ToolCallEnd { id }`.
- Al recibir `response.output_item.done`:
  - Extraer `item_id = item.id` y `call_id = call_id(item)`.
  - Resolver `id = self.resolve_call_id(item_id.or(call_id))`.
  - Emitir `ChatEvent::ToolCallArgumentsDone { id, args_json }` y `ChatEvent::ToolCallEnd { id }`.
- Mantener la función pública `pub fn translate_event(event_name: &str, data: &Value) -> Translated` como conveniencia stateless para compatibilidad con llamadas individuales en tests, delegando en una instancia temporal.

### 2. `crates/nexo-core/src/provider/chatgpt_subscription.rs`
- En el adaptador `chatgpt_subscription`, mantener la instancia de `ResponsesEventTranslator` durante el ciclo de vida del stream SSE:
  ```rust
  let mut translator = responses::ResponsesEventTranslator::new();
  let stream = response
      .bytes_stream()
      .eventsource()
      ...
      .flat_map(move |item| {
          ...
          match translator.translate_event(&event.event, &value) {
              Translated::Events(evs) => evs.into_iter().map(Ok).collect(),
              Translated::Failure(e) => vec![Err(e)],
              Translated::Ignored => vec![],
          }
      });
  ```

## Invariantes y Compatibilidad
- Los deltas de argumentos y el inicio de la llamada siempre comparten el mismo `id` canónico.
- `ChunkBuilder` en `wire.rs` recibe el mismo `id` tanto en `ToolCallStart` como en `ToolCallDelta`, asignando `index: 0` de principio a fin.
- Múltiples llamadas a herramientas con distintos `item_id` / `call_id` reciben índices correlativos `0`, `1`, `2`... sin mezclarse.
