//! Pruebas de extremo a extremo del gateway por HTTP real.
//!
//! Usan el proveedor mock: ejercitan autenticación, permisos, límites,
//! streaming SSE, forma de los chunks y registro de estadísticas sin gastar
//! cuota ni depender de la ruta frágil de suscripción.

use nexo_core::db::Db;
use nexo_core::gateway;
use nexo_core::provider::CredentialKind;
use nexo_core::secrets::MemorySecretStore;
use nexo_core::service::Nexo;
use serde_json::{json, Value};
use std::sync::Arc;

struct Harness {
    base: String,
    token: String,
    nexo: Arc<Nexo>,
    http: reqwest::Client,
}

async fn start() -> Harness {
    start_with_limit(None).await
}

async fn start_with_limit(max_requests: Option<i64>) -> Harness {
    let db = Db::open_in_memory().expect("db");
    let nexo = Nexo::new(db, Arc::new(MemorySecretStore::default())).expect("nexo");

    let issued = nexo.db().create_app("prueba", None).expect("app");
    nexo.db()
        .grant_with_mandatory_limit(
            &issued.app.id,
            "mock",
            CredentialKind::Mock,
            false,
            false,
            None,
            None,
        )
        .expect("grant");

    if let Some(max) = max_requests {
        nexo.db()
            .set_limit(
                &issued.app.id,
                &nexo_core::apps::Limit {
                    provider_id: "mock".into(),
                    credential_kind: "mock".into(),
                    window_seconds: 60,
                    max_requests: Some(max),
                    max_input_tokens: None,
                    max_output_tokens: None,
                },
            )
            .expect("limit");
    }

    let listener = gateway::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind");
    let port = listener.local_addr().unwrap().port();

    let serving = nexo.clone();
    tokio::spawn(async move {
        let _ = gateway::serve_on(serving, listener).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    Harness {
        base: format!("http://127.0.0.1:{port}"),
        token: issued.token,
        nexo,
        http: reqwest::Client::new(),
    }
}

impl Harness {
    async fn post_chat(&self, body: Value) -> reqwest::Response {
        self.http
            .post(format!("{}/v1/chat/completions", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .expect("respuesta")
    }

    fn simple_body(&self, stream: bool) -> Value {
        json!({
            "model": "mock/mock-echo",
            "messages": [{"role": "user", "content": "hola mundo"}],
            "stream": stream,
        })
    }
}

/// Extrae los objetos JSON de un cuerpo SSE, ignorando `[DONE]`.
fn parse_sse(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|d| d.trim() != "[DONE]")
        .filter_map(|d| serde_json::from_str(d).ok())
        .collect()
}

#[tokio::test]
async fn healthz_reports_status_without_a_token() {
    let h = start().await;
    let body: Value = h
        .http
        .get(format!("{}/healthz", h.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "nexo");
}

#[tokio::test]
async fn requests_without_a_token_are_rejected() {
    let h = start().await;
    let resp = h
        .http
        .post(format!("{}/v1/chat/completions", h.base))
        .json(&h.simple_body(false))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn a_revoked_token_stops_working_immediately() {
    let h = start().await;
    assert!(h.post_chat(h.simple_body(false)).await.status().is_success());

    let app_id = h.nexo.db().apps().unwrap()[0].id.clone();
    h.nexo.db().revoke_app(&app_id).unwrap();

    assert_eq!(h.post_chat(h.simple_body(false)).await.status(), 401);
}

#[tokio::test]
async fn models_endpoint_annotates_the_access_route() {
    let h = start().await;
    let body: Value = h
        .http
        .get(format!("{}/v1/models", h.base))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(body["object"], "list");
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], "mock/mock-echo");
    assert_eq!(data[0]["nexo"]["credential_kind"], "mock");
    assert_eq!(data[0]["nexo"]["accounting"], "local");
    assert_eq!(data[0]["nexo"]["capabilities"]["tools"], false);
}

#[tokio::test]
async fn non_streaming_response_has_openai_shape() {
    let h = start().await;
    let resp = h.post_chat(h.simple_body(false)).await;
    assert!(resp.status().is_success());

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert_eq!(body["model"], "mock/mock-echo");
    assert_eq!(body["choices"][0]["message"]["role"], "assistant");
    assert_eq!(body["choices"][0]["message"]["content"], "eco: hola mundo");
    assert_eq!(body["choices"][0]["finish_reason"], "stop");
    // El mock estima su uso; el bloque `nexo` lo declara sin ambigüedad.
    assert_eq!(body["usage"]["nexo"]["usage_source"], "estimated");
}

#[tokio::test]
async fn streaming_response_is_sse_and_reassembles_to_the_same_text() {
    let h = start().await;
    let resp = h.post_chat(h.simple_body(true)).await;
    assert!(resp.status().is_success());
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("text/event-stream"));

    let raw = resp.text().await.unwrap();
    assert!(raw.trim_end().ends_with("data: [DONE]"), "falta el centinela final");

    let chunks = parse_sse(&raw);

    // Todos los chunks pertenecen a la misma respuesta.
    let ids: std::collections::HashSet<&str> =
        chunks.iter().filter_map(|c| c["id"].as_str()).collect();
    assert_eq!(ids.len(), 1, "los chunks deben compartir un único id");

    // El primero anuncia el rol.
    assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");

    let text: String = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert_eq!(text, "eco: hola mundo");

    // Exactamente un finish_reason.
    let finishes: Vec<&Value> = chunks
        .iter()
        .filter(|c| !c["choices"][0]["finish_reason"].is_null())
        .collect();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0]["choices"][0]["finish_reason"], "stop");

    // El chunk de uso llega al final y declara el origen del dato.
    let usage = chunks
        .iter()
        .rev()
        .find(|c| !c["usage"].is_null())
        .expect("chunk de uso");
    assert_eq!(usage["usage"]["nexo"]["usage_source"], "estimated");
    assert_eq!(usage["usage"]["nexo"]["cost_basis"], "reported");
}

