//! Adaptador de Gemini Code Assist para cuentas autenticadas con la suscripción
//! de Google. El protocolo no es la API pública de Gemini: es el envoltorio
//! `v1internal` que utiliza el cliente oficial Antigravity.

use crate::auth::gemini_subscription as auth;
use crate::provider::{
    Accounting, AdapterError, AdapterId, Capabilities, ChatEvent, ChatRequest, ContentPart,
    CredentialKind, EventStream, FinishReason, Health, Limits, Message, ModelDescriptor,
    ProviderAdapter, ReasoningEffort, ResolvedCredential, Role, ToolChoice, UsageReport,
    UsageSource,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::time::Duration;

pub const PROVIDER: &str = auth::PROVIDER;
pub const CREDENTIAL_KIND: CredentialKind = CredentialKind::SubscriptionOauth;

const RUNTIME_BASE_URLS: &[&str] = auth::CODE_ASSIST_RUNTIME_BASE_URLS;

pub struct GeminiSubscriptionAdapter {
    http: reqwest::Client,
    runtime_bases: Vec<String>,
}

impl GeminiSubscriptionAdapter {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            runtime_bases: RUNTIME_BASE_URLS
                .iter()
                .map(|base| (*base).into())
                .collect(),
        }
    }

    #[cfg(test)]
    fn with_runtime_bases(http: reqwest::Client, bases: Vec<String>) -> Self {
        Self {
            http,
            runtime_bases: bases,
        }
    }

    fn project_id(cred: &ResolvedCredential) -> Result<String, AdapterError> {
        let metadata = cred
            .provider_metadata
            .as_deref()
            .ok_or_else(missing_project)?;
        let value: Value = serde_json::from_str(metadata).map_err(|e| AdapterError::Malformed {
            detail: format!("metadatos de Gemini inválidos: {e}"),
        })?;
        value
            .get("project_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .ok_or_else(missing_project)
    }

    fn endpoint(base: &str, action: &str) -> String {
        format!("{base}/{}:{action}", auth::CODE_ASSIST_API_VERSION)
    }
}

fn missing_project() -> AdapterError {
    AdapterError::SubscriptionPathBroken {
        provider: PROVIDER.into(),
        detail: "Google todavía no ha activado el contexto de Antigravity para esta cuenta; Nexo volverá a intentarlo automáticamente"
            .into(),
    }
}

