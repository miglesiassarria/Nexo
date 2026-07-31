//! Traducción del formato `chat/completions` de OpenAI.
//!
//! La comparten todos los proveedores que exponen esa superficie: la API pública
//! de OpenAI, LM Studio y cualquier servidor local compatible. Vive aquí y no en
//! un adaptador porque duplicarla garantizaría que las dos copias se separasen.

use crate::provider::{
    AdapterError, ChatEvent, ChatRequest, ContentPart, EventStream, FinishReason, Role, ToolChoice,
    UsageReport, UsageSource,
};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::{json, Value};

/// Construye el cuerpo de una petición `chat/completions` en streaming.
pub fn build_request(req: &ChatRequest) -> Value {
    let messages: Vec<Value> = req.messages.iter().map(message_to_wire).collect();

    let mut body = serde_json::Map::new();
    body.insert("model".into(), json!(req.api_model));
    body.insert("messages".into(), json!(messages));
    body.insert("stream".into(), json!(true));
    // Verificado también en LM Studio 0.4.20: manda el chunk de uso al final.
    body.insert("stream_options".into(), json!({"include_usage": true}));

    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
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
                ToolChoice::Named(n) => json!({"type": "function", "function": {"name": n}}),
            },
        );
    }

    if let Some(max) = req.max_output_tokens {
        body.insert("max_completion_tokens".into(), json!(max));
    }
    if let Some(t) = req.temperature {
        body.insert("temperature".into(), json!(t));
    }
    if let Some(p) = req.top_p {
        body.insert("top_p".into(), json!(p));
    }
    if !req.stop.is_empty() {
        body.insert("stop".into(), json!(req.stop));
    }
    if req.json_mode {
        body.insert("response_format".into(), json!({"type": "json_object"}));
    }
    if let Some(effort) = req.reasoning {
        body.insert("reasoning_effort".into(), json!(effort.as_str()));
    }

    Value::Object(body)
}

fn message_to_wire(m: &crate::provider::Message) -> Value {
    let role = match m.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    let mut obj = serde_json::Map::new();
    obj.insert("role".into(), json!(role));

    if m.role == Role::Tool {
        obj.insert("content".into(), json!(m.text()));
        obj.insert("tool_call_id".into(), json!(m.tool_call_id));
        return Value::Object(obj);
    }

    let has_media = m.parts.iter().any(|p| !matches!(p, ContentPart::Text(_)));
    if has_media {
        let parts: Vec<Value> = m
            .parts
            .iter()
            .map(|p| match p {
                ContentPart::Text(t) => json!({"type": "text", "text": t}),
                ContentPart::ImageUrl(u) => {
                    json!({"type": "image_url", "image_url": {"url": u}})
                }
                ContentPart::Audio { mime, base64 } => json!({
                    "type": "input_audio",
                    "input_audio": {"data": base64, "format": mime}
                }),
                ContentPart::File { name, mime, base64 } => json!({
                    "type": "file",
                    "file": {
                        "filename": name,
                        "file_data": format!("data:{mime};base64,{base64}")
                    }
                }),
            })
            .collect();
        obj.insert("content".into(), json!(parts));
    } else {
        obj.insert("content".into(), json!(m.text()));
    }

    if !m.tool_calls.is_empty() {
        let calls: Vec<Value> = m
            .tool_calls
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": {"name": c.name, "arguments": c.arguments_json}
                })
            })
            .collect();
        obj.insert("tool_calls".into(), json!(calls));
    }

    Value::Object(obj)
}

