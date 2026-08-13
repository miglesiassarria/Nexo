//! Rutas del gateway local.

use crate::gateway::wire::{error_body, ChunkBuilder, WireChatRequest};
use crate::provider::{AdapterError, ChatEvent, EventStream, FinishReason, ToolCall};
use crate::service::{Collector, Nexo, Prepared};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

pub fn router(nexo: Arc<Nexo>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .fallback(not_found)
        .with_state(nexo)
}

async fn healthz(State(nexo): State<Arc<Nexo>>) -> Json<Value> {
    Json(json!({
        "status": if nexo.is_paused() { "paused" } else { "ok" },
        "service": "nexo",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": {
                "message": "ruta no soportada por Nexo. Disponibles: \
                            GET /v1/models, POST /v1/chat/completions, GET /healthz",
                "type": "invalid_request_error",
                "code": "unknown_route",
            }
        })),
    )
        .into_response()
}

/// Extrae el bearer token. Nexo no acepta credenciales por query string.
fn bearer(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn unauthorized(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": {
                "message": message,
                "type": "invalid_request_error",
                "code": "invalid_api_key",
            }
        })),
    )
        .into_response()
}

fn simple_error(status: StatusCode, message: &str, code: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "message": message,
                "type": "invalid_request_error",
                "code": code,
            }
        })),
    )
        .into_response()
}

fn adapter_response(err: &AdapterError) -> Response {
    let status =
        StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = (status, Json(error_body(err))).into_response();
    if let AdapterError::RateLimited { retry_after: Some(d) } = err {
        if let Ok(value) = d.as_secs().to_string().parse() {
            response.headers_mut().insert("retry-after", value);
        }
    }
    response
}

async fn models(State(nexo): State<Arc<Nexo>>, headers: HeaderMap) -> Response {
    let Some(token) = bearer(&headers) else {
        return unauthorized("falta la cabecera Authorization: Bearer <token de Nexo>");
    };
    let Ok(Some(app)) = nexo.db().authenticate(&token) else {
        return unauthorized("el token no es válido o ha sido revocado");
    };

    match nexo.models_for_app(&app.id) {
        Ok(models) => Json(json!({"object": "list", "data": models})).into_response(),
        Err(e) => simple_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &e.to_string(),
            "internal_error",
        ),
    }
}

async fn chat_completions(
    State(nexo): State<Arc<Nexo>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(token) = bearer(&headers) else {
        return unauthorized("falta la cabecera Authorization: Bearer <token de Nexo>");
    };
    let Ok(Some(app)) = nexo.db().authenticate(&token) else {
        return unauthorized("el token no es válido o ha sido revocado");
    };

    if nexo.is_paused() {
        return simple_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Nexo está en pausa. Reanúdalo desde el icono de la barra de estado.",
            "gateway_paused",
        );
    }

    let wire: WireChatRequest = match serde_json::from_slice(&body) {
        Ok(w) => w,
        Err(e) => {
            return simple_error(
                StatusCode::BAD_REQUEST,
                &format!("cuerpo inválido: {e}"),
                "invalid_body",
            )
        }
    };

    let streaming = wire.stream;

    let prepared = match nexo.prepare(&app.id, wire).await {
        Ok(p) => p,
        Err(e) => return adapter_response(&e),
    };

    let builder = ChunkBuilder::new(prepared.public_model());
    let stream = match nexo.open_stream(&prepared).await {
        Ok(s) => s,
        Err(e) => {
            nexo.finish_failed(&prepared, &e);
            return adapter_response(&e);
        }
    };

    if streaming {
        stream_response(nexo, prepared, builder, stream)
    } else {
        collect_response(nexo, prepared, builder, stream).await
    }
}

fn sse(value: Value) -> Result<Event, Infallible> {
    Ok(Event::default().data(value.to_string()))
}