#[async_trait]
impl ProviderAdapter for GeminiSubscriptionAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::new(PROVIDER, CREDENTIAL_KIND)
    }

    async fn catalog(
        &self,
        cred: &ResolvedCredential,
    ) -> Result<Vec<ModelDescriptor>, AdapterError> {
        let project = Self::project_id(cred)?;
        let mut last_error = None;
        for base in &self.runtime_bases {
            let response = self
                .http
                .post(Self::endpoint(base, "fetchAvailableModels"))
                .bearer_auth(&cred.secret)
                .header("content-type", "application/json")
                .header("user-agent", auth::ANTIGRAVITY_USER_AGENT)
                .header("x-goog-api-client", auth::ANTIGRAVITY_API_CLIENT)
                .json(&json!({ "project": project }))
                .send()
                .await
                .map_err(AdapterError::from_reqwest)?;
            if response.status().is_success() {
                let body = response
                    .json::<Value>()
                    .await
                    .map_err(AdapterError::from_reqwest)?;
                let models = parse_models(&body);
                if !models.is_empty() {
                    return Ok(models);
                }
                last_error = Some(AdapterError::Malformed {
                    detail: "fetchAvailableModels devolvió un catálogo vacío".into(),
                });
                continue;
            }
            let status = response.status();
            let retry_after = retry_after(&response);
            let text = response.text().await.unwrap_or_default();
            let error = classify_http_error(status.as_u16(), retry_after, &text);
            if matches!(status.as_u16(), 404 | 502 | 503 | 504) {
                last_error = Some(error);
                continue;
            }
            return Err(error);
        }
        Err(last_error.unwrap_or_else(|| AdapterError::Transport {
            detail: "ningún endpoint de catálogo de Gemini responde".into(),
        }))
    }

    async fn stream(
        &self,
        req: &ChatRequest,
        cred: &ResolvedCredential,
    ) -> Result<EventStream, AdapterError> {
        let project = Self::project_id(cred)?;
        let body = build_request(req, &project)?;
        let action = if req.stream {
            "streamGenerateContent?alt=sse"
        } else {
            "generateContent"
        };
        let mut last_error = None;

        for base in &self.runtime_bases {
            let mut request = self
                .http
                .post(Self::endpoint(base, action))
                .bearer_auth(&cred.secret)
                .header("content-type", "application/json")
                .header("user-agent", auth::ANTIGRAVITY_USER_AGENT)
                .header("x-goog-api-client", auth::ANTIGRAVITY_API_CLIENT)
                .json(&body);
            if req.stream {
                request = request.header("accept", "text/event-stream");
            }

            let response = request.send().await.map_err(AdapterError::from_reqwest)?;
            let status = response.status();
            if status.is_success() {
                if req.stream {
                    return Ok(stream_response(response));
                }
                let value = response
                    .json::<Value>()
                    .await
                    .map_err(AdapterError::from_reqwest)?;
                let mut state = TranslatorState::default();
                let events = translate_value(&value, &mut state)?;
                return Ok(Box::pin(futures::stream::iter(events.into_iter().map(Ok))));
            }
            let retry = retry_after(&response);
            let text = response.text().await.unwrap_or_default();
            let error = classify_http_error(status.as_u16(), retry, &text);
            if matches!(status.as_u16(), 404 | 502 | 503 | 504) {
                last_error = Some(error);
                continue;
            }
            return Err(error);
        }
        Err(last_error.unwrap_or_else(|| AdapterError::Transport {
            detail: "ningún endpoint de Gemini responde".into(),
        }))
    }

    async fn health(&self, cred: &ResolvedCredential) -> Health {
        // No existe una comprobación barata que no pueda consumir cuota.
        if cred.secret.is_empty() {
            Health::Down
        } else {
            Health::Unknown
        }
    }
}

fn build_request(req: &ChatRequest, project: &str) -> Result<Value, AdapterError> {
    let contents = build_contents(&req.messages)?;
    let mut request = Map::new();
    request.insert("contents".into(), Value::Array(contents));
    if let Some(system) = req.system_text() {
        request.insert(
            "systemInstruction".into(),
            json!({
                "role": "user",
                "parts": [{"text": system}]
            }),
        );
    }
    if !req.tools.is_empty() {
        request.insert(
            "tools".into(),
            json!([{"functionDeclarations": req.tools.iter().map(|tool| json!({
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters
            })).collect::<Vec<_>>() }]),
        );
        request.insert(
            "toolConfig".into(),
            json!({
                "functionCallingConfig": tool_choice(&req.tool_choice)
            }),
        );
    }

    let mut generation = Map::new();
    if let Some(value) = req.max_output_tokens {
        generation.insert("maxOutputTokens".into(), json!(value));
    }
    if let Some(value) = req.temperature {
        generation.insert("temperature".into(), json!(value));
    }
    if let Some(value) = req.top_p {
        generation.insert("topP".into(), json!(value));
    }
    if !req.stop.is_empty() {
        generation.insert("stopSequences".into(), json!(req.stop));
    }
    if req.json_mode {
        generation.insert("responseMimeType".into(), json!("application/json"));
    }
    if let Some(effort) = req.reasoning {
        generation.insert(
            "thinkingConfig".into(),
            json!({
                "thinkingBudget": thinking_budget(effort),
                "includeThoughts": true
            }),
        );
    }
    if !generation.is_empty() {
        request.insert("generationConfig".into(), Value::Object(generation));
    }

    Ok(json!({
        "model": req.api_model,
        "project": project,
        "user_prompt_id": format!("nexo-{}", uuid::Uuid::new_v4().simple()),
        "request": Value::Object(request)
    }))
}