/// Reproduce lo que se vio con Zen real: el uso llega en un fragmento
/// posterior a `Finished`, igual que hace la propia API de OpenAI con
/// `include_usage`. Cubre los dos fallos encontrados en ese caso: registrar la
/// misma petición una vez por cada evento posterior al cierre, y registrarla
/// sin tokens por hacerlo antes de que llegara el uso.
#[tokio::test]
async fn usage_that_arrives_after_finished_is_recorded_once_and_with_its_tokens() {
    let h = start().await;
    // El modelo con el evento de más solo se añade a este catálogo de
    // prueba: no forma parte del manifiesto real que ven los usuarios.
    h.nexo
        .db()
        .replace_models(
            "mock",
            CredentialKind::Mock,
            &[
                nexo_core::provider::mock::MockAdapter::descriptor(),
                nexo_core::provider::mock::MockAdapter::trailing_event_descriptor(),
            ],
            nexo_core::catalog::MANIFEST_VERSION,
        )
        .expect("catálogo de prueba");

    let mut body = h.simple_body(true);
    body["model"] = json!("mock/mock-trailing-event");

    let resp = h.post_chat(body).await;
    assert!(resp.status().is_success());
    let raw = resp.text().await.unwrap();

    let recent = h.nexo.db().recent_requests(0, 10).unwrap();
    assert_eq!(
        recent.len(),
        1,
        "una sola petición debe dejar una sola fila, no una por cada evento tras el cierre"
    );
    assert!(
        recent[0].total_tokens.unwrap_or(0) > 0,
        "el uso llegó después de `Finished`: debe registrarse igual, no como no disponible"
    );
    assert_eq!(recent[0].usage_source, "estimated");

    // Y el cliente sigue viendo un único cierre bien formado.
    let chunks = parse_sse(&raw);
    assert_eq!(
        chunks.iter().filter(|c| !c["usage"].is_null()).count(),
        1,
        "un solo chunk de uso"
    );
    assert_eq!(raw.matches("data: [DONE]").count(), 1, "un solo centinela final");
}

// -- Modelos permitidos por aplicación (spec 0004) -------------------------

/// Prepara una aplicación con dos modelos del mock en catálogo y ninguno marcado.
/// Devuelve el arnés y los nombres públicos de los dos modelos.
async fn start_with_two_mock_models() -> (Harness, String, String) {
    let h = start().await;
    h.nexo
        .db()
        .replace_models(
            "mock",
            CredentialKind::Mock,
            &[
                nexo_core::provider::mock::MockAdapter::descriptor(),
                nexo_core::provider::mock::MockAdapter::trailing_event_descriptor(),
            ],
            nexo_core::catalog::MANIFEST_VERSION,
        )
        .expect("catálogo de prueba");
    (
        h,
        "mock/mock-echo".to_string(),
        "mock/mock-trailing-event".to_string(),
    )
}

fn app_id(h: &Harness) -> String {
    h.nexo.db().apps().unwrap()[0].id.clone()
}

async fn models_listed(h: &Harness) -> Vec<String> {
    let body: Value = h
        .http
        .get(format!("{}/v1/models", h.base))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect()
}

/// Criterio 1: el catálogo devuelve solo lo marcado, no todo lo de la vía.
#[tokio::test]
async fn the_catalog_only_lists_the_models_the_app_has_marked() {
    let (h, uno, dos) = start_with_two_mock_models().await;

    // El permiso de partida es `*`: los dos modelos se listan.
    let todos = models_listed(&h).await;
    assert!(todos.contains(&uno) && todos.contains(&dos));

    // Se marca solo uno.
    h.nexo
        .db()
        .replace_app_models(
            &app_id(&h),
            "mock",
            CredentialKind::Mock,
            std::slice::from_ref(&uno),
            true,
            true,
            None,
            None,
        )
        .unwrap();

    assert_eq!(models_listed(&h).await, vec![uno]);
}

/// Criterio 2: un modelo no marcado se rechaza nombrándolo, y no se sirve otro en su
/// lugar. Es la invariante de no degradar en silencio.
#[tokio::test]
async fn a_model_that_is_not_marked_is_refused_by_name_and_nothing_else_is_served() {
    let (h, uno, dos) = start_with_two_mock_models().await;
    h.nexo
        .db()
        .replace_app_models(
            &app_id(&h),
            "mock",
            CredentialKind::Mock,
            &[uno],
            true,
            true,
            None,
            None,
        )
        .unwrap();

    let resp = h
        .post_chat(json!({
            "model": dos,
            "messages": [{"role": "user", "content": "hola"}]
        }))
        .await;

    assert!(!resp.status().is_success(), "no puede atenderse");
    let body: Value = resp.json().await.unwrap();
    let message = body["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.contains(&dos),
        "el error debe nombrar el modelo rechazado: {message}"
    );
    assert!(
        body["choices"].is_null(),
        "no se sirve otro modelo en su lugar"
    );
}

