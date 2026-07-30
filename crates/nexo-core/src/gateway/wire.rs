//! Formato de cable de la API pública: compatible con OpenAI
//! `chat/completions`.
//!
//! Traduce entre el JSON que envían los clientes y la representación interna,
//! y de vuelta a chunks de `chat/completions`.

use crate::provider::{
    AdapterError, ChatRequest, ContentPart, FinishReason, Message, ReasoningEffort, Role, ToolCall,
    ToolChoice, ToolDef, UsageReport,
};
use crate::util;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub struct WireChatRequest {
    pub model: String,
    pub messages: Vec<WireMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub tools: Option<Vec<WireTool>>,
    #[serde(default)]
    pub tool_choice: Option<Value>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub max_completion_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stop: Option<Value>,
    #[serde(default)]
    pub response_format: Option<Value>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WireMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
pub struct WireTool {
    #[serde(default)]
    pub function: Option<WireFunction>,
}

#[derive(Debug, Deserialize)]
pub struct WireFunction {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parameters: Option<Value>,
}

impl WireChatRequest {
    pub fn into_internal(self, api_model: String, public_model: String) -> Result<ChatRequest, String> {
        if self.messages.is_empty() {
            return Err("`messages` no puede estar vacío".into());
        }

        let messages = self
            .messages
            .into_iter()
            .map(|m| m.into_internal())
            .collect::<Result<Vec<_>, String>>()?;

        let tools = self
            .tools
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| t.function)
            .map(|f| ToolDef {
                name: f.name,
                description: f.description,
                parameters: f.parameters.unwrap_or_else(|| json!({"type": "object"})),
            })
            .collect();

        let tool_choice = match &self.tool_choice {
            None => ToolChoice::Auto,
            Some(Value::String(s)) => match s.as_str() {
                "none" => ToolChoice::None,
                "required" => ToolChoice::Required,
                _ => ToolChoice::Auto,
            },
            Some(Value::Object(o)) => o
                .get("function")
                .and_then(|f| f.get("name"))
                .or_else(|| o.get("name"))
                .and_then(|v| v.as_str())
                .map(|n| ToolChoice::Named(n.to_string()))
                .unwrap_or(ToolChoice::Auto),
            Some(_) => ToolChoice::Auto,
        };

        let stop = match self.stop {
            Some(Value::String(s)) => vec![s],
            Some(Value::Array(a)) => a
                .into_iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => vec![],
        };

        let json_mode = self
            .response_format
            .as_ref()
            .and_then(|f| f.get("type"))
            .and_then(|v| v.as_str())
            .map(|t| t == "json_object" || t == "json_schema")
            .unwrap_or(false);

        let reasoning = self.reasoning_effort.as_deref().and_then(|e| match e {
            "minimal" => Some(ReasoningEffort::Minimal),
            "low" => Some(ReasoningEffort::Low),
            "medium" => Some(ReasoningEffort::Medium),
            "high" => Some(ReasoningEffort::High),
            _ => None,
        });

        Ok(ChatRequest {
            api_model,
            public_model,
            messages,
            tools,
            tool_choice,
            reasoning,
            max_output_tokens: self.max_completion_tokens.or(self.max_tokens),
            temperature: self.temperature,
            top_p: self.top_p,
            stop,
            json_mode,
            stream: self.stream,
        })
    }
}