fn build_contents(messages: &[Message]) -> Result<Vec<Value>, AdapterError> {
    let mut call_names = HashMap::new();
    for message in messages {
        for call in &message.tool_calls {
            call_names.insert(call.id.clone(), call.name.clone());
        }
    }
    let mut contents = Vec::new();
    for message in messages {
        if message.role == Role::System {
            continue;
        }
        let role = if message.role == Role::Assistant {
            "model"
        } else {
            "user"
        };
        let mut parts = Vec::new();
        for part in &message.parts {
            match part {
                ContentPart::Text(text) => {
                    if !text.is_empty() {
                        parts.push(json!({"text": text}));
                    }
                }
                ContentPart::ImageUrl(_) | ContentPart::Audio { .. } | ContentPart::File { .. } => {
                    return Err(AdapterError::Unsupported {
                        capability: "vision/audio".into(),
                        hint: Some(
                            "Gemini por suscripción en Nexo solo admite texto en esta versión"
                                .into(),
                        ),
                    });
                }
            }
        }
        for call in &message.tool_calls {
            let args: Value = serde_json::from_str(&call.arguments_json).map_err(|e| {
                AdapterError::Malformed {
                    detail: format!(
                        "argumentos JSON de la herramienta {} inválidos: {e}",
                        call.name
                    ),
                }
            })?;
            parts.push(json!({"functionCall": {"name": call.name, "args": args}}));
        }
        if message.role == Role::Tool {
            let name = message
                .tool_call_id
                .as_ref()
                .and_then(|id| call_names.get(id))
                .cloned()
                .unwrap_or_else(|| {
                    message
                        .tool_call_id
                        .clone()
                        .unwrap_or_else(|| "tool".into())
                });
            parts.push(json!({
                "functionResponse": {"name": name, "response": {"output": message.text()}}
            }));
        }
        if parts.is_empty() {
            continue;
        }
        if let Some(Value::Object(previous)) = contents.last_mut() {
            if previous.get("role").and_then(Value::as_str) == Some(role) {
                previous
                    .get_mut("parts")
                    .and_then(Value::as_array_mut)
                    .unwrap()
                    .extend(parts);
                continue;
            }
        }
        contents.push(json!({"role": role, "parts": parts}));
    }
    if contents.is_empty() {
        return Err(AdapterError::Malformed {
            detail: "la petición no contiene mensajes de usuario".into(),
        });
    }
    Ok(contents)
}

fn tool_choice(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::None => json!({"mode": "NONE"}),
        ToolChoice::Required => json!({"mode": "ANY"}),
        ToolChoice::Named(name) => json!({"mode": "ANY", "allowedFunctionNames": [name]}),
        ToolChoice::Auto => json!({"mode": "AUTO"}),
    }
}

fn thinking_budget(effort: ReasoningEffort) -> u32 {
    match effort {
        ReasoningEffort::Minimal => 1024,
        ReasoningEffort::Low => 4096,
        ReasoningEffort::Medium => 8192,
        ReasoningEffort::High => 16384,
        ReasoningEffort::XHigh => 32768,
    }
}

fn parse_models(body: &Value) -> Vec<ModelDescriptor> {
    let mut out = Vec::new();
    if let Some(models) = body.get("models").and_then(Value::as_object) {
        for (id, value) in models {
            if let Some(model) = descriptor(id, value) {
                out.push(model);
            }
        }
    } else if let Some(models) = body.get("models").and_then(Value::as_array) {
        for value in models {
            let id = value
                .get("name")
                .or_else(|| value.get("id"))
                .and_then(Value::as_str);
            if let Some(id) = id.and_then(normalize_model_id) {
                if let Some(model) = descriptor(&id, value) {
                    out.push(model);
                }
            }
        }
    }
    out
}