/// Criterio 3: y el modelo que sí está marcado sigue funcionando igual.
#[tokio::test]
async fn a_marked_model_still_works() {
    let (h, uno, _dos) = start_with_two_mock_models().await;
    h.nexo
        .db()
        .replace_app_models(
            &app_id(&h),
            "mock",
            CredentialKind::Mock,
            std::slice::from_ref(&uno),
            true,
            true,
            None,
            None,
        )
        .unwrap();

    let resp = h
        .post_chat(json!({
            "model": uno,
            "messages": [{"role": "user", "content": "hola mundo"}]
        }))
        .await;
    assert!(resp.status().is_success(), "estado: {}", resp.status());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["model"], uno);
    assert!(!body["choices"][0]["message"]["content"].is_null());
}

/// Criterio 6: un permiso heredado con `*` sigue sirviendo todos los modelos de su
/// vía. Es lo que impide que esta versión rompa las aplicaciones que ya funcionan.
#[tokio::test]
async fn an_inherited_wildcard_grant_keeps_serving_every_model_over_http() {
    let (h, uno, dos) = start_with_two_mock_models().await;

    // El arnés concede con `*`, igual que las aplicaciones ya existentes.
    let grants = h.nexo.db().grants(&app_id(&h)).unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].model_pattern, "*");

    let listed = models_listed(&h).await;
    assert!(listed.contains(&uno) && listed.contains(&dos));

    for model in [uno, dos] {
        let resp = h
            .post_chat(json!({
                "model": model,
                "messages": [{"role": "user", "content": "hola"}]
            }))
            .await;
        assert!(resp.status().is_success(), "«{model}» debía seguir sirviéndose");
    }
}

/// El tiempo hasta el primer token se mide desde que arrancó la petición, no
/// desde el evento `Started`. En `chat/completions` ese evento y el primer
/// trozo de texto salen del mismo fragmento SSE, así que medir entre ellos
/// daba siempre 0 ms: en el panel, ocho segundos de espera aparecían como
/// «0 ms al primer token» para Zen, OpenAI por API key y LM Studio.
#[tokio::test]
async fn time_to_first_token_is_measured_from_the_start_of_the_request() {
    let h = start().await;
    h.nexo
        .db()
        .replace_models(
            "mock",
            CredentialKind::Mock,
            &[
                nexo_core::provider::mock::MockAdapter::descriptor(),
                nexo_core::provider::mock::MockAdapter::slow_start_descriptor(),
            ],
            nexo_core::catalog::MANIFEST_VERSION,
        )
        .expect("catálogo de prueba");

    let mut body = h.simple_body(true);
    body["model"] = json!("mock/mock-slow-start");
    h.post_chat(body).await.text().await.unwrap();

    let row = h
        .nexo
        .db()
        .recent_requests(0, 5)
        .unwrap()
        .into_iter()
        .find(|r| r.operation == "chat")
        .expect("la petición queda registrada");

    let ttft = row.ttft_ms.expect("debe medirse el tiempo al primer token");
    let espera = nexo_core::provider::mock::SLOW_START_DELAY.as_millis() as i64;
    assert!(
        ttft >= espera / 2,
        "el proveedor tardó {espera} ms en arrancar y se registraron {ttft} ms"
    );
    assert!(
        ttft <= row.latency_ms.unwrap(),
        "el primer token no puede llegar después del final de la petición"
    );
}

#[tokio::test]
async fn unknown_model_is_rejected_with_a_pointer_to_the_catalog() {
    let h = start().await;
    let resp = h
        .post_chat(json!({
            "model": "no-existe",
            "messages": [{"role": "user", "content": "hola"}]
        }))
        .await;
    assert_eq!(resp.status(), 422);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unsupported_capability");
    assert!(body["error"]["nexo"]["hint"]
        .as_str()
        .unwrap()
        .contains("/v1/models"));
}

#[tokio::test]
async fn unsupported_capability_is_refused_not_silently_dropped() {
    let h = start().await;
    let resp = h
        .post_chat(json!({
            "model": "mock/mock-echo",
            "messages": [{"role": "user", "content": "hola"}],
            "tools": [{"type": "function", "function": {"name": "t", "parameters": {}}}]
        }))
        .await;
    assert_eq!(resp.status(), 422);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["nexo"]["kind"], "unsupported");
}

#[tokio::test]
async fn local_limit_returns_429_and_says_who_limited() {
    let h = start_with_limit(Some(1)).await;
    assert!(h.post_chat(h.simple_body(false)).await.status().is_success());

    let resp = h.post_chat(h.simple_body(false)).await;
    assert_eq!(resp.status(), 429);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "nexo_app_limit_exceeded");
    assert_eq!(body["error"]["nexo"]["limited_by"], "nexo");
    assert_eq!(body["error"]["nexo"]["window_seconds"], 60);
}

#[tokio::test]
async fn paused_gateway_refuses_traffic_but_stays_reachable() {
    let h = start().await;
    h.nexo.set_paused(true);

    let resp = h.post_chat(h.simple_body(false)).await;
    assert_eq!(resp.status(), 503);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "gateway_paused");

    let health: Value = h
        .http
        .get(format!("{}/healthz", h.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["status"], "paused");

    h.nexo.set_paused(false);
    assert!(h.post_chat(h.simple_body(false)).await.status().is_success());
}

#[tokio::test]
async fn unknown_routes_explain_what_is_available() {
    let h = start().await;
    let resp = h
        .http
        .get(format!("{}/v1/embeddings", h.base))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unknown_route");
}