impl WireMessage {
    fn into_internal(self) -> Result<Message, String> {
        let role = match self.role.as_str() {
            "system" | "developer" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool" | "function" => Role::Tool,
            other => return Err(format!("rol desconocido: {other}")),
        };

        let parts = match self.content {
            None | Some(Value::Null) => vec![],
            Some(Value::String(s)) => vec![ContentPart::Text(s)],
            Some(Value::Array(items)) => items
                .into_iter()
                .filter_map(|item| {
                    let kind = item.get("type").and_then(|v| v.as_str()).unwrap_or("text");
                    match kind {
                        "text" | "input_text" => item
                            .get("text")
                            .and_then(|v| v.as_str())
                            .map(|t| ContentPart::Text(t.to_string())),
                        "image_url" => item
                            .pointer("/image_url/url")
                            .or_else(|| item.get("image_url"))
                            .and_then(|v| v.as_str())
                            .map(|u| ContentPart::ImageUrl(u.to_string())),
                        "input_audio" => {
                            let data = item.pointer("/input_audio/data")?.as_str()?;
                            let format = item
                                .pointer("/input_audio/format")
                                .and_then(|v| v.as_str())
                                .unwrap_or("wav");
                            Some(ContentPart::Audio {
                                mime: format.to_string(),
                                base64: data.to_string(),
                            })
                        }
                        "file" => {
                            let data = item.pointer("/file/file_data")?.as_str()?;
                            let name = item
                                .pointer("/file/filename")
                                .and_then(|v| v.as_str())
                                .unwrap_or("archivo");
                            Some(ContentPart::File {
                                name: name.to_string(),
                                mime: "application/octet-stream".into(),
                                base64: data.to_string(),
                            })
                        }
                        _ => None,
                    }
                })
                .collect(),
            Some(other) => vec![ContentPart::Text(other.to_string())],
        };

        let tool_calls = self
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .filter_map(|c| {
                Some(ToolCall {
                    id: c.get("id")?.as_str()?.to_string(),
                    name: c.pointer("/function/name")?.as_str()?.to_string(),
                    arguments_json: c
                        .pointer("/function/arguments")
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}")
                        .to_string(),
                })
            })
            .collect();

        Ok(Message { role, parts, tool_call_id: self.tool_call_id, tool_calls })
    }
}

// -- Salida ----------------------------------------------------------------

/// Acumula eventos internos y produce chunks de `chat/completions`.
pub struct ChunkBuilder {
    pub id: String,
    pub model: String,
    created: i64,
    tool_index: HashMapIndex,
}

type HashMapIndex = std::collections::HashMap<String, usize>;

