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

/// Rastrea qué `id` de llamada a herramienta corresponde a cada índice del stream.
///
/// El protocolo estándar de OpenAI —y lo que de verdad envían los proveedores,
/// verificado contra OpenCode Zen el 2026-07-31— manda el `id` solo en el primer
/// chunk de cada llamada; los fragmentos de argumentos que siguen solo llevan el
/// `index`. Sin este rastreo, esos fragmentos se etiquetaban con un id inventado
/// que nunca coincidía con el de la llamada, y los argumentos se perdían.
pub type ToolCallIds = std::collections::HashMap<u64, String>;

/// Traduce un chunk de `chat/completions` al vocabulario interno.
pub fn translate_chunk(chunk: &Value, tool_ids: &mut ToolCallIds) -> Vec<ChatEvent> {
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

    // Dos nombres de campo distintos observados entre proveedores para lo mismo:
    // DeepSeek (vía Zen) usa `reasoning_content`; otro modelo de la misma pasarela
    // usa `reasoning` a secas. Se comprueban los dos.
    let reasoning_text = choice
        .pointer("/delta/reasoning_content")
        .or_else(|| choice.pointer("/delta/reasoning"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    if let Some(text) = reasoning_text {
        events.push(ChatEvent::ReasoningDelta { text: text.to_string() });
    }

    if let Some(calls) = choice.pointer("/delta/tool_calls").and_then(|v| v.as_array()) {
        for call in calls {
            let index = call.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
            let id = match call.get("id").and_then(|v| v.as_str()) {
                // El id solo llega en el primer chunk de la llamada: se recuerda
                // para los fragmentos de argumentos que vienen después sin él.
                Some(id) => {
                    tool_ids.insert(index, id.to_string());
                    id.to_string()
                }
                None => tool_ids
                    .get(&index)
                    .cloned()
                    .unwrap_or_else(|| format!("call_{index}")),
            };
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
    let mut tool_ids = ToolCallIds::new();
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
                            evs.extend(translate_chunk(&chunk, &mut tool_ids).into_iter().map(Ok));
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
///
/// Se intenta primero leer el cuerpo como `{"error": {"type": "...", "message":
/// "..."}}`, y solo se cae al HTTP status si no hay ese sobre. Hace falta: OpenCode
/// Zen devuelve `401` para «saldo insuficiente», «modelo no soportado» y «clave
/// inválida» por igual (verificado el 2026-07-31), así que confiar en el status
/// habría mostrado «clave inválida» cuando el problema real era el saldo — que es
/// exactamente lo que le pasó al usuario probando en Msty.
pub fn classify_http_error(
    status: u16,
    retry_after: Option<std::time::Duration>,
    body: &str,
) -> AdapterError {
    if let Some(err) = parse_error_envelope(body, status) {
        return err;
    }
    classify_by_status(status, retry_after, body)
}

fn classify_by_status(
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

/// Lee `{"error": {"type": ..., "message": ...}}` y clasifica por ese tipo.
///
/// El sobre lo comparten OpenAI y OpenCode Zen (Zen lo usa incluso cuando el tipo
/// no tiene nada que ver con autenticación, como `CreditsError` o `ModelError`).
/// Si el cuerpo no tiene esa forma, se devuelve `None` para que el llamador caiga
/// al status HTTP. `status` solo se usa para el caso desconocido, de forma que el
/// error final lleve el código real y no uno inventado.
fn parse_error_envelope(body: &str, status: u16) -> Option<AdapterError> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    // Gemini envuelve algunos errores en un array de un solo elemento
    // (verificado contra la API real el 2026-08-03, con un modelo
    // inexistente), a diferencia del sobre plano que comparten OpenAI y Zen.
    let value = match parsed.as_array().and_then(|a| a.first()) {
        Some(first) => first,
        None => &parsed,
    };
    let error = value.get("error")?;
    let message = error.get("message").and_then(|v| v.as_str());

    if let Some(kind) = error.get("type").and_then(|v| v.as_str()) {
        let message = message.unwrap_or(kind).to_string();
        return Some(match kind {
            // Zen: sin saldo en el workspace. No es un problema de la clave, y
            // reautenticar no lo arregla: hay que añadir saldo, no reconectar.
            "CreditsError" => AdapterError::Auth {
                reason: format!("saldo insuficiente en el proveedor: {message}"),
                reauth_required: false,
            },
            // Zen: el modelo pedido no existe o no está disponible para esta clave.
            "ModelError" => AdapterError::Unsupported {
                capability: "model".into(),
                hint: Some(message),
            },
            "AuthError" => AdapterError::Auth { reason: message, reauth_required: true },
            "RateLimitError" | "rate_limit_exceeded" => {
                AdapterError::RateLimited { retry_after: None }
            }
            _ => AdapterError::Upstream {
                status,
                provider_code: Some(kind.to_string()),
                message,
            },
        });
    }

    // Sobre `google.rpc.Status` de Gemini: `{"error":{"code","message","status"}}`,
    // sin campo `type`. Verificado contra la API real el 2026-08-03: una
    // clave inválida da HTTP 400 (no 401/403) con `status: "INVALID_ARGUMENT"`
    // — sin este caso, caía al genérico y el cliente veía un 502 en vez de un
    // error de credencial. `INVALID_ARGUMENT` y `NOT_FOUND` son demasiado
    // genéricos para clasificarlos solo por el código: se exige además que el
    // mensaje real hable de la clave o del modelo, o se cae al status HTTP.
    let google_status = error.get("status").and_then(|v| v.as_str())?;
    let message = message.unwrap_or(google_status).to_string();
    let lower = message.to_ascii_lowercase();
    match google_status {
        "UNAUTHENTICATED" | "PERMISSION_DENIED" => {
            Some(AdapterError::Auth { reason: message, reauth_required: true })
        }
        "INVALID_ARGUMENT" if lower.contains("api key") => {
            Some(AdapterError::Auth { reason: message, reauth_required: true })
        }
        "NOT_FOUND" if lower.contains("model") => {
            Some(AdapterError::Unsupported { capability: "model".into(), hint: Some(message) })
        }
        _ => None,
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
        match &translate_chunk(&chunk, &mut ToolCallIds::new())[0] {
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
        match &translate_chunk(&chunk, &mut ToolCallIds::new())[0] {
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
        match &translate_chunk(&chunk, &mut ToolCallIds::new())[0] {
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
            translate_chunk(&chunk, &mut ToolCallIds::new())[0],
            ChatEvent::Finished { reason: FinishReason::Length }
        ));
    }

    #[test]
    fn tool_call_delta_without_any_prior_id_falls_back_to_a_synthetic_one() {
        // Caso límite: llega un fragmento de argumentos sin que el id se haya visto
        // nunca. No puede pasar en un stream real bien formado, pero no debe entrar
        // en pánico.
        let chunk = json!({"choices": [{"delta": {"tool_calls": [
            {"index": 0, "function": {"arguments": "{\"a\""}}
        ]}}]});
        match &translate_chunk(&chunk, &mut ToolCallIds::new())[0] {
            ChatEvent::ToolCallDelta { id, .. } => assert_eq!(id, "call_0"),
            other => panic!("esperaba ToolCallDelta, llegó {other:?}"),
        }
    }

    /// Reproduce el fallo real: id solo en el primer chunk, ausente en los
    /// siguientes. Es el comportamiento estándar de OpenAI y lo que de verdad
    /// envían los proveedores — capturado contra OpenCode Zen el 2026-07-31 con
    /// un tool call real. Antes del arreglo, cada fragmento sin id recibía un
    /// `call_{index}` inventado que nunca coincidía con el id real, así que los
    /// argumentos nunca se juntaban con la llamada.
    #[test]
    fn tool_call_arguments_reassemble_across_chunks_that_omit_the_id() {
        let mut ids = ToolCallIds::new();

        let start = json!({"choices": [{"delta": {"tool_calls": [
            {"index": 0, "id": "tiempo_dy90japr030a", "type": "function",
             "function": {"name": "tiempo", "arguments": ""}}
        ]}}]});
        let e0 = translate_chunk(&start, &mut ids);
        assert!(matches!(&e0[0], ChatEvent::ToolCallStart { id, name }
            if id == "tiempo_dy90japr030a" && name == "tiempo"));

        // Los tres fragmentos siguientes, tal como los envió Zen: sin id.
        let mut assembled = String::new();
        for frag in ["{\"", "ciudad", "\":", " \"Madrid\"}"] {
            let chunk = json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": frag}}
            ]}}]});
            match &translate_chunk(&chunk, &mut ids)[0] {
                ChatEvent::ToolCallDelta { id, args_json } => {
                    assert_eq!(
                        id, "tiempo_dy90japr030a",
                        "el fragmento sin id debe heredar el de la llamada, no un id inventado"
                    );
                    assembled.push_str(args_json);
                }
                other => panic!("esperaba ToolCallDelta, llegó {other:?}"),
            }
        }
        assert_eq!(assembled, "{\"ciudad\": \"Madrid\"}");
    }

    #[test]
    fn two_concurrent_tool_calls_do_not_mix_their_arguments() {
        let mut ids = ToolCallIds::new();
        for (index, id, name) in [(0u64, "call_a", "buscar"), (1u64, "call_b", "sumar")] {
            let chunk = json!({"choices": [{"delta": {"tool_calls": [
                {"index": index, "id": id, "type": "function",
                 "function": {"name": name, "arguments": ""}}
            ]}}]});
            translate_chunk(&chunk, &mut ids);
        }
        let chunk = json!({"choices": [{"delta": {"tool_calls": [
            {"index": 1, "function": {"arguments": "b-args"}},
            {"index": 0, "function": {"arguments": "a-args"}}
        ]}}]});
        let events = translate_chunk(&chunk, &mut ids);
        let by_id: std::collections::HashMap<_, _> = events
            .iter()
            .filter_map(|e| match e {
                ChatEvent::ToolCallDelta { id, args_json } => Some((id.as_str(), args_json.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(by_id.get("call_b"), Some(&"b-args"));
        assert_eq!(by_id.get("call_a"), Some(&"a-args"));
    }

    #[test]
    fn reasoning_field_name_varies_by_backend_and_both_are_understood() {
        // `reasoning_content`, verificado con DeepSeek vía Zen.
        let a = json!({"choices": [{"delta": {"reasoning_content": "pienso"}}]});
        assert!(matches!(
            &translate_chunk(&a, &mut ToolCallIds::new())[0],
            ChatEvent::ReasoningDelta { text } if text == "pienso"
        ));
        // `reasoning` a secas, verificado con north-mini-code-free vía Zen.
        let b = json!({"choices": [{"delta": {"reasoning": "pienso"}}]});
        assert!(matches!(
            &translate_chunk(&b, &mut ToolCallIds::new())[0],
            ChatEvent::ReasoningDelta { text } if text == "pienso"
        ));
    }

    #[test]
    fn a_trailing_cost_chunk_after_done_is_harmless() {
        // Zen manda un chunk de telemetría después de [DONE], con `choices: []` y
        // sin `usage`. No debe producir ningún evento ni entrar en pánico.
        let chunk = json!({"choices": [], "x-opencode-type": "inference-cost", "cost": "0"});
        assert!(translate_chunk(&chunk, &mut ToolCallIds::new()).is_empty());
    }

    #[test]
    fn empty_chunk_yields_nothing() {
        assert!(translate_chunk(&json!({"choices": [{"delta": {}}]}), &mut ToolCallIds::new()).is_empty());
    }

    #[test]
    fn http_errors_without_a_recognisable_envelope_fall_back_to_status() {
        assert_eq!(classify_http_error(401, None, "").http_status(), 401);
        assert_eq!(classify_http_error(429, None, "").kind_str(), "rate_limited");
        assert_eq!(classify_http_error(503, None, "").kind_str(), "upstream");
    }

    /// Los tres cuerpos reales de OpenCode Zen capturados el 2026-07-31, los tres
    /// con HTTP 401 aunque signifiquen cosas completamente distintas. Confiar en
    /// el status habría mostrado «clave inválida» para un simple problema de saldo
    /// — el caso que el usuario vio de verdad probando en Msty.
    #[test]
    fn zen_credits_error_is_distinguished_from_an_invalid_key() {
        let body = r#"{"type":"error","error":{"type":"CreditsError","message":"Insufficient balance. Manage your billing here: https://opencode.ai/workspace/wrk_01KN81JJVZJ890QY3PCB3CPPS4/billing"}}"#;
        let err = classify_http_error(401, None, body);
        match &err {
            AdapterError::Auth { reason, reauth_required } => {
                assert!(reason.contains("saldo"), "debe explicar que es de saldo: {reason}");
                assert!(
                    !reauth_required,
                    "reconectar la cuenta no arregla un problema de saldo"
                );
            }
            other => panic!("esperaba Auth, llegó {other:?}"),
        }
    }

    #[test]
    fn zen_model_error_is_unsupported_not_a_generic_502() {
        let body = r#"{"type":"error","error":{"type":"ModelError","message":"Model no-existe-9999 is not supported"}}"#;
        let err = classify_http_error(401, None, body);
        assert_eq!(err.http_status(), 422);
        match &err {
            AdapterError::Unsupported { hint, .. } => {
                assert!(hint.as_deref().unwrap_or("").contains("no-existe-9999"))
            }
            other => panic!("esperaba Unsupported, llegó {other:?}"),
        }
    }

    #[test]
    fn zen_auth_error_still_asks_to_reconnect() {
        let body = r#"{"type":"error","error":{"type":"AuthError","message":"Missing API key."}}"#;
        match classify_http_error(401, None, body) {
            AdapterError::Auth { reauth_required, .. } => assert!(reauth_required),
            other => panic!("esperaba Auth, llegó {other:?}"),
        }
    }

    #[test]
    fn an_unrecognised_error_type_keeps_the_real_http_status() {
        let body = r#"{"type":"error","error":{"type":"SomeNewErrorType","message":"boom"}}"#;
        match classify_http_error(500, None, body) {
            AdapterError::Upstream { status, provider_code, .. } => {
                assert_eq!(status, 500, "no debe inventarse un status distinto del real");
                assert_eq!(provider_code.as_deref(), Some("SomeNewErrorType"));
            }
            other => panic!("esperaba Upstream, llegó {other:?}"),
        }
    }

    #[test]
    fn a_plain_openai_style_body_without_the_zen_envelope_still_falls_back() {
        // Forma de OpenAI: {"error": {"message": ..., "type": "invalid_request_error"}}
        // sin ningún tipo de la lista reconocida de Zen — debe caer al status,
        // no fallar al parsear.
        let body = r#"{"error":{"message":"algo","type":"invalid_request_error"}}"#;
        assert_eq!(classify_http_error(400, None, body).kind_str(), "upstream");
    }

    /// Cuerpo real capturado de Gemini el 2026-08-03 para una clave inválida:
    /// HTTP 400 (no 401/403), sobre `google.rpc.Status` (`status`, no `type`).
    /// Sin reconocer esta forma, caía al genérico y el cliente veía un 502 en
    /// vez de un error de credencial — justo lo que el criterio 5 de la spec
    /// 0008 prohíbe.
    #[test]
    fn gemini_invalid_api_key_is_recognised_despite_the_400_status() {
        let body = r#"{"error":{"code":400,"message":"Please pass a valid API key","status":"INVALID_ARGUMENT"}}"#;
        match classify_http_error(400, None, body) {
            AdapterError::Auth { reason, reauth_required } => {
                assert!(reason.contains("API key"), "debe conservar el motivo real: {reason}");
                assert!(reauth_required);
            }
            other => panic!("esperaba Auth, llegó {other:?}"),
        }
    }

    /// Gemini envuelve algunos errores (verificado con un modelo inexistente,
    /// 2026-08-03) en un array de un elemento, a diferencia del sobre plano
    /// que comparten OpenAI y Zen.
    #[test]
    fn gemini_wraps_some_errors_in_a_single_element_array() {
        let body = r#"[{"error":{"code":404,"message":"models/no-existe is not found for API version v1main, or is not supported for generateContent.","status":"NOT_FOUND"}}]"#;
        match classify_http_error(404, None, body) {
            AdapterError::Unsupported { hint, .. } => {
                assert!(hint.as_deref().unwrap_or("").contains("no-existe"))
            }
            other => panic!("esperaba Unsupported, llegó {other:?}"),
        }
    }

    /// Un `INVALID_ARGUMENT` que no habla de la clave (cuerpo malformado,
    /// parámetro fuera de rango…) no debe clasificarse como error de
    /// credencial solo por compartir el mismo código.
    #[test]
    fn an_invalid_argument_unrelated_to_the_api_key_stays_upstream() {
        let body = r#"{"error":{"code":400,"message":"max_tokens debe ser un entero positivo","status":"INVALID_ARGUMENT"}}"#;
        assert_eq!(classify_http_error(400, None, body).kind_str(), "upstream");
    }

    /// `NOT_FOUND` sin relación con el modelo (por ejemplo, un recurso interno
    /// que no existe) tampoco debe pasar por «modelo no soportado» solo por
    /// coincidir el código de estado.
    #[test]
    fn a_not_found_unrelated_to_the_model_stays_upstream() {
        let body = r#"{"error":{"code":404,"message":"El recurso solicitado no existe","status":"NOT_FOUND"}}"#;
        assert_eq!(classify_http_error(404, None, body).kind_str(), "upstream");
    }
}
