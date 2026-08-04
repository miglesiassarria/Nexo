//! Adaptador de OpenAI por OAuth de suscripción de ChatGPT.
//!
//! Es la razón de ser de Nexo y también su punto más frágil. Los valores que
//! pueden romperse viven en `crate::auth::chatgpt`, no aquí.

use crate::auth::chatgpt;
use crate::provider::{
    Accounting, AdapterError, AdapterId, Capabilities, ChatEvent, ChatRequest, CredentialKind,
    EventStream, Health, Limits, ModelDescriptor, ProviderAdapter, ResolvedCredential,
};
use crate::translate::responses::{self, Translated};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::StreamExt;

pub const PROVIDER: &str = "openai";

pub struct ChatgptSubscriptionAdapter {
    http: reqwest::Client,
    endpoint: String,
    models_endpoint: String,
    client_version: String,
}

impl ChatgptSubscriptionAdapter {
    pub fn new(http: reqwest::Client) -> Self {
        Self::with_client_version(http, chatgpt::DEFAULT_CLIENT_VERSION)
    }

    pub fn with_client_version(http: reqwest::Client, client_version: impl Into<String>) -> Self {
        Self {
            http,
            endpoint: chatgpt::API_ENDPOINT.to_string(),
            models_endpoint: chatgpt::MODELS_ENDPOINT.to_string(),
            client_version: client_version.into(),
        }
    }

    /// Permite apuntar a un servidor de pruebas sin tocar el módulo frágil.
    pub fn with_endpoint(http: reqwest::Client, endpoint: impl Into<String>) -> Self {
        Self {
            http,
            endpoint: endpoint.into(),
            models_endpoint: chatgpt::MODELS_ENDPOINT.to_string(),
            client_version: chatgpt::DEFAULT_CLIENT_VERSION.to_string(),
        }
    }
}