impl ChunkBuilder {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            id: format!("chatcmpl-{}", util::new_id("nx")),
            model: model.into(),
            created: util::now_ms() / 1000,
            tool_index: HashMapIndex::new(),
        }
    }

    fn envelope(&self, delta: Value, finish_reason: Option<&str>) -> Value {
        json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason,
            }],
        })
    }

    pub fn role_chunk(&self) -> Value {
        self.envelope(json!({"role": "assistant", "content": ""}), None)
    }

    pub fn text_chunk(&self, text: &str) -> Value {
        self.envelope(json!({"content": text}), None)
    }

    pub fn reasoning_chunk(&self, text: &str) -> Value {
        self.envelope(json!({"reasoning_content": text}), None)
    }

    pub fn tool_start_chunk(&mut self, id: &str, name: &str) -> Value {
        let next = self.tool_index.len();
        let index = *self.tool_index.entry(id.to_string()).or_insert(next);
        self.envelope(
            json!({"tool_calls": [{
                "index": index,
                "id": id,
                "type": "function",
                "function": {"name": name, "arguments": ""}
            }]}),
            None,
        )
    }

    pub fn tool_args_chunk(&mut self, id: &str, args: &str) -> Value {
        let next = self.tool_index.len();
        let index = *self.tool_index.entry(id.to_string()).or_insert(next);
        self.envelope(
            json!({"tool_calls": [{
                "index": index,
                "function": {"arguments": args}
            }]}),
            None,
        )
    }

    pub fn finish_chunk(&self, reason: FinishReason) -> Value {
        self.envelope(json!({}), Some(reason.as_openai()))
    }

    /// Chunk final de uso. Los campos que el proveedor no informó se omiten en
    /// lugar de enviarse como cero, y `nexo` explicita el origen del dato.
    pub fn usage_chunk(&self, usage: &UsageReport, cost_basis: &str) -> Value {
        let mut u = serde_json::Map::new();
        if let Some(v) = usage.input_tokens {
            u.insert("prompt_tokens".into(), json!(v));
        }
        if let Some(v) = usage.output_tokens {
            u.insert("completion_tokens".into(), json!(v));
        }
        if let Some(v) = usage.total_tokens() {
            u.insert("total_tokens".into(), json!(v));
        }
        if let Some(v) = usage.cached_input_tokens {
            u.insert(
                "prompt_tokens_details".into(),
                json!({"cached_tokens": v}),
            );
        }
        if let Some(v) = usage.reasoning_tokens {
            u.insert(
                "completion_tokens_details".into(),
                json!({"reasoning_tokens": v}),
            );
        }
        u.insert(
            "nexo".into(),
            json!({"usage_source": usage.source.as_str(), "cost_basis": cost_basis}),
        );

        json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [],
            "usage": Value::Object(u),
        })
    }

    /// Respuesta completa no-streaming, ensamblada a partir de los eventos.
    pub fn full_response(
        &self,
        text: &str,
        tool_calls: &[ToolCall],
        reason: FinishReason,
        usage: &UsageReport,
        cost_basis: &str,
    ) -> Value {
        let mut message = serde_json::Map::new();
        message.insert("role".into(), json!("assistant"));
        message.insert(
            "content".into(),
            if text.is_empty() && !tool_calls.is_empty() {
                Value::Null
            } else {
                json!(text)
            },
        );
        if !tool_calls.is_empty() {
            let calls: Vec<Value> = tool_calls
                .iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "type": "function",
                        "function": {"name": c.name, "arguments": c.arguments_json}
                    })
                })
                .collect();
            message.insert("tool_calls".into(), json!(calls));
        }

        let usage_chunk = self.usage_chunk(usage, cost_basis);

        json!({
            "id": self.id,
            "object": "chat.completion",
            "created": self.created,
            "model": self.model,
            "choices": [{
                "index": 0,
                "message": Value::Object(message),
                "finish_reason": reason.as_openai(),
            }],
            "usage": usage_chunk.get("usage").cloned().unwrap_or(Value::Null),
        })
    }
}