fn descriptor(raw_id: &str, value: &Value) -> Option<ModelDescriptor> {
    let api_id = normalize_model_id(raw_id)?;
    let modalities = value
        .get("inputModalities")
        .or_else(|| value.get("input_modalities"))
        .and_then(Value::as_array);
    let vision = modalities
        .map(|items| items.iter().any(|item| item.as_str() == Some("IMAGE")))
        .unwrap_or(false);
    let reasoning = api_id.contains("gemini-2.5") || api_id.contains("gemini-3");
    let input_max = value
        .get("inputTokenLimit")
        .or_else(|| value.get("input_token_limit"))
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    let output_max = value
        .get("outputTokenLimit")
        .or_else(|| value.get("output_token_limit"))
        .and_then(Value::as_u64)
        .map(|v| v as u32);
    Some(ModelDescriptor {
        public_name: format!("{PROVIDER}/{api_id}"),
        api_id,
        caps: Capabilities {
            text: true,
            vision,
            audio: false,
            tools: true,
            reasoning,
            json_mode: true,
            streaming: true,
            reasoning_levels: if reasoning {
                vec!["minimal", "low", "medium", "high", "xhigh"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            } else {
                vec![]
            },
            ..Default::default()
        },
        limits: Limits {
            context_max: input_max,
            input_max,
            output_max,
        },
        accounting: Accounting::Subscription,
        pricing: None,
    })
}

fn normalize_model_id(raw: &str) -> Option<String> {
    let id = raw
        .trim()
        .strip_prefix("models/")
        .unwrap_or(raw.trim())
        .trim();
    (!id.is_empty()).then(|| id.to_string())
}

#[derive(Default)]
struct TranslatorState {
    started: bool,
    tool_index: usize,
}

fn stream_response(response: reqwest::Response) -> EventStream {
    let stream = response
        .bytes_stream()
        .eventsource()
        .map(|item| {
            item.map_err(|e| AdapterError::Transport {
                detail: e.to_string(),
            })
        })
        .scan(TranslatorState::default(), |state, item| {
            let events: Vec<Result<ChatEvent, AdapterError>> = match item {
                Err(error) => vec![Err(error)],
                Ok(event) if event.data.trim() == "[DONE]" => vec![],
                Ok(event) => match serde_json::from_str::<Value>(&event.data) {
                    Err(error) => vec![Err(AdapterError::Malformed {
                        detail: format!("evento SSE inválido: {error}"),
                    })],
                    Ok(value) => match translate_value(&value, state) {
                        Ok(events) => events.into_iter().map(Ok).collect(),
                        Err(error) => vec![Err(error)],
                    },
                },
            };
            futures::future::ready(Some(events))
        })
        .flat_map(futures::stream::iter);
    Box::pin(stream)
}

fn translate_value(
    value: &Value,
    state: &mut TranslatorState,
) -> Result<Vec<ChatEvent>, AdapterError> {
    let response = value.get("response").unwrap_or(value);
    let mut events = Vec::new();
    if !state.started {
        events.push(ChatEvent::Started {
            provider_request_id: value
                .get("traceId")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
        state.started = true;
    }
    if let Some(usage) = response.get("usageMetadata") {
        events.push(ChatEvent::Usage(UsageReport {
            input_tokens: usage
                .get("promptTokenCount")
                .and_then(Value::as_u64)
                .map(|v| v as u32),
            output_tokens: usage
                .get("candidatesTokenCount")
                .and_then(Value::as_u64)
                .map(|v| v as u32),
            cached_input_tokens: usage
                .get("cachedContentTokenCount")
                .and_then(Value::as_u64)
                .map(|v| v as u32),
            reasoning_tokens: None,
            source: UsageSource::Reported,
            raw: Some(usage.clone()),
        }));
    }
    let Some(candidate) = response.pointer("/candidates/0") else {
        return Ok(events);
    };
    if let Some(parts) = candidate
        .pointer("/content/parts")
        .and_then(Value::as_array)
    {
        for part in parts {
            if let Some(text) = part
                .get("text")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                if part
                    .get("thought")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    events.push(ChatEvent::ReasoningDelta { text: text.into() });
                } else {
                    events.push(ChatEvent::TextDelta { text: text.into() });
                }
            }
            if let Some(call) = part.get("functionCall") {
                let name = call.get("name").and_then(Value::as_str).ok_or_else(|| {
                    AdapterError::Malformed {
                        detail: "functionCall sin nombre".into(),
                    }
                })?;
                let id = format!("gemini_call_{}", state.tool_index);
                state.tool_index += 1;
                let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
                let args_json =
                    serde_json::to_string(&args).map_err(|e| AdapterError::Malformed {
                        detail: e.to_string(),
                    })?;
                events.push(ChatEvent::ToolCallStart {
                    id: id.clone(),
                    name: name.into(),
                });
                events.push(ChatEvent::ToolCallArgumentsDone {
                    id: id.clone(),
                    args_json,
                });
                events.push(ChatEvent::ToolCallEnd { id });
            }
        }
    }
    if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
        events.push(ChatEvent::Finished {
            reason: finish_reason(reason),
        });
    }
    Ok(events)
}