#[async_trait]
impl ProviderAdapter for ChatgptSubscriptionAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::new(PROVIDER, CredentialKind::SubscriptionOauth)
    }

    /// Descubre los modelos reales de esta vía.
    ///
    /// A diferencia de lo que se supuso al diseñar el producto, esta vía **sí**
    /// tiene endpoint de catálogo, y devuelve metadatos mejores que cualquier
    /// manifiesto escrito a mano: contexto, modalidades y niveles de
    /// razonamiento por modelo. El manifiesto queda solo como respaldo para
    /// cuando el descubrimiento falle.
    async fn catalog(
        &self,
        cred: &ResolvedCredential,
    ) -> Result<Vec<ModelDescriptor>, AdapterError> {
        let mut request = self
            .http
            .get(&self.models_endpoint)
            .query(&[("client_version", self.client_version.as_str())])
            .header("authorization", format!("Bearer {}", cred.secret))
            .header("originator", chatgpt::ORIGINATOR);
        if let Some(account) = &cred.external_id {
            request = request.header("ChatGPT-Account-Id", account.as_str());
        }

        let resp = request.send().await.map_err(AdapterError::from_reqwest)?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(classify_http_error(status.as_u16(), None, &text));
        }

        let body: serde_json::Value = resp.json().await.map_err(AdapterError::from_reqwest)?;
        let models = parse_models(&body);

        if models.is_empty() {
            return Err(AdapterError::Malformed {
                detail: format!(
                    "el catálogo llegó vacío pidiendo client_version={}. \
                     Prueba a subirlo en Configuración: el proveedor filtra los \
                     modelos por versión de cliente.",
                    self.client_version
                ),
            });
        }
        Ok(models)
    }

    async fn stream(
        &self,
        req: &ChatRequest,
        cred: &ResolvedCredential,
    ) -> Result<EventStream, AdapterError> {
        // Las capacidades ya las comprobó el servicio contra el catálogo real
        // descubierto; repetirlo aquí con un manifiesto local rechazaría
        // modelos nuevos perfectamente válidos.

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

/// Traduce la respuesta del endpoint de catálogo a descriptores de modelo.
///
/// Se descartan los modelos marcados como ocultos (`visibility != "list"`) y los
/// que el proveedor no expone por API: son internos del cliente oficial y
/// ofrecerlos sería prometer algo que no funciona.
///
/// El campo `instructions_template` que viene en la respuesta contiene el prompt
/// de sistema del cliente oficial. **No se usa**: inyectarlo sería hacer pasar a
/// Nexo por ese cliente, y eso es lo que el ADR 0001 prohíbe.
fn parse_models(body: &serde_json::Value) -> Vec<ModelDescriptor> {
    let Some(items) = body.get("models").and_then(|m| m.as_array()) else {
        return Vec::new();
    };

    items
        .iter()
        .filter(|m| {
            let visible = m
                .get("visibility")
                .and_then(|v| v.as_str())
                .map(|v| v == "list")
                .unwrap_or(true);
            let in_api = m
                .get("supported_in_api")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            visible && in_api
        })
        .filter_map(|m| {
            let slug = m.get("slug").and_then(|v| v.as_str())?;
            let modalities: Vec<&str> = m
                .get("input_modalities")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            // Se conservan los NOMBRES, no solo cuántos son (invariante 6).
            // Antes esto era `.map(|a| a.len())`: el proveedor decía «low,
            // medium, high, xhigh» y Nexo se quedaba con «4», así que sabía si
            // un modelo razonaba pero no qué niveles aceptaba — y no podía
            // ofrecer una lista honesta al configurarlo (spec 0009).
            let reasoning_levels: Vec<String> = m
                .get("supported_reasoning_levels")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|lvl| {
                            // Forma real capturada: `{"effort": "high"}`. Se
                            // admite también la cadena a secas por si cambia.
                            lvl.get("effort")
                                .and_then(|v| v.as_str())
                                .or_else(|| lvl.as_str())
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(ModelDescriptor {
                api_id: slug.to_string(),
                public_name: format!("{PROVIDER}/{slug}"),
                caps: Capabilities {
                    text: modalities.is_empty() || modalities.contains(&"text"),
                    vision: modalities.contains(&"image"),
                    audio: modalities.contains(&"audio"),
                    tools: true,
                    reasoning: !reasoning_levels.is_empty(),
                    json_mode: true,
                    streaming: true,
                    embeddings: false,
                    reasoning_levels,
                },
                limits: Limits {
                    context_max: m
                        .get("context_window")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                    input_max: m
                        .get("context_window")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                    output_max: None,
                },
                accounting: Accounting::Subscription,
                // Sin precio: el coste marginal es cero y la cuota es desconocida.
                pricing: None,
            })
        })
        .collect()
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

    /// Recorte de la respuesta real del endpoint, capturada el 2026-07-31.
    fn real_catalog_sample() -> serde_json::Value {
        serde_json::json!({"models": [
            {
                "slug": "gpt-5.6-sol",
                "display_name": "GPT-5.6-Sol",
                "context_window": 272000,
                "max_context_window": 1000000,
                "input_modalities": ["text", "image"],
                "supported_reasoning_levels": [
                    {"effort": "low"}, {"effort": "medium"},
                    {"effort": "high"}, {"effort": "xhigh"}
                ],
                "minimal_client_version": "0.144.0",
                "visibility": "list",
                "supported_in_api": true,
                "instructions_template": "You are Codex, a coding agent…"
            },
            {
                "slug": "codex-auto-review",
                "display_name": "Codex Auto Review",
                "context_window": 272000,
                "input_modalities": ["text", "image"],
                "visibility": "hide",
                "supported_in_api": true
            },
            {
                "slug": "solo-cliente",
                "context_window": 1000,
                "visibility": "list",
                "supported_in_api": false
            }
        ]})
    }

    #[test]
    fn discovers_visible_api_models_only() {
        let models = parse_models(&real_catalog_sample());
        let ids: Vec<&str> = models.iter().map(|m| m.api_id.as_str()).collect();
        assert_eq!(ids, vec!["gpt-5.6-sol"]);
        assert!(
            !ids.contains(&"codex-auto-review"),
            "un modelo oculto es interno del cliente oficial, no se ofrece"
        );
        assert!(
            !ids.contains(&"solo-cliente"),
            "sin soporte en API, ofrecerlo sería prometer algo que no funciona"
        );
    }

    #[test]
    fn discovered_models_carry_provider_and_metadata() {
        let m = &parse_models(&real_catalog_sample())[0];
        assert_eq!(m.public_name, "openai/gpt-5.6-sol");
        assert_eq!(m.limits.context_max, Some(272_000));
        assert!(m.caps.text);
        assert!(m.caps.vision, "input_modalities incluye image");
        assert!(!m.caps.audio);
        assert!(m.caps.reasoning, "tiene niveles de razonamiento");
        assert!(m.caps.streaming);
    }

    /// Criterio 1 de la spec 0009: el catálogo conserva los NOMBRES de los
    /// niveles admitidos, no solo cuántos son. Antes de este cambio el
    /// proveedor mandaba `low/medium/high/xhigh` y Nexo guardaba un `4` que se
    /// tiraba acto seguido al reducirlo a `reasoning: bool`.
    #[test]
    fn the_real_reasoning_levels_are_kept_not_just_counted() {
        let m = &parse_models(&real_catalog_sample())[0];
        assert_eq!(
            m.caps.reasoning_levels,
            vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string()
            ],
            "los niveles llegan con su nombre y en el orden que los publica el proveedor"
        );
        assert!(m.caps.reasoning, "con niveles, la capacidad sigue siendo cierta");
    }

    #[test]
    fn a_model_without_reasoning_levels_declares_none_and_no_capability() {
        // `supported_reasoning_levels` ausente: ni capacidad ni lista inventada.
        let sample = serde_json::json!({"models": [{
            "slug": "sin-razonamiento",
            "context_window": 1000,
            "input_modalities": ["text"],
            "visibility": "list",
            "supported_in_api": true
        }]});
        let m = &parse_models(&sample)[0];
        assert!(!m.caps.reasoning);
        assert!(m.caps.reasoning_levels.is_empty());
    }

    #[test]
    fn discovered_models_are_subscription_and_unpriced() {
        for m in parse_models(&real_catalog_sample()) {
            assert_eq!(m.accounting, Accounting::Subscription);
            assert!(m.pricing.is_none());
        }
    }

    #[test]
    fn a_body_without_models_yields_nothing() {
        assert!(parse_models(&serde_json::json!({"models": []})).is_empty());
        assert!(parse_models(&serde_json::json!({})).is_empty());
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