/// Cuerpo de error con la forma que esperan los clientes de OpenAI, más un
/// bloque `nexo` que nombra la causa sin ambigüedad.
pub fn error_body(err: &AdapterError) -> Value {
    let mut nexo = serde_json::Map::new();
    nexo.insert("kind".into(), json!(err.kind_str()));

    match err {
        AdapterError::SubscriptionPathBroken { provider, detail } => {
            nexo.insert("provider".into(), json!(provider));
            nexo.insert("detail".into(), json!(detail));
            nexo.insert(
                "hint".into(),
                json!(
                    "La vía de suscripción no es un mecanismo soportado por el proveedor \
                     y ha dejado de funcionar. Configura una API key como respaldo en Nexo \
                     o vuelve a conectar la cuenta."
                ),
            );
        }
        AdapterError::LocalLimit { app_id, window_secs, detail } => {
            nexo.insert("app_id".into(), json!(app_id));
            nexo.insert("window_seconds".into(), json!(window_secs));
            nexo.insert("detail".into(), json!(detail));
            nexo.insert(
                "limited_by".into(),
                json!("nexo"),
            );
        }
        AdapterError::RateLimited { retry_after } => {
            nexo.insert("limited_by".into(), json!("provider"));
            if let Some(d) = retry_after {
                nexo.insert("retry_after_seconds".into(), json!(d.as_secs()));
            }
        }
        AdapterError::Unsupported { capability, hint } => {
            nexo.insert("capability".into(), json!(capability));
            if let Some(h) = hint {
                nexo.insert("hint".into(), json!(h));
            }
        }
        AdapterError::Auth { reauth_required, .. } => {
            nexo.insert("reauth_required".into(), json!(reauth_required));
        }
        _ => {}
    }

    json!({
        "error": {
            "message": err.to_string(),
            "type": "invalid_request_error",
            "code": err.openai_code(),
            "nexo": Value::Object(nexo),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(v: Value) -> WireChatRequest {
        serde_json::from_value(v).expect("deserializa")
    }

    #[test]
    fn minimal_request_is_accepted() {
        let w = parse(json!({
            "model": "gpt-5.5",
            "messages": [{"role": "user", "content": "hola"}]
        }));
        let r = w
            .into_internal("gpt-5.5".into(), "openai/gpt-5.5".into())
            .unwrap();
        assert_eq!(r.messages.len(), 1);
        assert_eq!(r.messages[0].text(), "hola");
        assert!(!r.stream);
    }

    #[test]
    fn empty_messages_are_rejected() {
        let w = parse(json!({"model": "m", "messages": []}));
        assert!(w.into_internal("m".into(), "p/m".into()).is_err());
    }

    #[test]
    fn unknown_role_is_rejected() {
        let w = parse(json!({
            "model": "m",
            "messages": [{"role": "wizard", "content": "x"}]
        }));
        assert!(w.into_internal("m".into(), "p/m".into()).is_err());
    }

    #[test]
    fn developer_role_maps_to_system() {
        let w = parse(json!({
            "model": "m",
            "messages": [{"role": "developer", "content": "instrucciones"}]
        }));
        let r = w.into_internal("m".into(), "p/m".into()).unwrap();
        assert_eq!(r.messages[0].role, Role::System);
        assert_eq!(r.system_text().as_deref(), Some("instrucciones"));
    }

    #[test]
    fn multimodal_content_array_is_parsed() {
        let w = parse(json!({
            "model": "m",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "mira"},
                {"type": "image_url", "image_url": {"url": "https://x/y.png"}}
            ]}]
        }));
        let r = w.into_internal("m".into(), "p/m".into()).unwrap();
        assert_eq!(r.messages[0].parts.len(), 2);
        assert!(matches!(r.messages[0].parts[1], ContentPart::ImageUrl(_)));
    }

    #[test]
    fn both_max_token_fields_are_honoured_with_new_one_winning() {
        let w = parse(json!({
            "model": "m", "messages": [{"role": "user", "content": "x"}],
            "max_tokens": 100, "max_completion_tokens": 200
        }));
        let r = w.into_internal("m".into(), "p/m".into()).unwrap();
        assert_eq!(r.max_output_tokens, Some(200));

        let w = parse(json!({
            "model": "m", "messages": [{"role": "user", "content": "x"}],
            "max_tokens": 100
        }));
        let r = w.into_internal("m".into(), "p/m".into()).unwrap();
        assert_eq!(r.max_output_tokens, Some(100));
    }

    #[test]
    fn stop_accepts_string_and_array() {
        let w = parse(json!({
            "model": "m", "messages": [{"role": "user", "content": "x"}], "stop": "FIN"
        }));
        assert_eq!(w.into_internal("m".into(), "p/m".into()).unwrap().stop, vec!["FIN"]);

        let w = parse(json!({
            "model": "m", "messages": [{"role": "user", "content": "x"}],
            "stop": ["A", "B"]
        }));
        assert_eq!(
            w.into_internal("m".into(), "p/m".into()).unwrap().stop,
            vec!["A", "B"]
        );
    }

    #[test]
    fn tool_choice_variants_are_mapped() {
        let cases = [
            (json!("none"), ToolChoice::None),
            (json!("required"), ToolChoice::Required),
            (json!("auto"), ToolChoice::Auto),
            (
                json!({"type": "function", "function": {"name": "t"}}),
                ToolChoice::Named("t".into()),
            ),
        ];
        for (raw, expected) in cases {
            let w = parse(json!({
                "model": "m", "messages": [{"role": "user", "content": "x"}],
                "tool_choice": raw
            }));
            assert_eq!(
                w.into_internal("m".into(), "p/m".into()).unwrap().tool_choice,
                expected
            );
        }
    }

    #[test]
    fn json_schema_response_format_enables_json_mode() {
        let w = parse(json!({
            "model": "m", "messages": [{"role": "user", "content": "x"}],
            "response_format": {"type": "json_schema"}
        }));
        assert!(w.into_internal("m".into(), "p/m".into()).unwrap().json_mode);
    }

    #[test]
    fn assistant_tool_calls_survive_the_round_trip() {
        let w = parse(json!({
            "model": "m",
            "messages": [
                {"role": "assistant", "content": null, "tool_calls": [{
                    "id": "call_1", "type": "function",
                    "function": {"name": "buscar", "arguments": "{\"q\":\"a\"}"}
                }]},
                {"role": "tool", "tool_call_id": "call_1", "content": "resultado"}
            ]
        }));
        let r = w.into_internal("m".into(), "p/m".into()).unwrap();
        assert_eq!(r.messages[0].tool_calls[0].name, "buscar");
        assert_eq!(r.messages[1].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn chunks_have_the_expected_shape() {
        let b = ChunkBuilder::new("openai/gpt-5.5");
        let c = b.text_chunk("hola");
        assert_eq!(c["object"], "chat.completion.chunk");
        assert_eq!(c["choices"][0]["delta"]["content"], "hola");
        assert!(c["choices"][0]["finish_reason"].is_null());
        assert_eq!(c["model"], "openai/gpt-5.5");
    }

    #[test]
    fn tool_chunks_keep_a_stable_index_per_call_id() {
        let mut b = ChunkBuilder::new("m");
        assert_eq!(b.tool_start_chunk("a", "t1")["choices"][0]["delta"]["tool_calls"][0]["index"], 0);
        assert_eq!(b.tool_start_chunk("b", "t2")["choices"][0]["delta"]["tool_calls"][0]["index"], 1);
        assert_eq!(b.tool_args_chunk("a", "{}")["choices"][0]["delta"]["tool_calls"][0]["index"], 0);
    }

    #[test]
    fn usage_chunk_omits_unknown_fields_instead_of_sending_zero() {
        let b = ChunkBuilder::new("m");
        let chunk = b.usage_chunk(&UsageReport::unavailable(), "subscription");
        let usage = &chunk["usage"];
        assert!(usage.get("prompt_tokens").is_none());
        assert!(usage.get("total_tokens").is_none());
        assert_eq!(usage["nexo"]["usage_source"], "unavailable");
        assert_eq!(usage["nexo"]["cost_basis"], "subscription");
    }

    #[test]
    fn full_response_nulls_content_when_only_tool_calls() {
        let b = ChunkBuilder::new("m");
        let calls = [ToolCall {
            id: "call_1".into(),
            name: "t".into(),
            arguments_json: "{}".into(),
        }];
        let r = b.full_response(
            "",
            &calls,
            FinishReason::ToolCalls,
            &UsageReport::unavailable(),
            "subscription",
        );
        assert!(r["choices"][0]["message"]["content"].is_null());
        assert_eq!(r["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(r["object"], "chat.completion");
    }

    #[test]
    fn broken_subscription_error_explains_the_fallback() {
        let body = error_body(&AdapterError::SubscriptionPathBroken {
            provider: "openai".into(),
            detail: "HTTP 404".into(),
        });
        assert_eq!(body["error"]["code"], "subscription_path_broken");
        assert!(body["error"]["nexo"]["hint"]
            .as_str()
            .unwrap()
            .contains("API key"));
    }

    #[test]
    fn local_and_provider_limits_are_distinguishable_on_the_wire() {
        let local = error_body(&AdapterError::LocalLimit {
            app_id: "a1".into(),
            window_secs: 3600,
            detail: "60/60".into(),
        });
        assert_eq!(local["error"]["nexo"]["limited_by"], "nexo");
        assert_eq!(local["error"]["code"], "nexo_app_limit_exceeded");

        let remote = error_body(&AdapterError::RateLimited {
            retry_after: Some(std::time::Duration::from_secs(30)),
        });
        assert_eq!(remote["error"]["nexo"]["limited_by"], "provider");
        assert_eq!(remote["error"]["nexo"]["retry_after_seconds"], 30);
    }
}