fn finish_reason(raw: &str) -> FinishReason {
    match raw {
        "MAX_TOKENS" => FinishReason::Length,
        "SAFETY" | "RECITATION" | "BLOCKLIST" => FinishReason::ContentFilter,
        "STOP" => FinishReason::Stop,
        _ => FinishReason::Stop,
    }
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn classify_http_error(status: u16, retry_after: Option<Duration>, body: &str) -> AdapterError {
    let structured = serde_json::from_str::<Value>(body).ok();
    let provider_code = structured
        .as_ref()
        .and_then(|value| {
            value
                .get("status")
                .or_else(|| value.pointer("/error/status"))
        })
        .and_then(Value::as_str)
        .map(str::to_string);
    let message = structured
        .as_ref()
        .and_then(|value| {
            value
                .get("message")
                .or_else(|| value.pointer("/error/message"))
        })
        .and_then(Value::as_str)
        .unwrap_or(body);
    match status {
        401 => AdapterError::Auth {
            reason: "el token de Gemini ha caducado".into(),
            reauth_required: true,
        },
        403 => AdapterError::Auth {
            reason: format!(
                "Google no permite usar Code Assist con esta cuenta: {}",
                truncate(body, 300)
            ),
            reauth_required: true,
        },
        429 => AdapterError::RateLimited { retry_after },
        _ => AdapterError::Upstream {
            status,
            provider_code,
            message: truncate(message, 500),
        },
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}…", &value[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Message, ToolCall, ToolDef};
    use axum::Router;
    use axum::extract::Json as AxumJson;
    use axum::routing::post;

    fn request() -> ChatRequest {
        ChatRequest {
            api_model: "gemini-2.5-flash".into(),
            public_model: "gemini_subscription/gemini-2.5-flash".into(),
            opencode_session: None,
            messages: vec![Message {
                role: Role::User,
                parts: vec![ContentPart::Text("hola".into())],
                tool_call_id: None,
                tool_calls: vec![],
            }],
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            reasoning: None,
            max_output_tokens: Some(100),
            temperature: None,
            top_p: None,
            stop: vec![],
            json_mode: false,
            stream: true,
        }
    }

    #[test]
    fn request_uses_code_assist_envelope_and_camel_case_fields() {
        let body = build_request(&request(), "project-1").unwrap();
        assert_eq!(body["project"], "project-1");
        assert_eq!(body["request"]["contents"][0]["role"], "user");
        assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 100);
        assert!(
            body["user_prompt_id"]
                .as_str()
                .unwrap()
                .starts_with("nexo-")
        );
    }

    #[test]
    fn tools_and_previous_calls_round_trip_to_gemini_parts() {
        let mut req = request();
        req.tools = vec![ToolDef {
            name: "buscar".into(),
            description: Some("Busca".into()),
            parameters: json!({"type":"object"}),
        }];
        req.messages.push(Message {
            role: Role::Assistant,
            parts: vec![],
            tool_call_id: None,
            tool_calls: vec![ToolCall {
                id: "call-1".into(),
                name: "buscar".into(),
                arguments_json: r#"{"q":"nexo"}"#.into(),
            }],
        });
        req.messages.push(Message {
            role: Role::Tool,
            parts: vec![ContentPart::Text("resultado".into())],
            tool_call_id: Some("call-1".into()),
            tool_calls: vec![],
        });
        let body = build_request(&req, "project-1").unwrap();
        assert_eq!(
            body["request"]["tools"][0]["functionDeclarations"][0]["name"],
            "buscar"
        );
        assert_eq!(
            body["request"]["contents"][1]["parts"][0]["functionCall"]["name"],
            "buscar"
        );
        assert_eq!(
            body["request"]["contents"][2]["parts"][1]["functionResponse"]["name"],
            "buscar"
        );
    }

    #[test]
    fn response_translates_text_usage_and_finish() {
        let value = json!({"traceId":"trace-1","response":{"candidates":[{"content":{"parts":[{"text":"hola"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}}});
        let mut state = TranslatorState::default();
        let events = translate_value(&value, &mut state).unwrap();
        assert!(matches!(events[0], ChatEvent::Started { .. }));
        assert!(matches!(events[1], ChatEvent::Usage(_)));
        assert!(matches!(&events[2], ChatEvent::TextDelta { text } if text == "hola"));
        assert!(matches!(
            events[3],
            ChatEvent::Finished {
                reason: FinishReason::Stop
            }
        ));
    }

    #[test]
    fn google_rpc_status_is_preserved_in_upstream_errors() {
        let error = classify_http_error(
            400,
            None,
            r#"{"error":{"status":"INVALID_ARGUMENT","message":"modelo no válido"}}"#,
        );
        assert!(matches!(
            error,
            AdapterError::Upstream {
                provider_code: Some(code),
                message,
                ..
            } if code == "INVALID_ARGUMENT" && message == "modelo no válido"
        ));
    }

    #[tokio::test]
    async fn adapter_uses_native_catalog_and_non_stream_contract() {
        let app = Router::new()
            .route(
                "/v1internal:fetchAvailableModels",
                post(|| async {
                    AxumJson(json!({
                        "models": {"gemini-2.5-flash": {"inputTokenLimit": 1000}}
                    }))
                }),
            )
            .route(
                "/v1internal:generateContent",
                post(|AxumJson(body): AxumJson<Value>| async move {
                    assert_eq!(body["project"], "project-1");
                    assert_eq!(body["model"], "gemini-2.5-flash");
                    AxumJson(json!({
                        "traceId": "trace-test",
                        "response": {
                            "candidates": [{
                                "content": {"parts": [{"text": "respuesta"}]},
                                "finishReason": "STOP"
                            }],
                            "usageMetadata": {
                                "promptTokenCount": 4,
                                "candidatesTokenCount": 2
                            }
                        }
                    }))
                }),
            );
        let app = app.route(
            "/v1internal:streamGenerateContent",
            post(|| async {
                let first = json!({"traceId":"trace-stream","response":{"candidates":[{"content":{"parts":[{"text":"st"}]}}]}});
                let last = json!({"response":{"candidates":[{"content":{"parts":[{"text":"ream"}]},"finishReason":"STOP"}]}});
                (
                    [("content-type", "text/event-stream")],
                    format!("data: {first}\n\ndata: {last}\n\n"),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let metadata = serde_json::to_string(&auth::AccountMetadata {
            project_id: "project-1".into(),
            ..Default::default()
        })
        .unwrap();
        let cred = ResolvedCredential {
            account_id: "account-1".into(),
            provider_id: PROVIDER.into(),
            kind: CREDENTIAL_KIND,
            secret: "access-token".into(),
            external_id: None,
            provider_metadata: Some(metadata),
        };
        let adapter = GeminiSubscriptionAdapter::with_runtime_bases(
            reqwest::Client::new(),
            vec![format!("http://{address}")],
        );
        let catalog = adapter.catalog(&cred).await.unwrap();
        assert_eq!(
            catalog[0].public_name,
            "gemini_subscription/gemini-2.5-flash"
        );
        assert_eq!(catalog[0].limits.input_max, Some(1000));

        let mut req = request();
        req.stream = false;
        let events = adapter
            .stream(&req, &cred)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        assert!(events.iter().any(
            |event| matches!(event, Ok(ChatEvent::TextDelta { text }) if text == "respuesta")
        ));
        assert!(events.iter().any(
            |event| matches!(event, Ok(ChatEvent::Usage(usage)) if usage.input_tokens == Some(4))
        ));

        req.stream = true;
        let events = adapter
            .stream(&req, &cred)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        let text = events
            .iter()
            .filter_map(|event| match event {
                Ok(ChatEvent::TextDelta { text }) => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, "stream");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Ok(ChatEvent::Started { .. })))
                .count(),
            1
        );
        server.abort();
    }
}