#[tokio::test]
async fn malformed_body_is_a_400_not_a_500() {
    let h = start().await;
    let resp = h
        .http
        .post(format!("{}/v1/chat/completions", h.base))
        .bearer_auth(&h.token)
        .header("content-type", "application/json")
        .body("{ esto no es json }")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn every_request_lands_in_the_statistics() {
    let h = start().await;
    h.post_chat(h.simple_body(true)).await.text().await.unwrap();
    h.post_chat(h.simple_body(false)).await.text().await.unwrap();

    let recent = h.nexo.db().recent_requests(0, 10).unwrap();
    assert_eq!(recent.len(), 2);
    for row in &recent {
        assert_eq!(row.status, "ok");
        assert_eq!(row.app, "prueba");
        assert_eq!(row.public_model, "mock/mock-echo");
        assert!(row.latency_ms.is_some());
        assert!(row.ttft_ms.is_some(), "debe medirse el tiempo al primer token");
    }

    let summary = h
        .nexo
        .db()
        .usage_summary(0, nexo_core::db::stats::GroupBy::CredentialKind, Some("chat"))
        .unwrap();
    assert_eq!(summary.len(), 1);
    assert_eq!(summary[0].requests, 2);
    assert!(summary[0].total_tokens > 0);
}

#[tokio::test]
async fn rejected_requests_are_also_recorded_and_distinguishable() {
    let h = start_with_limit(Some(1)).await;
    h.post_chat(h.simple_body(false)).await;
    h.post_chat(h.simple_body(false)).await;

    let summary = h
        .nexo
        .db()
        .usage_summary(0, nexo_core::db::stats::GroupBy::App, Some("chat"))
        .unwrap();
    assert_eq!(summary[0].requests, 2);
    assert_eq!(summary[0].errors, 1);
    assert_eq!(
        summary[0].local_limited, 1,
        "un límite de Nexo no puede confundirse con un 429 del proveedor"
    );
    assert_eq!(summary[0].rate_limited, 0);
}

#[tokio::test]
async fn an_empty_model_list_leaves_a_diagnosable_trace() {
    // Reproduce el caso que costó tres intentos diagnosticar: la aplicación se
    // autentica bien pero no tiene ninguna vía concedida, así que el cliente
    // recibe cero modelos y muestra «no se encontraron modelos» sin más pista.
    let db = Db::open_in_memory().expect("db");
    let nexo = Nexo::new(db, Arc::new(MemorySecretStore::default())).expect("nexo");
    let issued = nexo.db().create_app("sin permisos", None).expect("app");

    let listener = gateway::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind");
    let port = listener.local_addr().unwrap().port();
    let serving = nexo.clone();
    tokio::spawn(async move {
        let _ = gateway::serve_on(serving, listener).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    let body: Value = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/v1/models"))
        .bearer_auth(&issued.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(body["data"].as_array().unwrap().is_empty());

    // El panel debe poder explicar por qué.
    let recent = nexo.db().recent_requests(0, 10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].operation, "models");
    assert_eq!(recent[0].status, "error");
    assert_eq!(
        recent[0].error_kind.as_deref(),
        Some("no_grants"),
        "el motivo del catálogo vacío tiene que quedar registrado"
    );

    // Y no puede contaminar los totales de inferencia.
    let chat = nexo
        .db()
        .usage_summary(0, nexo_core::db::stats::GroupBy::App, Some("chat"))
        .unwrap();
    assert!(chat.is_empty(), "una consulta de catálogo no es una petición de uso");
}

#[tokio::test]
async fn a_successful_catalog_query_is_recorded_without_an_error() {
    let h = start().await;
    h.http
        .get(format!("{}/v1/models", h.base))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap();

    let recent = h.nexo.db().recent_requests(0, 10).unwrap();
    let models: Vec<_> = recent.iter().filter(|r| r.operation == "models").collect();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].status, "ok");
    assert_eq!(models[0].error_kind, None);
    assert!(models[0].public_model.contains("1 modelo"));
}

#[tokio::test]
async fn catalog_queries_do_not_consume_the_app_limit() {
    let h = start_with_limit(Some(1)).await;
    // Diez consultas de catálogo no deben gastar la única petición permitida.
    for _ in 0..10 {
        h.http
            .get(format!("{}/v1/models", h.base))
            .bearer_auth(&h.token)
            .send()
            .await
            .unwrap();
    }
    assert!(
        h.post_chat(h.simple_body(false)).await.status().is_success(),
        "el límite es de inferencia, no de consultas de catálogo"
    );
}

// ---------------------------------------------------------------------------
// LM Studio real. Marcadas `#[ignore]` porque exigen tenerlo abierto:
//   cargo test -p nexo-core --test gateway_e2e -- --ignored lmstudio
// ---------------------------------------------------------------------------

/// Monta un Nexo con LM Studio detectado y una aplicación con acceso local.
///
/// Devuelve `None` si LM Studio no está en marcha, para que la prueba se salte de
/// forma explícita en lugar de fallar por algo que no es un defecto del código.
async fn start_with_lmstudio() -> Option<Harness> {
    let db = Db::open_in_memory().expect("db");
    let nexo = Nexo::new(db, Arc::new(MemorySecretStore::default())).expect("nexo");

    let status = nexo.detect_lmstudio().await.expect("detección");
    if !status.reachable {
        eprintln!("LM Studio no está en marcha ({:?}); prueba omitida", status.detail);
        return None;
    }

    let issued = nexo.db().create_app("prueba-local", None).expect("app");
    // Sin límite a propósito: la vía local no debe exigirlo (criterio 9).
    nexo.db()
        .set_grant(
            &issued.app.id,
            &nexo_core::apps::Grant {
                provider_id: "lmstudio".into(),
                credential_kind: "local".into(),
                model_pattern: "*".into(),
                allow_tools: true,
                allow_multimodal: true,
                log_content: false,
            },
        )
        .expect("grant");

    let listener = gateway::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind");
    let port = listener.local_addr().unwrap().port();
    let serving = nexo.clone();
    tokio::spawn(async move {
        let _ = gateway::serve_on(serving, listener).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    Some(Harness {
        base: format!("http://127.0.0.1:{port}"),
        token: issued.token,
        nexo,
        http: reqwest::Client::new(),
    })
}

/// Primer modelo del catálogo local que sirva para chat.
fn first_chat_model(h: &Harness) -> Option<String> {
    h.nexo
        .db()
        .catalog_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.provider_id == "lmstudio" && r.caps.text)
        .map(|r| r.public_name)
}

#[tokio::test]
#[ignore = "necesita LM Studio en marcha"]
async fn lmstudio_models_appear_with_the_local_route() {
    let Some(h) = start_with_lmstudio().await else { return };

    let body: Value = h
        .http
        .get(format!("{}/v1/models", h.base))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let locals: Vec<&Value> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["nexo"]["provider"] == "lmstudio")
        .collect();

    assert!(!locals.is_empty(), "el catálogo local debe aparecer");
    for m in &locals {
        assert_eq!(m["nexo"]["credential_kind"], "local");
        assert_eq!(m["nexo"]["accounting"], "local");
        assert_eq!(m["nexo"]["priced"], false, "lo local no cuesta por token");
        assert!(m["id"].as_str().unwrap().starts_with("lmstudio/"));
    }
}

#[tokio::test]
#[ignore = "necesita LM Studio en marcha"]
async fn lmstudio_chat_works_and_is_recorded_without_a_limit() {
    let Some(h) = start_with_lmstudio().await else { return };
    let Some(model) = first_chat_model(&h) else {
        eprintln!("no hay modelo de chat local; prueba omitida");
        return;
    };

    let resp = h
        .post_chat(json!({
            "model": model,
            "messages": [{"role": "user", "content": "Responde solo: OK"}],
            "max_tokens": 20
        }))
        .await;
    assert!(resp.status().is_success(), "estado: {}", resp.status());

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(!body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .is_empty());
    // LM Studio informa de tokens: verificado en T0.
    assert_eq!(body["usage"]["nexo"]["usage_source"], "reported");
    assert!(body["usage"]["total_tokens"].as_u64().unwrap() > 0);

    let recent = h.nexo.db().recent_requests(0, 10).unwrap();
    let row = recent
        .iter()
        .find(|r| r.operation == "chat")
        .expect("la petición quedó registrada");
    assert_eq!(row.status, "ok");
    assert_eq!(row.credential_kind, "local");
    assert!(row.latency_ms.is_some());
    assert!(row.total_tokens.unwrap_or(0) > 0);

    // Criterio 8: lo local no aporta coste estimado.
    let summary = h
        .nexo
        .db()
        .usage_summary(0, nexo_core::db::stats::GroupBy::CredentialKind, Some("chat"))
        .unwrap();
    let local = summary.iter().find(|b| b.bucket == "local").unwrap();
    assert_eq!(local.cost_estimated_micros, 0);
    assert_eq!(local.subscription_requests, 0);
}

#[tokio::test]
#[ignore = "necesita LM Studio en marcha"]
async fn lmstudio_streaming_reassembles_to_the_same_text() {
    let Some(h) = start_with_lmstudio().await else { return };
    let Some(model) = first_chat_model(&h) else { return };

    let raw = h
        .post_chat(json!({
            "model": model,
            "messages": [{"role": "user", "content": "Escribe exactamente: uno dos tres"}],
            "stream": true,
            "max_tokens": 30
        }))
        .await
        .text()
        .await
        .unwrap();

    assert!(raw.trim_end().ends_with("data: [DONE]"));
    let chunks = parse_sse(&raw);

    let ids: std::collections::HashSet<&str> =
        chunks.iter().filter_map(|c| c["id"].as_str()).collect();
    assert_eq!(ids.len(), 1, "un solo id para toda la respuesta");

    let text: String = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert!(!text.is_empty(), "el texto reensamblado no puede estar vacío");

    let finishes = chunks
        .iter()
        .filter(|c| !c["choices"][0]["finish_reason"].is_null())
        .count();
    assert_eq!(finishes, 1);

    let usage = chunks
        .iter()
        .rev()
        .find(|c| !c["usage"].is_null())
        .expect("LM Studio respeta include_usage: verificado en T0");
    assert_eq!(usage["usage"]["nexo"]["usage_source"], "reported");

    let ttft = h
        .nexo
        .db()
        .recent_requests(0, 5)
        .unwrap()
        .into_iter()
        .find(|r| r.operation == "chat")
        .and_then(|r| r.ttft_ms);
    assert!(ttft.is_some(), "hay que medir el tiempo hasta el primer token");
}

#[tokio::test]
#[ignore = "necesita LM Studio en marcha"]
async fn lmstudio_embeddings_model_refuses_chat_with_422() {
    let Some(h) = start_with_lmstudio().await else { return };

    let embeddings = h
        .nexo
        .db()
        .catalog_rows()
        .unwrap()
        .into_iter()
        .find(|r| r.provider_id == "lmstudio" && r.caps.embeddings);
    let Some(model) = embeddings else {
        eprintln!("no hay modelo de embeddings cargado en LM Studio; prueba omitida");
        return;
    };

    let resp = h
        .post_chat(json!({
            "model": model.public_name,
            "messages": [{"role": "user", "content": "hola"}]
        }))
        .await;
    assert_eq!(resp.status(), 422, "pedir chat a un embeddings se rechaza");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["nexo"]["kind"], "unsupported");
    assert_eq!(body["error"]["nexo"]["capability"], "text");
}

