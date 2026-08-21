//! Traducción entre la representación interna de Nexo y el formato Responses.
//!
//! Esta traducción es el caso base, no la excepción: la ruta de suscripción de
//! ChatGPT habla Responses y la API pública de Nexo habla `chat/completions`.

use crate::provider::{
    AdapterError, ChatEvent, ChatRequest, ContentPart, FinishReason, Message, Role, ToolChoice,
    UsageReport, UsageSource,
};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Construye el cuerpo de una petición Responses a partir de la petición interna.
pub fn build_request(req: &ChatRequest) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), json!(req.api_model));
    body.insert("stream".into(), json!(true));
    // Nexo no delega la conservación de conversaciones en el proveedor.
    body.insert("store".into(), json!(false));

    if let Some(system) = req.system_text() {
        body.insert("instructions".into(), json!(system));
    }

    let input: Vec<Value> = req
        .messages
        .iter()
        .filter(|m| m.role != Role::System)
        .flat_map(input_items_for)
        .collect();
    body.insert("input".into(), json!(input));

    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();
        body.insert("tools".into(), json!(tools));
        body.insert(
            "tool_choice".into(),
            match &req.tool_choice {
                ToolChoice::Auto => json!("auto"),
                ToolChoice::None => json!("none"),
                ToolChoice::Required => json!("required"),
                ToolChoice::Named(name) => json!({"type": "function", "name": name}),
            },
        );
    }

    if let Some(effort) = req.reasoning {
        body.insert("reasoning".into(), json!({"effort": effort.as_str()}));
    }
    // El backend de la suscripción rechaza `max_output_tokens`, aunque sea un
    // campo válido en Responses públicas. Nexo conserva el límite para otras
    // vías, pero aquí debe omitirse y dejar que el proveedor aplique el suyo.
    if req.json_mode {
        body.insert("text".into(), json!({"format": {"type": "json_object"}}));
    }

    // `temperature`, `top_p` y `stop` se omiten a propósito: esta ruta no los
    // acepta de forma fiable. Se registra en el log del adaptador cuando el
    // cliente los envía, en lugar de fingir que se han aplicado.

    Value::Object(body)
}

fn input_items_for(m: &Message) -> Vec<Value> {
    match m.role {
        Role::System => vec![],
        Role::Tool => vec![json!({
            "type": "function_call_output",
            "call_id": m.tool_call_id.clone().unwrap_or_default(),
            "output": m.text(),
        })],
        Role::Assistant => {
            let mut items = Vec::new();
            let text = m.text();
            if !text.is_empty() {
                items.push(json!({
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": text}],
                }));
            }
            for call in &m.tool_calls {
                items.push(json!({
                    "type": "function_call",
                    "call_id": call.id,
                    "name": call.name,
                    "arguments": call.arguments_json,
                }));
            }
            items
        }
        Role::User => {
            let content: Vec<Value> = m
                .parts
                .iter()
                .map(|p| match p {
                    ContentPart::Text(t) => json!({"type": "input_text", "text": t}),
                    ContentPart::ImageUrl(url) => {
                        json!({"type": "input_image", "image_url": url})
                    }
                    ContentPart::Audio { mime, base64 } => json!({
                        "type": "input_audio",
                        "audio": {"data": base64, "format": mime},
                    }),
                    ContentPart::File { name, mime, base64 } => json!({
                        "type": "input_file",
                        "filename": name,
                        "file_data": format!("data:{mime};base64,{base64}"),
                    }),
                })
                .collect();
            vec![json!({"type": "message", "role": "user", "content": content})]
        }
    }
}

/// Resultado de traducir un evento SSE del formato Responses.
pub enum Translated {
    /// Uno o más eventos internos.
    Events(Vec<ChatEvent>),
    /// El proveedor comunicó un fallo.
    Failure(AdapterError),
    /// Evento sin equivalente interno; se ignora.
    Ignored,
}

/// Traductor con estado de eventos SSE de la Responses API.
///
/// Mantiene la correlación entre los identificadores internos del stream (`item.id`,
/// p.ej. `"fc_123"`) y los identificadores canónicos de la llamada (`call_id`,
/// p.ej. `"call_123"`), garantizando que los deltas posteriores compartan el mismo `id`
/// canónico y `ChunkBuilder` mantenga un índice único (`index: 0`) para toda la llamada.
#[derive(Default, Debug, Clone)]
pub struct ResponsesEventTranslator {
    item_to_call_id: HashMap<String, String>,
}

