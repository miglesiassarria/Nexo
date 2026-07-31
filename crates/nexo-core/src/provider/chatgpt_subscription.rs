//! Adaptador de OpenAI por OAuth de suscripción de ChatGPT.
//!
//! Es la razón de ser de Nexo y también su punto más frágil. Los valores que
//! pueden romperse viven en `crate::auth::chatgpt`, no aquí.

use crate::auth::chatgpt;
use crate::catalog;
use crate::provider::{
    check_capabilities, AdapterError, AdapterId, ChatEvent, ChatRequest, CredentialKind,
    EventStream, Health, ModelDescriptor, ProviderAdapter, ResolvedCredential,
};
use crate::translate::responses::{self, Translated};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;

pub const PROVIDER: &str = "openai";

pub struct ChatgptSubscriptionAdapter {
    http: reqwest::Client,
    endpoint: String,
}

impl ChatgptSubscriptionAdapter {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http, endpoint: chatgpt::API_ENDPOINT.to_string() }
    }

    /// Permite apuntar a un servidor de pruebas sin tocar el módulo frágil.
    pub fn with_endpoint(http: reqwest::Client, endpoint: impl Into<String>) -> Self {
        Self { http, endpoint: endpoint.into() }
    }
}

#[async_trait]
impl ProviderAdapter for ChatgptSubscriptionAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::new(PROVIDER, CredentialKind::SubscriptionOauth)
    }

    async fn catalog(
        &self,
        _cred: &ResolvedCredential,
    ) -> Result<Vec<ModelDescriptor>, AdapterError> {
        // Esta vía no expone un endpoint de modelos utilizable: el catálogo
        // sale del manifiesto versionado que se distribuye con Nexo.
        Ok(catalog::chatgpt_subscription_models())
    }

    async fn stream(
        &self,
        req: &ChatRequest,
        cred: &ResolvedCredential,
    ) -> Result<EventStream, AdapterError> {
        let model = catalog::chatgpt_subscription_models()
            .into_iter()
            .find(|m| m.api_id == req.api_model)
            .ok_or_else(|| AdapterError::Unsupported {
                capability: "model".into(),
                hint: Some(format!(
                    "{} no está disponible por suscripción; usa la vía de API key",
                    req.api_model
                )),
            })?;

        check_capabilities(req, &model)?;

        if req.temperature.is_some() || req.top_p.is_some() || !req.stop.is_empty() {
            tracing::debug!(
                model = %req.api_model,
                "temperature, top_p y stop se ignoran en la vía de suscripción"
            );
        }

        let body = responses::build_request(req);

        let mut request = self
            .http
            .post(&self.endpoint)
            .header("authorization", format!("Bearer {}", cred.secret))
            .header("accept", "text/event-stream")
            .header("content-type", "application/json")
            .header("originator", chatgpt::ORIGINATOR)
            .json(&body);

        if let Some(account) = &cred.external_id {
            request = request.header("ChatGPT-Account-Id", account.as_str());
        }

        let resp = request.send().await.map_err(AdapterError::from_reqwest)?;
        let status = resp.status();

        if !status.is_success() {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(std::time::Duration::from_secs);
            let text = resp.text().await.unwrap_or_default();
            return Err(classify_http_error(status.as_u16(), retry_after, &text));
        }

        // Una respuesta correcta que no sea un stream significa que la forma
        // del flujo ha cambiado: es lo que hay que detectar pronto.
        //
        // Pero el backend de ChatGPT responde con SSE **sin cabecera
        // `content-type`**, así que ausencia de cabecera no es sospecha: solo
        // se rechaza cuando el tipo declarado es claramente otra cosa.
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if !is_plausible_stream(&content_type) {
            let text = resp.text().await.unwrap_or_default();
            return Err(AdapterError::SubscriptionPathBroken {
                provider: PROVIDER.into(),
                detail: format!(
                    "se esperaba un stream de eventos y llegó «{content_type}». \
                     Cuerpo: {}",
                    truncate(&text, 300)
                ),
            });
        }

        let stream = resp
            .bytes_stream()
            .eventsource()
            .map(|item| match item {
                Err(e) => Err(AdapterError::Transport { detail: e.to_string() }),
                Ok(event) => Ok(event),
            })
            .flat_map(|item| {
                let out: Vec<Result<ChatEvent, AdapterError>> = match item {
                    Err(e) => vec![Err(e)],
                    Ok(event) => {
                        if event.data.trim() == "[DONE]" {
                            vec![]
                        } else {
                            match serde_json::from_str::<serde_json::Value>(&event.data) {
                                Err(e) => vec![Err(AdapterError::Malformed {
                                    detail: format!(
                                        "evento «{}» con json inválido: {e}",
                                        event.event
                                    ),
                                })],
                                Ok(value) => {
                                    match responses::translate_event(&event.event, &value) {
                                        Translated::Events(evs) => evs.into_iter().map(Ok).collect(),
                                        Translated::Failure(e) => vec![Err(e)],
                                        Translated::Ignored => vec![],
                                    }
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
        // No hay endpoint gratuito de comprobación en esta vía. Comprobar la
        // salud gastaría cuota de la suscripción del usuario, así que solo se
        // informa de si la credencial existe.
        if cred.secret.is_empty() {
            Health::Down
        } else {
            Health::Unknown
        }
    }
}

/// ¿Puede este `content-type` corresponder a un stream de eventos?
///
/// Ausente cuenta como plausible: el backend de ChatGPT no lo envía, y
/// rechazar por eso descartaría respuestas perfectamente válidas. Solo se
/// descarta un tipo declarado que sea incompatible con SSE.
fn is_plausible_stream(content_type: &str) -> bool {
    let ct = content_type.trim().to_ascii_lowercase();
    if ct.is_empty() {
        return true;
    }
    if ct.contains("event-stream") {
        return true;
    }
    // `application/json`, `text/html`, … son señal de que ya no hay stream.
    !(ct.contains("json") || ct.contains("html") || ct.contains("xml") || ct.contains("plain"))
}

fn classify_http_error(
    status: u16,
    retry_after: Option<std::time::Duration>,
    body: &str,
) -> AdapterError {
    match status {
        401 | 403 => AdapterError::Auth {
            reason: format!(
                "la suscripción rechazó la credencial ({status}). \
                 Vuelve a conectar la cuenta desde Nexo. Detalle: {}",
                truncate(body, 200)
            ),
            reauth_required: true,
        },
        429 => AdapterError::RateLimited { retry_after },
        // Un 404 o un 400 en un endpoint que ayer funcionaba es la firma de
        // que el flujo no soportado ha cambiado, no un error del cliente.
        400 | 404 | 410 => AdapterError::SubscriptionPathBroken {
            provider: PROVIDER.into(),
            detail: format!("HTTP {status}: {}", truncate(body, 300)),
        },
        _ => AdapterError::Upstream {
            status,
            provider_code: None,
            message: truncate(body, 300),
        },
    }
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn auth_errors_request_reauth() {
        match classify_http_error(401, None, "no") {
            AdapterError::Auth { reauth_required, .. } => assert!(reauth_required),
            other => panic!("esperaba Auth, llegó {other:?}"),
        }
    }

    #[test]
    fn rate_limit_propagates_retry_after() {
        match classify_http_error(429, Some(Duration::from_secs(30)), "") {
            AdapterError::RateLimited { retry_after } => {
                assert_eq!(retry_after, Some(Duration::from_secs(30)));
            }
            other => panic!("esperaba RateLimited, llegó {other:?}"),
        }
    }

    #[test]
    fn missing_endpoint_is_reported_as_broken_path_not_client_error() {
        for status in [400u16, 404, 410] {
            let err = classify_http_error(status, None, "not found");
            assert_eq!(
                err.kind_str(),
                "subscription_path_broken",
                "HTTP {status} debería señalar ruta rota"
            );
        }
    }

    #[test]
    fn server_errors_stay_upstream() {
        assert_eq!(classify_http_error(503, None, "").kind_str(), "upstream");
    }

    #[test]
    fn missing_content_type_is_accepted() {
        // Caso real observado el 2026-07-31: el backend de ChatGPT devuelve SSE
        // sin cabecera `content-type`. Rechazarlo por eso rompía la vía entera.
        assert!(is_plausible_stream(""));
        assert!(is_plausible_stream("   "));
    }

    #[test]
    fn event_stream_is_accepted_with_or_without_charset() {
        assert!(is_plausible_stream("text/event-stream"));
        assert!(is_plausible_stream("text/event-stream; charset=utf-8"));
        assert!(is_plausible_stream("TEXT/EVENT-STREAM"));
    }

    #[test]
    fn json_or_html_responses_mean_the_route_changed() {
        assert!(!is_plausible_stream("application/json"));
        assert!(!is_plausible_stream("application/json; charset=utf-8"));
        assert!(!is_plausible_stream("text/html"));
        assert!(!is_plausible_stream("text/plain"));
    }

    #[test]
    fn truncate_is_utf8_safe() {
        let s = "áéíóúñ".repeat(10);
        let t = truncate(&s, 5);
        assert!(t.starts_with("áéíóú"));
        assert!(t.ends_with('…'));
    }
}