#[tokio::test]
async fn a_local_server_that_is_down_gives_a_useful_error_not_a_generic_502() {
    // Criterio 7. Se apunta a un puerto muerto en lugar de cerrar el LM Studio del
    // usuario: es el mismo camino de código y no interfiere con su equipo.
    let db = Db::open_in_memory().expect("db");
    let nexo = Nexo::new(db, Arc::new(MemorySecretStore::default())).expect("nexo");

    // Catálogo local sembrado a mano, como si LM Studio hubiera estado activo antes.
    nexo.db()
        .replace_models(
            "lmstudio",
            CredentialKind::Local,
            &[nexo_core::provider::ModelDescriptor {
                api_id: "modelo-fantasma".into(),
                public_name: "lmstudio/modelo-fantasma".into(),
                caps: nexo_core::provider::Capabilities {
                    text: true,
                    streaming: true,
                    ..Default::default()
                },
                limits: Default::default(),
                accounting: nexo_core::provider::Accounting::Local,
                pricing: None,
            }],
            "prueba",
        )
        .unwrap();

    // Cuenta local apuntando a un puerto donde no hay nada.
    nexo.db()
        .upsert_account(&nexo_core::db::Account {
            id: "acc-local".into(),
            provider_id: "lmstudio".into(),
            credential_kind: CredentialKind::Local,
            label: "LM Studio apagado".into(),
            keychain_ref: None,
            external_id: Some("http://127.0.0.1:1".into()),
            scopes: None,
            expires_at: None,
            status: "active".into(),
            risk_ack_at: None,
            created_at: 0,
            last_used_at: None,
        })
        .unwrap();

    let mut settings = nexo.db().settings().unwrap();
    settings.lmstudio_base_url = "http://127.0.0.1:1".into();
    nexo.db().save_settings(&settings).unwrap();

    let issued = nexo.db().create_app("cliente", None).unwrap();
    nexo.db()
        .set_grant(
            &issued.app.id,
            &nexo_core::apps::Grant {
                provider_id: "lmstudio".into(),
                credential_kind: "local".into(),
                model_pattern: "*".into(),
                allow_tools: false,
                allow_multimodal: false,
                log_content: false,
            },
        )
        .unwrap();

    let listener = gateway::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let serving = nexo.clone();
    tokio::spawn(async move {
        let _ = gateway::serve_on(serving, listener).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    let resp = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .bearer_auth(&issued.token)
        .json(&json!({
            "model": "lmstudio/modelo-fantasma",
            "messages": [{"role": "user", "content": "hola"}]
        }))
        .send()
        .await
        .unwrap();

    // 503, no 502: el destino no responde, no es que haya fallado.
    assert_eq!(resp.status(), 503);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["nexo"]["kind"], "transport");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("127.0.0.1:1"),
        "el error debe nombrar la dirección: {message}"
    );
    assert!(
        message.contains("abierto"),
        "el error debe decir qué hacer: {message}"
    );

    // Y queda registrado, para que el panel pueda explicarlo.
    let row = nexo
        .db()
        .recent_requests(0, 5)
        .unwrap()
        .into_iter()
        .find(|r| r.operation == "chat")
        .expect("el fallo queda registrado");
    assert_eq!(row.status, "error");
    assert_eq!(row.error_kind.as_deref(), Some("transport"));
    assert_eq!(row.credential_kind, "local");
}

