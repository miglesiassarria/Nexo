//! Adaptador de OpenAI por API key contra `api.openai.com`.
//!
//! Es la vía estable y el respaldo de la ruta de suscripción. Habla
//! `chat/completions`, así que la traducción es casi la identidad.

use crate::catalog;
use crate::provider::{
    check_capabilities, AdapterError, AdapterId, ChatEvent, ChatRequest, ContentPart,
    CredentialKind, EventStream, FinishReason, Health, ModelDescriptor, ProviderAdapter,
    ResolvedCredential, Role, ToolChoice, UsageReport, UsageSource,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::{json, Value};

pub const PROVIDER: &str = "openai";
const BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAiApiKeyAdapter {
    http: reqwest::Client,
    base_url: String,
}

impl OpenAiApiKeyAdapter {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http, base_url: BASE_URL.to_string() }
    }

    pub fn with_base_url(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self { http, base_url: base_url.into() }
    }
}

#[async_trait]
impl ProviderAdapter for OpenAiApiKeyAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::new(PROVIDER, CredentialKind::ApiKey)
    }

    async fn catalog(
        &self,
        cred: &ResolvedCredential,
    ) -> Result<Vec<ModelDescriptor>, AdapterError> {
        // El proveedor anuncia qué modelos existen, pero no sus capacidades:
        // esas salen del manifiesto. Se cruzan las dos fuentes.
        let manifest = catalog::openai_apikey_models();

        let resp = self
            .http
            .get(format!("{}/models", self.base_url))
            .header("authorization", format!("Bearer {}", cred.secret))
            .send()
            .await
            .map_err(AdapterError::from_reqwest)?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            if status == 401 || status == 403 {
                return Err(AdapterError::Auth {
                    reason: "la API key fue rechazada".into(),
                    reauth_required: true,
                });
            }
            // El catálogo del manifiesto sigue siendo utilizable.
            tracing::warn!(status, "no se pudo listar modelos; se usa el manifiesto");
            return Ok(manifest);
        }

        let body: Value = resp.json().await.map_err(AdapterError::from_reqwest)?;
        let announced: Vec<String> = body
            .get("data")
            .and_then(|d| d.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|m| m.get("id").and_then(|v| v.as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        if announced.is_empty() {
            return Ok(manifest);
        }

        Ok(manifest
            .into_iter()
            .filter(|m| announced.contains(&m.api_id))
            .collect())
    }

    async fn stream(
        &self,
        req: &ChatRequest,
        cred: &ResolvedCredential,
    ) -> Result<EventStream, AdapterError> {
        let model = catalog::openai_apikey_models()
            .into_iter()
            .find(|m| m.api_id == req.api_model)
            .ok_or_else(|| AdapterError::Unsupported {
                capability: "model".into(),
                hint: Some(format!("{} no está en el manifiesto", req.api_model)),
            })?;

        check_capabilities(req, &model)?;

        let body = build_chat_completions(req);

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .header("authorization", format!("Bearer {}", cred.secret))
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(AdapterError::from_reqwest)?;

        let status = resp.status();
        if !status.is_success() {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(std::time::Duration::from_secs);
            let text = resp.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                401 | 403 => AdapterError::Auth {
                    reason: "la API key fue rechazada".into(),
                    reauth_required: true,
                },
                429 => AdapterError::RateLimited { retry_after },
                s => AdapterError::Upstream {
                    status: s,
                    provider_code: None,
                    message: text.chars().take(300).collect(),
                },
            });
        }

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

        Ok(Box::pin(stream))
    }

    async fn health(&self, cred: &ResolvedCredential) -> Health {
        // `GET /models` no consume cuota facturable.
        match self
            .http
            .get(format!("{}/models", self.base_url))
            .header("authorization", format!("Bearer {}", cred.secret))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => Health::Ok,
            Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => Health::Down,
            Ok(_) => Health::Degraded,
            Err(_) => Health::Down,
        }
    }
}

fn build_chat_completions(req: &ChatRequest) -> Value {
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
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

            let has_media = m
                .parts
                .iter()
                .any(|p| !matches!(p, ContentPart::Text(_)));
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
        })
        .collect();

    let mut body = serde_json::Map::new();
    body.insert("model".into(), json!(req.api_model));
    body.insert("messages".into(), json!(messages));
    body.insert("stream".into(), json!(true));
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
                ToolChoice::Named(n) => {
                    json!({"type": "function", "function": {"name": n}})
                }
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

fn translate_chunk(chunk: &Value) -> Vec<ChatEvent> {
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
                    format!("call_{}", call.get("index").and_then(|v| v.as_u64()).unwrap_or(0))
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
        let reason = match reason {
            "length" => FinishReason::Length,
            "tool_calls" | "function_call" => FinishReason::ToolCalls,
            "content_filter" => FinishReason::ContentFilter,
            _ => FinishReason::Stop,
        };
        events.push(ChatEvent::Finished { reason });
    }

    events
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
        let body = build_chat_completions(&req());
        assert_eq!(body["messages"][0]["content"], "hola");
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn media_switches_to_parts_array() {
        let mut r = req();
        r.messages[0]
            .parts
            .push(ContentPart::ImageUrl("https://x/y.png".into()));
        let body = build_chat_completions(&r);
        assert!(body["messages"][0]["content"].is_array());
        assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
    }

    #[test]
    fn sampling_params_are_passed_through_on_this_route() {
        let mut r = req();
        r.temperature = Some(0.3);
        r.top_p = Some(0.8);
        r.stop = vec!["FIN".into()];
        let body = build_chat_completions(&r);
        // Comparación por tolerancia: el valor viaja como f32 y se serializa
        // a f64, así que la igualdad exacta no aplica.
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
        let body = build_chat_completions(&r);
        assert_eq!(body["tools"][0]["function"]["name"], "buscar");
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
}
