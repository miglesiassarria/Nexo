//! Adaptador de OpenAI por API key contra `api.openai.com`.
//!
//! Es la vía estable y el respaldo de la ruta de suscripción. Habla
//! `chat/completions`, así que toda su traducción vive en el módulo compartido
//! `translate::chat_completions`.

use crate::catalog;
use crate::provider::{
    AdapterError, AdapterId, CredentialKind, EventStream, Health, ModelDescriptor, ProviderAdapter,
    ResolvedCredential,
};
use crate::translate::chat_completions;
use async_trait::async_trait;
use serde_json::Value;

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
        req: &crate::provider::ChatRequest,
        cred: &ResolvedCredential,
    ) -> Result<EventStream, AdapterError> {
        // Las capacidades ya las comprobó el servicio contra el catálogo real.
        let body = chat_completions::build_request(req);

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
            return Err(chat_completions::classify_http_error(
                status.as_u16(),
                retry_after,
                &text,
            ));
        }

        Ok(chat_completions::stream_from_response(resp))
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