// ---------------------------------------------------------------------------
// OpenCode Zen real. Marcadas `#[ignore]` porque exigen una clave:
//   NEXO_TEST_ZEN_API_KEY=sk-... cargo test -p nexo-core --test gateway_e2e -- --ignored zen
// ---------------------------------------------------------------------------

/// Monta un Nexo con OpenCode Zen añadido como proveedor genérico y una
/// aplicación con acceso. Devuelve `None` si no hay clave en el entorno, para que
/// la prueba se salte de forma explícita en lugar de fallar por algo que no es un
/// defecto del código.
async fn start_with_zen() -> Option<Harness> {
    let Ok(key) = std::env::var("NEXO_TEST_ZEN_API_KEY") else {
        eprintln!("NEXO_TEST_ZEN_API_KEY no está definida; prueba omitida");
        return None;
    };

    let db = Db::open_in_memory().expect("db");
    let nexo = Nexo::new(db, Arc::new(MemorySecretStore::default())).expect("nexo");

    nexo.add_custom_provider("OpenCode Zen", "https://opencode.ai/zen/v1", &key)
        .await
        .expect("añadir el proveedor no debe fallar con una clave válida");

    let issued = nexo.db().create_app("prueba-zen", None).expect("app");
    nexo.db()
        .set_grant(
            &issued.app.id,
            &nexo_core::apps::Grant {
                provider_id: "opencode-zen".into(),
                credential_kind: "api_key".into(),
                model_pattern: "*".into(),
                allow_tools: true,
                allow_multimodal: false,
                log_content: false,
            },
        )
        .expect("grant");

    let listener = gateway::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind");
    let port = listener.local_addr().unwrap().port();
    let serving = nexo.clone();
    tokio::spawn(async move {
        let _ = gateway::serve_on(serving, listener).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    Some(Harness {
        base: format!("http://127.0.0.1:{port}"),
        token: issued.token,
        nexo,
        http: reqwest::Client::new(),
    })
}

const ZEN_FREE_MODEL: &str = "opencode-zen/deepseek-v4-flash-free";

#[tokio::test]
#[ignore = "necesita NEXO_TEST_ZEN_API_KEY"]
async fn zen_discovers_its_real_catalog_through_the_generic_adapter() {
    let Some(h) = start_with_zen().await else { return };

    let body: Value = h
        .http
        .get(format!("{}/v1/models", h.base))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let models = body["data"].as_array().unwrap();
    // Criterio 9: Zen tiene 60 modelos reales el 2026-07-31; se admite margen
    // por si cambia el catálogo entre esa fecha y la ejecución de la prueba.
    assert!(models.len() > 30, "esperaba decenas de modelos, llegaron {}", models.len());
    assert!(
        models.iter().any(|m| m["id"] == ZEN_FREE_MODEL),
        "el modelo gratuito de prueba debe estar en el catálogo real"
    );
    for m in models {
        assert!(m["id"].as_str().unwrap().starts_with("opencode-zen/"));
        assert_eq!(m["nexo"]["credential_kind"], "api_key");
    }
}

#[tokio::test]
#[ignore = "necesita NEXO_TEST_ZEN_API_KEY"]
async fn zen_chat_with_a_free_model_works_end_to_end() {
    let Some(h) = start_with_zen().await else { return };

    let resp = h
        .post_chat(json!({
            "model": ZEN_FREE_MODEL,
            "messages": [{"role": "user", "content": "Responde solo: OK"}],
            "max_tokens": 20
        }))
        .await;
    assert!(resp.status().is_success(), "estado: {}", resp.status());

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(!body["choices"][0]["message"]["content"].is_null());
    assert!(body["usage"]["total_tokens"].as_u64().unwrap() > 0);
    // Verificado en T0: Zen informa de tokens de verdad, no es una estimación.
    assert_eq!(body["usage"]["nexo"]["usage_source"], "reported");

    let row = h
        .nexo
        .db()
        .recent_requests(0, 5)
        .unwrap()
        .into_iter()
        .find(|r| r.operation == "chat")
        .expect("la petición queda registrada");
    assert_eq!(row.status, "ok");
    assert_eq!(row.credential_kind, "api_key");

    // Nexo no añade contexto por su cuenta: un prompt corto tiene que contar
    // como corto. Un recuento inflado significaría que el gateway está
    // inyectando algo, y no sería del cliente. Comprobado contra el mismo
    // prompt enviado directo a Zen: 23 tokens de entrada.
    let input = body["usage"]["prompt_tokens"].as_u64().unwrap();
    assert!(
        input < 100,
        "un prompt de una línea no puede contar {input} tokens de entrada"
    );

    // El total es entrada + salida. Los tokens de razonamiento van dentro de
    // la salida, así que sumarlos otra vez inflaría la cifra.
    let output = body["usage"]["completion_tokens"].as_u64().unwrap();
    let total = body["usage"]["total_tokens"].as_u64().unwrap();
    assert_eq!(
        total,
        input + output,
        "el total no puede sumar dos veces el razonamiento ni la caché"
    );
    assert_eq!(row.total_tokens.unwrap() as u64, total, "se registra lo mismo que se devuelve");
}

#[tokio::test]
#[ignore = "necesita NEXO_TEST_ZEN_API_KEY"]
async fn zen_streaming_with_a_free_model_reassembles_correctly() {
    let Some(h) = start_with_zen().await else { return };

    let raw = h
        .post_chat(json!({
            "model": ZEN_FREE_MODEL,
            "messages": [{"role": "user", "content": "Cuenta: 1 2 3"}],
            "stream": true,
            "max_tokens": 30
        }))
        .await
        .text()
        .await
        .unwrap();

    assert!(raw.trim_end().ends_with("data: [DONE]"));
    let chunks = parse_sse(&raw);

    let ids: std::collections::HashSet<&str> =
        chunks.iter().filter_map(|c| c["id"].as_str()).collect();
    assert_eq!(ids.len(), 1, "un solo id para toda la respuesta");

    let text: String = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert!(!text.is_empty());

    let finishes = chunks
        .iter()
        .filter(|c| !c["choices"][0]["finish_reason"].is_null())
        .count();
    assert_eq!(finishes, 1);

    // Zen manda el uso en un fragmento posterior al de `finish_reason`. Contra
    // el proveedor real: una sola fila en estadísticas, y con sus tokens.
    let rows: Vec<_> = h
        .nexo
        .db()
        .recent_requests(0, 20)
        .unwrap()
        .into_iter()
        .filter(|r| r.operation == "chat")
        .collect();
    assert_eq!(rows.len(), 1, "una petición, una fila");
    assert!(
        rows[0].total_tokens.unwrap_or(0) > 0,
        "los tokens llegan después del cierre y deben quedar registrados"
    );
    assert_eq!(rows[0].usage_source, "reported");

    // Zen habla `chat/completions`: `Started` y el primer texto llegan en el
    // mismo fragmento. El tiempo al primer token debe salir del arranque de la
    // petición, no de ahí, o se registra un 0 ms que no es verdad.
    let ttft = rows[0].ttft_ms.expect("tiempo al primer token");
    assert!(ttft > 0, "un proveedor remoto no contesta en 0 ms");
    assert!(ttft <= rows[0].latency_ms.unwrap());
}

#[tokio::test]
#[ignore = "necesita NEXO_TEST_ZEN_API_KEY"]
async fn zen_insufficient_balance_is_a_clear_credits_error_not_invalid_key() {
    // Criterio 8: el caso real que el usuario vio en Msty. Un modelo de pago sin
    // saldo debe llegar como error de saldo, no como clave inválida — y no debe
    // pedir reconectar la cuenta, porque reconectar no soluciona esto.
    let Some(h) = start_with_zen().await else { return };

    let resp = h
        .post_chat(json!({
            "model": "opencode-zen/claude-haiku-4-5",
            "messages": [{"role": "user", "content": "hola"}]
        }))
        .await;

    if resp.status().is_success() {
        eprintln!("la cuenta de prueba tiene saldo hoy; el caso de error no se ejerce");
        return;
    }

    let body: Value = resp.json().await.unwrap();
    let message = body["error"]["message"].as_str().unwrap_or("");
    assert!(
        message.contains("saldo"),
        "debe explicar que es un problema de saldo, no de clave: {message}"
    );
    assert_eq!(
        body["error"]["nexo"]["reauth_required"], false,
        "reconectar la cuenta no arregla un problema de saldo"
    );
}

// ---------------------------------------------------------------------------
// OpenRouter real (spec 0006). Marcadas `#[ignore]` porque exigen una clave:
//   NEXO_TEST_OPENROUTER_API_KEY=sk-or-v1-... cargo test -p nexo-core --test gateway_e2e -- --ignored openrouter
// ---------------------------------------------------------------------------

const OPENROUTER_FREE_MODEL: &str = "openrouter/poolside/laguna-s-2.1:free";

/// Monta un Nexo con OpenRouter añadido con el atajo de la spec 0006 (mismo
/// adaptador genérico que Zen) y una aplicación con acceso. `None` si no hay
/// clave en el entorno, para que la prueba se salte de forma explícita.
async fn start_with_openrouter() -> Option<Harness> {
    let Ok(key) = std::env::var("NEXO_TEST_OPENROUTER_API_KEY") else {
        eprintln!("NEXO_TEST_OPENROUTER_API_KEY no está definida; prueba omitida");
        return None;
    };

    let db = Db::open_in_memory().expect("db");
    let nexo = Nexo::new(db, Arc::new(MemorySecretStore::default())).expect("nexo");

    // `Nexo::new()` deja `models_dev` vacío a propósito (lo rellena
    // `refresh_models_dev`, que en la app real se dispara en segundo plano al
    // arrancar). Sin este paso, `add_custom_provider` descubre el catálogo
    // contra un `models_dev` vacío y ningún modelo queda enriquecido —
    // encontrado al escribir esta prueba, ver spec 0006.
    nexo.refresh_models_dev().await;

    nexo.add_custom_provider("OpenRouter", "https://openrouter.ai/api/v1", &key)
        .await
        .expect("añadir el proveedor no debe fallar con una clave válida");

    let issued = nexo.db().create_app("prueba-openrouter", None).expect("app");
    nexo.db()
        .set_grant(
            &issued.app.id,
            &nexo_core::apps::Grant {
                provider_id: "openrouter".into(),
                credential_kind: "api_key".into(),
                model_pattern: "*".into(),
                allow_tools: true,
                allow_multimodal: false,
                log_content: false,
            },
        )
        .expect("grant");

    let listener = gateway::bind("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind");
    let port = listener.local_addr().unwrap().port();
    let serving = nexo.clone();
    tokio::spawn(async move {
        let _ = gateway::serve_on(serving, listener).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    Some(Harness {
        base: format!("http://127.0.0.1:{port}"),
        token: issued.token,
        nexo,
        http: reqwest::Client::new(),
    })
}

#[tokio::test]
#[ignore = "necesita NEXO_TEST_OPENROUTER_API_KEY"]
async fn openrouter_discovers_its_real_catalog_and_enriches_it_with_models_dev() {
    // Criterio 5 de la spec 0006: el catálogo no solo debe listar los modelos,
    // tiene que traer precio y capacidades reales de models.dev, no solo texto.
    let Some(h) = start_with_openrouter().await else { return };

    let body: Value = h
        .http
        .get(format!("{}/v1/models", h.base))
        .bearer_auth(&h.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let models = body["data"].as_array().unwrap();
    // OpenRouter tenía 337 modelos el 2026-08-02; se admite margen por si el
    // catálogo cambia entre esa fecha y la ejecución de la prueba.
    assert!(models.len() > 100, "esperaba cientos de modelos, llegaron {}", models.len());

    let free = models
        .iter()
        .find(|m| m["id"] == OPENROUTER_FREE_MODEL)
        .expect("el modelo gratuito de prueba debe estar en el catálogo real");
    assert_eq!(free["nexo"]["credential_kind"], "api_key");
    assert_eq!(
        free["nexo"]["priced"], true,
        "un modelo real de models.dev siempre trae precio, aunque sea cero por ser gratuito"
    );
    assert!(
        !free["nexo"]["context_max"].is_null(),
        "el límite de contexto debe venir de models.dev, no quedar sin dato"
    );
    for m in models {
        assert!(m["id"].as_str().unwrap().starts_with("openrouter/"));
    }
}

#[tokio::test]
#[ignore = "necesita NEXO_TEST_OPENROUTER_API_KEY"]
async fn openrouter_chat_with_a_free_model_works_end_to_end() {
    let Some(h) = start_with_openrouter().await else { return };

    let resp = h
        .post_chat(json!({
            "model": OPENROUTER_FREE_MODEL,
            "messages": [{"role": "user", "content": "Responde solo: OK"}],
            "max_tokens": 20
        }))
        .await;
    assert!(resp.status().is_success(), "estado: {}", resp.status());

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert!(!body["choices"][0]["message"]["content"].is_null());

    let row = h
        .nexo
        .db()
        .recent_requests(0, 5)
        .unwrap()
        .into_iter()
        .find(|r| r.operation == "chat")
        .expect("la petición queda registrada");
    assert_eq!(row.status, "ok");
    assert_eq!(row.credential_kind, "api_key");
    assert_eq!(row.provider_id, "openrouter");
}