impl ResponsesEventTranslator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resuelve el identificador de llamada canónico a partir de un `item_id` o `call_id`.
    pub fn resolve_call_id(&self, id: &str) -> String {
        if let Some(canonical) = self.item_to_call_id.get(id) {
            canonical.clone()
        } else {
            id.to_string()
        }
    }

    fn register_mapping(&mut self, item_id: Option<&str>, call_id: Option<&str>) {
        if let (Some(item_id), Some(call_id)) = (item_id, call_id) {
            if !item_id.is_empty() && !call_id.is_empty() {
                self.item_to_call_id
                    .insert(item_id.to_string(), call_id.to_string());
                self.item_to_call_id
                    .insert(call_id.to_string(), call_id.to_string());
            }
        }
    }

    /// Traduce un evento SSE de Responses al vocabulario interno.
    pub fn translate_event(&mut self, event_name: &str, data: &Value) -> Translated {
        // Algunos eventos llegan sin `event:` y traen el tipo dentro del payload.
        let kind = if event_name.is_empty() {
            data.get("type").and_then(|v| v.as_str()).unwrap_or("")
        } else {
            event_name
        };

        match kind {
            "response.created" => Translated::Events(vec![ChatEvent::Started {
                provider_request_id: data
                    .pointer("/response/id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            }]),

            "response.output_text.delta" => match data.get("delta").and_then(|v| v.as_str()) {
                Some(text) if !text.is_empty() => {
                    Translated::Events(vec![ChatEvent::TextDelta { text: text.to_string() }])
                }
                _ => Translated::Ignored,
            },

            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                match data.get("delta").and_then(|v| v.as_str()) {
                    Some(text) if !text.is_empty() => Translated::Events(vec![
                        ChatEvent::ReasoningDelta { text: text.to_string() },
                    ]),
                    _ => Translated::Ignored,
                }
            }

            "response.output_item.added" => {
                let item = data.get("item");
                let is_call = item
                    .and_then(|i| i.get("type"))
                    .and_then(|v| v.as_str())
                    .map(|t| t == "function_call")
                    .unwrap_or(false);
                if !is_call {
                    return Translated::Ignored;
                }
                let item_id = item.and_then(|i| i.get("id")).and_then(|v| v.as_str());
                let explicit_call_id = call_id(item);
                self.register_mapping(item_id, explicit_call_id.as_deref());

                let id = explicit_call_id
                    .or_else(|| item_id.map(str::to_string))
                    .unwrap_or_default();
                let name = item
                    .and_then(|i| i.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                Translated::Events(vec![ChatEvent::ToolCallStart { id, name }])
            }

            "response.function_call_arguments.delta" => {
                let raw_id = data
                    .get("item_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let id = self.resolve_call_id(raw_id);
                match data.get("delta").and_then(|v| v.as_str()) {
                    Some(args) if !args.is_empty() => Translated::Events(vec![
                        ChatEvent::ToolCallDelta { id, args_json: args.to_string() },
                    ]),
                    _ => Translated::Ignored,
                }
            }

            "response.function_call_arguments.done" => {
                let raw_id = data
                    .get("item_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        data.get("item")
                            .and_then(|i| i.get("id"))
                            .and_then(|v| v.as_str())
                    });
                let id = raw_id
                    .map(|r| self.resolve_call_id(r))
                    .or_else(|| call_id(data.get("item")));
                match id {
                    Some(id) if !id.is_empty() => {
                        Translated::Events(vec![ChatEvent::ToolCallEnd { id }])
                    }
                    _ => Translated::Ignored,
                }
            }

            "response.output_item.done" => {
                let item = data.get("item");
                let item_id = item.and_then(|i| i.get("id")).and_then(|v| v.as_str());
                let explicit_call_id = call_id(item);
                self.register_mapping(item_id, explicit_call_id.as_deref());

                let id = explicit_call_id
                    .or_else(|| item_id.map(|r| self.resolve_call_id(r)));
                match id {
                    Some(id) if !id.is_empty() => {
                        let mut events = Vec::new();
                        if let Some(arguments) = item
                            .and_then(|value| value.get("arguments"))
                            .and_then(|value| value.as_str())
                        {
                            events.push(ChatEvent::ToolCallArgumentsDone {
                                id: id.clone(),
                                args_json: arguments.to_string(),
                            });
                        }
                        events.push(ChatEvent::ToolCallEnd { id });
                        Translated::Events(events)
                    }
                    _ => Translated::Ignored,
                }
            }

            "response.completed" => {
                let response = data.get("response");
                let usage = parse_usage(response.and_then(|r| r.get("usage")));
                let reason = finish_reason(response);
                let mut events = Vec::new();
                if let Some(u) = usage {
                    events.push(ChatEvent::Usage(u));
                } else {
                    // La ruta de suscripción no informa de tokens. Se registra
                    // como no disponible; no se inventa una cifra.
                    events.push(ChatEvent::Usage(UsageReport::unavailable()));
                }
                events.push(ChatEvent::Finished { reason });
                Translated::Events(events)
            }

            "response.incomplete" => {
                let reason = data
                    .pointer("/response/incomplete_details/reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let finish = if reason == "max_output_tokens" {
                    FinishReason::Length
                } else {
                    FinishReason::ContentFilter
                };
                Translated::Events(vec![
                    ChatEvent::Usage(
                        parse_usage(data.pointer("/response/usage"))
                            .unwrap_or_else(UsageReport::unavailable),
                    ),
                    ChatEvent::Finished { reason: finish },
                ])
            }

            "response.failed" | "error" => {
                let message = data
                    .pointer("/response/error/message")
                    .or_else(|| data.pointer("/error/message"))
                    .or_else(|| data.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("el proveedor no detalló el error")
                    .to_string();
                let code = data
                    .pointer("/response/error/code")
                    .or_else(|| data.pointer("/error/code"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                Translated::Failure(AdapterError::Upstream {
                    status: 502,
                    provider_code: code,
                    message,
                })
            }

            _ => Translated::Ignored,
        }
    }
}

/// Traduce un evento SSE de Responses al vocabulario interno (función de conveniencia stateless).
///
/// `event_name` es el campo `event:` del SSE y `data` su payload ya parseado.
pub fn translate_event(event_name: &str, data: &Value) -> Translated {
    ResponsesEventTranslator::new().translate_event(event_name, data)
}

fn call_id(item: Option<&Value>) -> Option<String> {
    let item = item?;
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn finish_reason(response: Option<&Value>) -> FinishReason {
    let has_call = response
        .and_then(|r| r.get("output"))
        .and_then(|o| o.as_array())
        .map(|items| {
            items.iter().any(|i| {
                i.get("type").and_then(|v| v.as_str()) == Some("function_call")
            })
        })
        .unwrap_or(false);
    if has_call {
        FinishReason::ToolCalls
    } else {
        FinishReason::Stop
    }
}

fn parse_usage(usage: Option<&Value>) -> Option<UsageReport> {
    let usage = usage?;
    let input = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let output = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    if input.is_none() && output.is_none() {
        return None;
    }
    Some(UsageReport {
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: usage
            .pointer("/input_tokens_details/cached_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        reasoning_tokens: usage
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        source: UsageSource::Reported,
        raw: Some(usage.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Message, ReasoningEffort, ToolCall, ToolDef};

    fn base_request() -> ChatRequest {
        ChatRequest {
            api_model: "gpt-5.5".into(),
            public_model: "openai/gpt-5.5".into(),
            messages: vec![],
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            reasoning: None,
            max_output_tokens: None,
            temperature: None,
            top_p: None,
            stop: vec![],
            json_mode: false,
            stream: true,
        }
    }

    fn msg(role: Role, text: &str) -> Message {
        Message {
            role,
            parts: vec![ContentPart::Text(text.into())],
            tool_call_id: None,
            tool_calls: vec![],
        }
    }

    #[test]
    fn system_messages_become_instructions_not_input() {
        let mut req = base_request();
        req.messages = vec![msg(Role::System, "eres útil"), msg(Role::User, "hola")];
        let body = build_request(&req);
        assert_eq!(body["instructions"], "eres útil");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
    }

    #[test]
    fn multiple_system_messages_are_joined() {
        let mut req = base_request();
        req.messages = vec![msg(Role::System, "a"), msg(Role::System, "b")];
        assert_eq!(build_request(&req)["instructions"], "a\n\nb");
    }

    #[test]
    fn store_is_always_false() {
        assert_eq!(build_request(&base_request())["store"], false);
    }

    #[test]
    fn user_image_becomes_input_image() {
        let mut req = base_request();
        req.messages = vec![Message {
            role: Role::User,
            parts: vec![
                ContentPart::Text("mira".into()),
                ContentPart::ImageUrl("https://x/y.png".into()),
            ],
            tool_call_id: None,
            tool_calls: vec![],
        }];
        let content = build_request(&req)["input"][0]["content"].clone();
        assert_eq!(content[0]["type"], "input_text");
        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(content[1]["image_url"], "https://x/y.png");
    }

    #[test]
    fn tool_results_become_function_call_output() {
        let mut req = base_request();
        req.messages = vec![Message {
            role: Role::Tool,
            parts: vec![ContentPart::Text("42".into())],
            tool_call_id: Some("call_1".into()),
            tool_calls: vec![],
        }];
        let item = build_request(&req)["input"][0].clone();
        assert_eq!(item["type"], "function_call_output");
        assert_eq!(item["call_id"], "call_1");
        assert_eq!(item["output"], "42");
    }

    #[test]
    fn assistant_tool_calls_are_preserved() {
        let mut req = base_request();
        req.messages = vec![Message {
            role: Role::Assistant,
            parts: vec![ContentPart::Text("voy a mirar".into())],
            tool_call_id: None,
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "buscar".into(),
                arguments_json: "{\"q\":\"a\"}".into(),
            }],
        }];
        let input = build_request(&req)["input"].clone();
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_1");
    }

    #[test]
    fn tools_and_reasoning_are_mapped() {
        let mut req = base_request();
        req.tools = vec![ToolDef {
            name: "buscar".into(),
            description: Some("busca".into()),
            parameters: json!({"type": "object"}),
        }];
        req.tool_choice = ToolChoice::Named("buscar".into());
        req.reasoning = Some(ReasoningEffort::High);
        req.max_output_tokens = Some(1024);
        let body = build_request(&req);
        assert_eq!(body["tools"][0]["name"], "buscar");
        assert_eq!(body["tool_choice"]["name"], "buscar");
        assert_eq!(body["reasoning"]["effort"], "high");
        assert!(body.get("max_output_tokens").is_none());
    }

    #[test]
    fn unsupported_sampling_params_are_omitted_not_faked() {
        let mut req = base_request();
        req.temperature = Some(0.7);
        req.top_p = Some(0.9);
        req.stop = vec!["FIN".into()];
        let body = build_request(&req);
        assert!(body.get("temperature").is_none());
        assert!(body.get("top_p").is_none());
        assert!(body.get("stop").is_none());
    }

    fn events(name: &str, data: Value) -> Vec<ChatEvent> {
        match translate_event(name, &data) {
            Translated::Events(e) => e,
            Translated::Ignored => vec![],
            Translated::Failure(e) => panic!("fallo inesperado: {e}"),
        }
    }

    #[test]
    fn created_marks_start() {
        let e = events("response.created", json!({"response": {"id": "resp_1"}}));
        match &e[0] {
            ChatEvent::Started { provider_request_id } => {
                assert_eq!(provider_request_id.as_deref(), Some("resp_1"));
            }
            other => panic!("esperaba Started, llegó {other:?}"),
        }
    }

    #[test]
    fn text_delta_translates() {
        let e = events("response.output_text.delta", json!({"delta": "ho"}));
        match &e[0] {
            ChatEvent::TextDelta { text } => assert_eq!(text, "ho"),
            other => panic!("esperaba TextDelta, llegó {other:?}"),
        }
    }

    #[test]
    fn empty_delta_is_ignored() {
        assert!(events("response.output_text.delta", json!({"delta": ""})).is_empty());
    }

    #[test]
    fn reasoning_delta_is_kept_separate_from_text() {
        let e = events("response.reasoning_summary_text.delta", json!({"delta": "pienso"}));
        assert!(matches!(e[0], ChatEvent::ReasoningDelta { .. }));
    }

    #[test]
    fn type_in_payload_works_without_event_name() {
        let e = events("", json!({"type": "response.output_text.delta", "delta": "x"}));
        assert!(matches!(e[0], ChatEvent::TextDelta { .. }));
    }

    #[test]
    fn tool_call_lifecycle_translates() {
        let start = events(
            "response.output_item.added",
            json!({"item": {"type": "function_call", "call_id": "call_1", "name": "buscar"}}),
        );
        match &start[0] {
            ChatEvent::ToolCallStart { id, name } => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "buscar");
            }
            other => panic!("esperaba ToolCallStart, llegó {other:?}"),
        }

        let delta = events(
            "response.function_call_arguments.delta",
            json!({"item_id": "call_1", "delta": "{\"q\""}),
        );
        assert!(matches!(delta[0], ChatEvent::ToolCallDelta { .. }));

        let end = events(
            "response.function_call_arguments.done",
            json!({"item_id": "call_1"}),
        );
        assert!(matches!(end[0], ChatEvent::ToolCallEnd { .. }));
    }

    #[test]
    fn output_item_done_preserves_complete_tool_arguments() {
        let events = events(
            "response.output_item.done",
            json!({"item": {
                "type": "function_call",
                "call_id": "call_1",
                "name": "write_file",
                "arguments": "{\"path\":\"result.txt\",\"content\":\"ok\"}"
            }}),
        );

        assert!(events.iter().any(|event| matches!(
            event,
            ChatEvent::ToolCallArgumentsDone { id, args_json }
                if id == "call_1" && args_json == "{\"path\":\"result.txt\",\"content\":\"ok\"}"
        )));
    }

    #[test]
    fn non_function_output_items_are_ignored() {
        assert!(events(
            "response.output_item.added",
            json!({"item": {"type": "message"}})
        )
        .is_empty());
    }

    #[test]
    fn completed_without_usage_reports_unavailable_not_zero() {
        let e = events("response.completed", json!({"response": {"output": []}}));
        match &e[0] {
            ChatEvent::Usage(u) => {
                assert_eq!(u.source, UsageSource::Unavailable);
                assert_eq!(u.input_tokens, None);
                assert_eq!(u.total_tokens(), None);
            }
            other => panic!("esperaba Usage, llegó {other:?}"),
        }
        assert!(matches!(e[1], ChatEvent::Finished { reason: FinishReason::Stop }));
    }

    #[test]
    fn completed_with_usage_is_reported_and_keeps_raw() {
        let e = events(
            "response.completed",
            json!({"response": {"output": [], "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "input_tokens_details": {"cached_tokens": 4},
                "output_tokens_details": {"reasoning_tokens": 3}
            }}}),
        );
        match &e[0] {
            ChatEvent::Usage(u) => {
                assert_eq!(u.source, UsageSource::Reported);
                assert_eq!(u.input_tokens, Some(10));
                assert_eq!(u.cached_input_tokens, Some(4));
                assert_eq!(u.reasoning_tokens, Some(3));
                assert_eq!(u.total_tokens(), Some(15));
                assert!(u.raw.is_some(), "el objeto original debe conservarse");
            }
            other => panic!("esperaba Usage, llegó {other:?}"),
        }
    }

    #[test]
    fn completed_with_function_call_finishes_as_tool_calls() {
        let e = events(
            "response.completed",
            json!({"response": {"output": [{"type": "function_call"}]}}),
        );
        assert!(matches!(
            e[1],
            ChatEvent::Finished { reason: FinishReason::ToolCalls }
        ));
    }

    #[test]
    fn incomplete_by_max_tokens_finishes_as_length() {
        let e = events(
            "response.incomplete",
            json!({"response": {"incomplete_details": {"reason": "max_output_tokens"}}}),
        );
        assert!(matches!(
            e[1],
            ChatEvent::Finished { reason: FinishReason::Length }
        ));
    }

    #[test]
    fn failure_becomes_upstream_error() {
        let t = translate_event(
            "response.failed",
            &json!({"response": {"error": {"code": "server_error", "message": "boom"}}}),
        );
        match t {
            Translated::Failure(AdapterError::Upstream { message, provider_code, .. }) => {
                assert_eq!(message, "boom");
                assert_eq!(provider_code.as_deref(), Some("server_error"));
            }
            _ => panic!("esperaba Failure"),
        }
    }

    #[test]
    fn unknown_events_are_ignored() {
        assert!(matches!(
            translate_event("response.something.new", &json!({})),
            Translated::Ignored
        ));
    }

    #[test]
    fn realistic_openai_responses_tool_call_sequence_maintains_consistent_id_and_chunk_index() {
        use crate::gateway::wire::ChunkBuilder;

        // Secuencia realista de OpenAI Responses API:
        // 1. response.output_item.added -> item.id = "fc_123", item.call_id = "call_123"
        // 2. response.function_call_arguments.delta -> item_id = "fc_123", delta = "{\""
        // 3. response.function_call_arguments.delta -> item_id = "fc_123", delta = "path\":\"img.png\"}"
        // 4. response.function_call_arguments.done -> item_id = "fc_123"
        // 5. response.output_item.done -> item.id = "fc_123", item.call_id = "call_123", item.arguments = "{\"path\":\"img.png\"}"

        let raw_events = vec![
            (
                "response.output_item.added",
                json!({
                    "item": {
                        "id": "fc_123",
                        "type": "function_call",
                        "call_id": "call_123",
                        "name": "__MSTY_attachments_read"
                    }
                }),
            ),
            (
                "response.function_call_arguments.delta",
                json!({
                    "item_id": "fc_123",
                    "delta": "{\""
                }),
            ),
            (
                "response.function_call_arguments.delta",
                json!({
                    "item_id": "fc_123",
                    "delta": "path\":\"img.png\"}"
                }),
            ),
            (
                "response.function_call_arguments.done",
                json!({
                    "item_id": "fc_123"
                }),
            ),
            (
                "response.output_item.done",
                json!({
                    "item": {
                        "id": "fc_123",
                        "type": "function_call",
                        "call_id": "call_123",
                        "name": "__MSTY_attachments_read",
                        "arguments": "{\"path\":\"img.png\"}"
                    }
                }),
            ),
        ];

        let mut builder = ChunkBuilder::new("gpt-4o");
        let mut translator = ResponsesEventTranslator::new();
        let mut chunks = Vec::new();

        for (event_name, data) in raw_events {
            if let Translated::Events(evs) = translator.translate_event(event_name, &data) {
                for ev in evs {
                    match ev {
                        ChatEvent::ToolCallStart { id, name } => {
                            chunks.push(builder.tool_start_chunk(&id, &name));
                        }
                        ChatEvent::ToolCallDelta { id, args_json } => {
                            chunks.push(builder.tool_args_chunk(&id, &args_json));
                        }
                        ChatEvent::ToolCallArgumentsDone { id, args_json } => {
                            if let Some(chunk) = builder.tool_final_args_chunk(&id, &args_json) {
                                chunks.push(chunk);
                            }
                        }
                        ChatEvent::ToolCallEnd { .. } => {}
                        _ => {}
                    }
                }
            }
        }

        // Cada chunk emitido para esta llamada a herramienta DEBE tener index: 0
        for (i, chunk) in chunks.iter().enumerate() {
            let tool_calls = chunk["choices"][0]["delta"]["tool_calls"].as_array().unwrap();
            let index = tool_calls[0]["index"].as_u64().unwrap();
            assert_eq!(
                index, 0,
                "El chunk #{i} tiene index {index}, pero todos deben tener index 0. Chunk: {chunk}"
            );
        }

        // El primer chunk debe tener el id público "call_123"
        let first_call = &chunks[0]["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(first_call["id"], "call_123");
        assert_eq!(first_call["function"]["name"], "__MSTY_attachments_read");
    }

    #[test]
    fn multiple_tool_calls_maintain_distinct_indices_and_correlated_deltas() {
        use crate::gateway::wire::ChunkBuilder;

        let raw_events = vec![
            // Llamada 1
            (
                "response.output_item.added",
                json!({
                    "item": {
                        "id": "fc_1",
                        "type": "function_call",
                        "call_id": "call_1",
                        "name": "get_weather"
                    }
                }),
            ),
            (
                "response.function_call_arguments.delta",
                json!({"item_id": "fc_1", "delta": "{\"city\":"}),
            ),
            (
                "response.function_call_arguments.delta",
                json!({"item_id": "fc_1", "delta": "\"Madrid\"}"}),
            ),
            (
                "response.function_call_arguments.done",
                json!({"item_id": "fc_1"}),
            ),
            // Llamada 2 (concurrente / secuencial en el mismo turno)
            (
                "response.output_item.added",
                json!({
                    "item": {
                        "id": "fc_2",
                        "type": "function_call",
                        "call_id": "call_2",
                        "name": "get_time"
                    }
                }),
            ),
            (
                "response.function_call_arguments.delta",
                json!({"item_id": "fc_2", "delta": "{\"tz\":"}),
            ),
            (
                "response.function_call_arguments.delta",
                json!({"item_id": "fc_2", "delta": "\"UTC\"}"}),
            ),
            (
                "response.function_call_arguments.done",
                json!({"item_id": "fc_2"}),
            ),
        ];

        let mut builder = ChunkBuilder::new("gpt-4o");
        let mut translator = ResponsesEventTranslator::new();
        let mut chunks_call_1 = Vec::new();
        let mut chunks_call_2 = Vec::new();

        for (event_name, data) in raw_events {
            if let Translated::Events(evs) = translator.translate_event(event_name, &data) {
                for ev in evs {
                    match ev {
                        ChatEvent::ToolCallStart { id, name } => {
                            let chunk = builder.tool_start_chunk(&id, &name);
                            if id == "call_1" {
                                chunks_call_1.push(chunk);
                            } else {
                                chunks_call_2.push(chunk);
                            }
                        }
                        ChatEvent::ToolCallDelta { id, args_json } => {
                            let chunk = builder.tool_args_chunk(&id, &args_json);
                            if id == "call_1" {
                                chunks_call_1.push(chunk);
                            } else {
                                chunks_call_2.push(chunk);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Llamada 1 debe tener 3 chunks, todos con index 0
        assert_eq!(chunks_call_1.len(), 3);
        for chunk in &chunks_call_1 {
            let tc = chunk["choices"][0]["delta"]["tool_calls"].as_array().unwrap();
            assert_eq!(tc[0]["index"], 0);
        }
        assert_eq!(chunks_call_1[0]["choices"][0]["delta"]["tool_calls"][0]["id"], "call_1");

        // Llamada 2 debe tener 3 chunks, todos con index 1
        assert_eq!(chunks_call_2.len(), 3);
        for chunk in &chunks_call_2 {
            let tc = chunk["choices"][0]["delta"]["tool_calls"].as_array().unwrap();
            assert_eq!(tc[0]["index"], 1);
        }
        assert_eq!(chunks_call_2[0]["choices"][0]["delta"]["tool_calls"][0]["id"], "call_2");
    }

    #[test]
    fn tool_call_fallback_when_call_id_is_missing_or_identical() {
        use crate::gateway::wire::ChunkBuilder;

        // Caso sin call_id explícito
        let mut translator = ResponsesEventTranslator::new();
        let mut builder = ChunkBuilder::new("gpt-4o");

        let start = translator.translate_event(
            "response.output_item.added",
            &json!({"item": {"id": "fc_fallback", "type": "function_call", "name": "test"}}),
        );
        let mut chunks = Vec::new();
        if let Translated::Events(evs) = start {
            for ev in evs {
                if let ChatEvent::ToolCallStart { id, name } = ev {
                    chunks.push(builder.tool_start_chunk(&id, &name));
                }
            }
        }

        let delta = translator.translate_event(
            "response.function_call_arguments.delta",
            &json!({"item_id": "fc_fallback", "delta": "{}"}),
        );
        if let Translated::Events(evs) = delta {
            for ev in evs {
                if let ChatEvent::ToolCallDelta { id, args_json } = ev {
                    chunks.push(builder.tool_args_chunk(&id, &args_json));
                }
            }
        }

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0]["choices"][0]["delta"]["tool_calls"][0]["index"], 0);
        assert_eq!(chunks[0]["choices"][0]["delta"]["tool_calls"][0]["id"], "fc_fallback");
        assert_eq!(chunks[1]["choices"][0]["delta"]["tool_calls"][0]["index"], 0);
    }
}