/// Traduce un chunk de `chat/completions` al vocabulario interno.
pub fn translate_chunk(chunk: &Value) -> Vec<ChatEvent> {
    let mut events = Vec::new();

    if let Some(usage) = chunk.get("usage").filter(|u| !u.is_null()) {
        events.push(ChatEvent::Usage(UsageReport {
            input_tokens: usage
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            output_tokens: usage
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            cached_input_tokens: usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            reasoning_tokens: usage
                .pointer("/completion_tokens_details/reasoning_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            source: UsageSource::Reported,
            raw: Some(usage.clone()),
        }));
    }

    let Some(choice) = chunk.pointer("/choices/0") else {
        return events;
    };

    if let Some(text) = choice
        .pointer("/delta/content")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        events.push(ChatEvent::TextDelta { text: text.to_string() });
    }

    if let Some(text) = choice
        .pointer("/delta/reasoning_content")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        events.push(ChatEvent::ReasoningDelta { text: text.to_string() });
    }

    if let Some(calls) = choice.pointer("/delta/tool_calls").and_then(|v| v.as_array()) {
        for call in calls {
            let id = call
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    format!(
                        "call_{}",
                        call.get("index").and_then(|v| v.as_u64()).unwrap_or(0)
                    )
                });
            if let Some(name) = call.pointer("/function/name").and_then(|v| v.as_str()) {
                events.push(ChatEvent::ToolCallStart { id: id.clone(), name: name.to_string() });
            }
            if let Some(args) = call
                .pointer("/function/arguments")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                events.push(ChatEvent::ToolCallDelta {
                    id: id.clone(),
                    args_json: args.to_string(),
                });
            }
        }
    }

    if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
        events.push(ChatEvent::Finished { reason: finish_reason(reason) });
    }

    events
}

fn finish_reason(raw: &str) -> FinishReason {
    match raw {
        "length" => FinishReason::Length,
        "tool_calls" | "function_call" => FinishReason::ToolCalls,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Stop,
    }
}

/// Convierte una respuesta SSE de `chat/completions` en el stream interno.
///
/// Emite `Started` en el primer chunk, para que el tiempo hasta el primer token se
/// mida desde que el proveedor empieza a hablar y no desde que se abrió la conexión.
pub fn stream_from_response(resp: reqwest::Response) -> EventStream {
    let mut started = false;
    let stream = resp.bytes_stream().eventsource().flat_map(move |item| {
        let out: Vec<Result<ChatEvent, AdapterError>> = match item {
            Err(e) => vec![Err(AdapterError::Transport { detail: e.to_string() })],
            Ok(event) => {
                if event.data.trim() == "[DONE]" {
                    vec![]
                } else {
                    match serde_json::from_str::<Value>(&event.data) {
                        Err(e) => vec![Err(AdapterError::Malformed {
                            detail: format!("chunk con json inválido: {e}"),
                        })],
                        Ok(chunk) => {
                            let mut evs = Vec::new();
                            if !started {
                                started = true;
                                evs.push(Ok(ChatEvent::Started {
                                    provider_request_id: chunk
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .map(str::to_string),
                                }));
                            }
                            evs.extend(translate_chunk(&chunk).into_iter().map(Ok));
                            evs
                        }
                    }
                }
            }
        };
        futures::stream::iter(out)
    });
    Box::pin(stream)
}

