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

    let recent = h.nexo.db().recent_requests(10).unwrap();
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
    let recent = nexo.db().recent_requests(10).unwrap();
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

    let recent = h.nexo.db().recent_requests(10).unwrap();
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

    let recent = h.nexo.db().recent_requests(10).unwrap();
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
        .recent_requests(5)
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
        .recent_requests(5)
        .unwrap()
        .into_iter()
        .find(|r| r.operation == "chat")
        .expect("el fallo queda registrado");
    assert_eq!(row.status, "error");
    assert_eq!(row.error_kind.as_deref(), Some("transport"));
    assert_eq!(row.credential_kind, "local");
}