fn stream_response(
    nexo: Arc<Nexo>,
    prepared: Prepared,
    builder: ChunkBuilder,
    stream: EventStream,
) -> Response {
    let accounting = prepared.accounting;

    // El primer chunk anuncia el rol, como hacen los clientes de OpenAI. Se
    // materializa antes de mover el builder para que todos los chunks de la
    // respuesta compartan el mismo id.
    let role = builder.role_chunk();
    let head = futures::stream::once(async move { sse(role) });

    // El cierre —chunk de uso, `[DONE]` y registro en estadísticas— se hace
    // cuando el stream del proveedor se agota, no al ver `Finished`. El uso
    // puede llegar *después* de ese evento: así lo manda OpenCode Zen y así lo
    // manda la propia API de OpenAI con `include_usage`. Cerrar en `Finished`
    // costó dos fallos reales: la petición se registraba una vez por cada
    // evento posterior, y sin los tokens que aún no habían llegado.
    let state = std::sync::Arc::new(std::sync::Mutex::new((
        builder,
        Collector::since(prepared.started),
    )));

    let body = stream.flat_map({
        let state = state.clone();
        move |item| {
            let mut guard = state.lock().expect("estado del stream");
            let (builder, collector) = &mut *guard;
            let mut out: Vec<Result<Event, Infallible>> = Vec::new();

            match item {
                Ok(event) => {
                    collector.observe(&event);
                    match &event {
                        ChatEvent::TextDelta { text } => {
                            out.push(sse(builder.text_chunk(text)))
                        }
                        ChatEvent::ReasoningDelta { text } => {
                            out.push(sse(builder.reasoning_chunk(text)))
                        }
                        ChatEvent::ToolCallStart { id, name } => {
                            out.push(sse(builder.tool_start_chunk(id, name)))
                        }
                        ChatEvent::ToolCallDelta { id, args_json } => {
                            out.push(sse(builder.tool_args_chunk(id, args_json)))
                        }
                        ChatEvent::ToolCallArgumentsDone { id, args_json } => {
                            if let Some(chunk) = builder.tool_final_args_chunk(id, args_json) {
                                out.push(sse(chunk));
                            }
                        }
                        ChatEvent::Finished { reason } => {
                            out.push(sse(builder.finish_chunk(*reason)))
                        }
                        ChatEvent::Started { .. }
                        | ChatEvent::ToolCallEnd { .. }
                        | ChatEvent::Usage(_) => {}
                    }
                }
                Err(err) => {
                    // Un fallo a mitad de stream no puede cambiar el código
                    // HTTP, que ya se envió: se emite como evento y se cierra.
                    collector.observe_error(&err);
                    out.push(sse(error_body(&err)));
                }
            }

            futures::stream::iter(out)
        }
    });

    let tail = futures::stream::once(async move {
        let guard = state.lock().expect("estado del stream");
        let (builder, collector) = &*guard;
        let usage = collector.usage();
        let basis = accounting.cost_basis_for(usage.source);
        nexo.finish(&prepared, collector);
        vec![
            sse(builder.usage_chunk(&usage, basis.as_str())),
            Ok(Event::default().data("[DONE]")),
        ]
    })
    .flat_map(futures::stream::iter);

    Sse::new(head.chain(body).chain(tail))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

async fn collect_response(
    nexo: Arc<Nexo>,
    prepared: Prepared,
    builder: ChunkBuilder,
    stream: EventStream,
) -> Response {
    let mut stream = stream;
    let mut collector = Collector::since(prepared.started);
    let mut text = String::new();
    let mut calls: Vec<ToolCall> = Vec::new();
    let mut failure: Option<AdapterError> = None;

    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => {
                collector.observe(&event);
                match event {
                    ChatEvent::TextDelta { text: t } => text.push_str(&t),
                    ChatEvent::ToolCallStart { id, name } => calls.push(ToolCall {
                        id,
                        name,
                        arguments_json: String::new(),
                    }),
                    ChatEvent::ToolCallDelta { id, args_json } => {
                        if let Some(c) = calls.iter_mut().find(|c| c.id == id) {
                            c.arguments_json.push_str(&args_json);
                        }
                    }
                    ChatEvent::ToolCallArgumentsDone { id, args_json } => {
                        if let Some(c) = calls.iter_mut().find(|c| c.id == id) {
                            c.arguments_json = args_json;
                        }
                    }
                    _ => {}
                }
            }
            Err(e) => {
                collector.observe_error(&e);
                failure = Some(e);
                break;
            }
        }
    }

    if let Some(err) = failure {
        nexo.finish(&prepared, &collector);
        return adapter_response(&err);
    }

    let usage = collector.usage();
    let basis = prepared.accounting.cost_basis_for(usage.source);
    let reason = collector.finish_reason().unwrap_or(FinishReason::Stop);
    let response = builder.full_response(&text, &calls, reason, &usage, basis.as_str());
    nexo.finish(&prepared, &collector);
    Json(response).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::AUTHORIZATION;

    fn headers_with(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, value.parse().unwrap());
        h
    }

    #[test]
    fn bearer_accepts_both_capitalisations() {
        assert_eq!(bearer(&headers_with("Bearer nx_a")).as_deref(), Some("nx_a"));
        assert_eq!(bearer(&headers_with("bearer nx_a")).as_deref(), Some("nx_a"));
    }

    #[test]
    fn bearer_rejects_missing_or_empty_tokens() {
        assert!(bearer(&HeaderMap::new()).is_none());
        assert!(bearer(&headers_with("Bearer ")).is_none());
        assert!(bearer(&headers_with("nx_sin_prefijo")).is_none());
        assert!(bearer(&headers_with("Basic dXNlcjpwYXNz")).is_none());
    }

    #[test]
    fn rate_limit_response_carries_retry_after_header() {
        let err = AdapterError::RateLimited {
            retry_after: Some(Duration::from_secs(42)),
        };
        let resp = adapter_response(&err);
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(resp.headers().get("retry-after").unwrap(), "42");
    }

    #[test]
    fn unsupported_capability_maps_to_422() {
        let err = AdapterError::Unsupported {
            capability: "vision".into(),
            hint: None,
        };
        assert_eq!(
            adapter_response(&err).status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[test]
    fn broken_subscription_maps_to_502() {
        let err = AdapterError::SubscriptionPathBroken {
            provider: "openai".into(),
            detail: "404".into(),
        };
        assert_eq!(adapter_response(&err).status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn local_limit_maps_to_429_without_retry_after() {
        let err = AdapterError::LocalLimit {
            app_id: "a".into(),
            window_secs: 60,
            detail: String::new(),
        };
        let resp = adapter_response(&err);
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(resp.headers().get("retry-after").is_none());
    }
}