/// Clasificación común de errores HTTP de un servidor `chat/completions`.
pub fn classify_http_error(
    status: u16,
    retry_after: Option<std::time::Duration>,
    body: &str,
) -> AdapterError {
    match status {
        401 | 403 => AdapterError::Auth {
            reason: "la credencial fue rechazada".into(),
            reauth_required: true,
        },
        429 => AdapterError::RateLimited { retry_after },
        s => AdapterError::Upstream {
            status: s,
            provider_code: None,
            message: body.chars().take(300).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Message, ToolDef};

    fn req() -> ChatRequest {
        ChatRequest {
            api_model: "gpt-5.5".into(),
            public_model: "openai/gpt-5.5".into(),
            messages: vec![Message {
                role: Role::User,
                parts: vec![ContentPart::Text("hola".into())],
                tool_call_id: None,
                tool_calls: vec![],
            }],
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

    #[test]
    fn plain_text_uses_string_content() {
        let body = build_request(&req());
        assert_eq!(body["messages"][0]["content"], "hola");
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn media_switches_to_parts_array() {
        let mut r = req();
        r.messages[0]
            .parts
            .push(ContentPart::ImageUrl("https://x/y.png".into()));
        let body = build_request(&r);
        assert!(body["messages"][0]["content"].is_array());
        assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
    }

    #[test]
    fn sampling_params_are_passed_through() {
        let mut r = req();
        r.temperature = Some(0.3);
        r.top_p = Some(0.8);
        r.stop = vec!["FIN".into()];
        let body = build_request(&r);
        assert!((body["temperature"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert!((body["top_p"].as_f64().unwrap() - 0.8).abs() < 1e-6);
        assert_eq!(body["stop"][0], "FIN");
    }

    #[test]
    fn tools_are_wrapped_in_function_envelope() {
        let mut r = req();
        r.tools = vec![ToolDef {
            name: "buscar".into(),
            description: None,
            parameters: json!({"type": "object"}),
        }];
        let body = build_request(&r);
        assert_eq!(body["tools"][0]["function"]["name"], "buscar");
    }

    #[test]
    fn tool_messages_carry_their_call_id() {
        let mut r = req();
        r.messages = vec![Message {
            role: Role::Tool,
            parts: vec![ContentPart::Text("42".into())],
            tool_call_id: Some("call_1".into()),
            tool_calls: vec![],
        }];
        let body = build_request(&r);
        assert_eq!(body["messages"][0]["tool_call_id"], "call_1");
        assert_eq!(body["messages"][0]["content"], "42");
    }

    #[test]
    fn text_delta_is_translated() {
        let chunk = json!({"choices": [{"delta": {"content": "ho"}}]});
        match &translate_chunk(&chunk)[0] {
            ChatEvent::TextDelta { text } => assert_eq!(text, "ho"),
            other => panic!("esperaba TextDelta, llegó {other:?}"),
        }
    }

    #[test]
    fn usage_chunk_is_reported_and_keeps_raw() {
        let chunk = json!({
            "choices": [],
            "usage": {
                "prompt_tokens": 12,
                "completion_tokens": 8,
                "prompt_tokens_details": {"cached_tokens": 4}
            }
        });
        match &translate_chunk(&chunk)[0] {
            ChatEvent::Usage(u) => {
                assert_eq!(u.source, UsageSource::Reported);
                assert_eq!(u.total_tokens(), Some(20));
                assert_eq!(u.cached_input_tokens, Some(4));
                assert!(u.raw.is_some());
            }
            other => panic!("esperaba Usage, llegó {other:?}"),
        }
    }

    /// Forma real capturada de LM Studio 0.4.20 el 2026-07-31.
    #[test]
    fn lm_studio_usage_shape_is_understood() {
        let chunk = json!({
            "choices": [],
            "usage": {
                "prompt_tokens": 21,
                "completion_tokens": 8,
                "total_tokens": 29,
                "completion_tokens_details": {"reasoning_tokens": 0}
            }
        });
        match &translate_chunk(&chunk)[0] {
            ChatEvent::Usage(u) => {
                assert_eq!(u.input_tokens, Some(21));
                assert_eq!(u.output_tokens, Some(8));
                assert_eq!(u.reasoning_tokens, Some(0));
                assert_eq!(u.source, UsageSource::Reported);
            }
            other => panic!("esperaba Usage, llegó {other:?}"),
        }
    }

    #[test]
    fn finish_reason_is_mapped() {
        let chunk = json!({"choices": [{"delta": {}, "finish_reason": "length"}]});
        assert!(matches!(
            translate_chunk(&chunk)[0],
            ChatEvent::Finished { reason: FinishReason::Length }
        ));
    }

    #[test]
    fn tool_call_deltas_reuse_index_when_id_absent() {
        let chunk = json!({"choices": [{"delta": {"tool_calls": [
            {"index": 0, "function": {"arguments": "{\"a\""}}
        ]}}]});
        match &translate_chunk(&chunk)[0] {
            ChatEvent::ToolCallDelta { id, .. } => assert_eq!(id, "call_0"),
            other => panic!("esperaba ToolCallDelta, llegó {other:?}"),
        }
    }

    #[test]
    fn empty_chunk_yields_nothing() {
        assert!(translate_chunk(&json!({"choices": [{"delta": {}}]})).is_empty());
    }

    #[test]
    fn http_errors_are_classified() {
        assert_eq!(classify_http_error(401, None, "").http_status(), 401);
        assert_eq!(classify_http_error(429, None, "").kind_str(), "rate_limited");
        assert_eq!(classify_http_error(503, None, "").kind_str(), "upstream");
    }
}
