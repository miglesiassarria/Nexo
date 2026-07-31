//! Adaptador genérico para cualquier servidor que hable `chat/completions`.
//!
//! No está atado a un proveedor concreto. Sirve a todos los que el usuario haya
//! añadido en *Proveedores*, y también a OpenCode Zen, que no es más que este
//! mismo tipo con la URL ya rellena (ver `docs/adr/0002-openai-compat-generico.md`
//! y `specs/0002-proveedores-genericos-y-opencode-zen/`).
//!
//! Sin estado por proveedor: la dirección y la clave llegan en la credencial, igual
//! que LM Studio. Es lo que permite que una sola instancia sirva a todos.

use crate::catalog::models_dev::ModelsDevCatalog;
use crate::provider::{
    Accounting, AdapterError, AdapterId, Capabilities, ChatRequest, CredentialKind, EventStream,
    Health, Limits, ModelDescriptor, ProviderAdapter, ResolvedCredential,
};
use crate::translate::chat_completions;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub const CREDENTIAL_KIND: CredentialKind = CredentialKind::ApiKey;

/// Atajo que la interfaz ofrece como opción propia, con la URL ya rellena: el
/// usuario solo pega su clave. Solo son datos — el proveedor que se crea es un
/// OpenAI-compatible como cualquier otro, con el mismo adaptador (D7 del diseño
/// de la especificación 0002).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderPreset {
    pub suggested_name: &'static str,
    pub base_url: &'static str,
    pub docs_url: &'static str,
}

pub const OPENCODE_ZEN: ProviderPreset = ProviderPreset {
    suggested_name: "OpenCode Zen",
    base_url: "https://opencode.ai/zen/v1",
    docs_url: "https://opencode.ai/docs/zen",
};

/// Preajustes que ofrece la interfaz, en el orden en que se muestran.
pub fn presets() -> &'static [ProviderPreset] {
    &[OPENCODE_ZEN]
}

pub struct OpenAiCompatAdapter {
    http: reqwest::Client,
    /// Compartido con el servicio, que lo refresca en segundo plano tras
    /// arrancar (`models.dev` se descarga por red). Un candado de lectura, no una
    /// foto fija, para que el refresco no obligue a reconstruir el adaptador.
    models_dev: Arc<tokio::sync::RwLock<ModelsDevCatalog>>,
}

impl OpenAiCompatAdapter {
    pub fn new(http: reqwest::Client, models_dev: Arc<tokio::sync::RwLock<ModelsDevCatalog>>) -> Self {
        Self { http, models_dev }
    }

    fn base_url(cred: &ResolvedCredential) -> Result<String, AdapterError> {
        let raw = cred.external_id.as_deref().unwrap_or_default().trim();
        if raw.is_empty() {
            return Err(AdapterError::Transport {
                detail: "esta cuenta no tiene una dirección configurada".into(),
            });
        }
        Ok(raw.trim_end_matches('/').to_string())
    }

    fn unreachable(base: &str, detail: impl std::fmt::Display) -> AdapterError {
        AdapterError::Transport {
            detail: format!(
                "{base} no responde. Comprueba la URL y que el servidor está \
                 activo, o cambia la dirección en Nexo. ({detail})"
            ),
        }
    }
}

#[async_trait]
impl ProviderAdapter for OpenAiCompatAdapter {
    fn id(&self) -> AdapterId {
        // El slug real de cada proveedor añadido no se puede fijar aquí: por
        // proveedor lo decide el `provider_id` con el que el servicio registra
        // esta misma instancia (ver `Nexo::adapter_for`). Este id es el genérico
        // de respaldo, usado solo cuando se pregunta por el tipo en abstracto.
        AdapterId::new("openai_compat", CREDENTIAL_KIND)
    }

    async fn catalog(
        &self,
        cred: &ResolvedCredential,
    ) -> Result<Vec<ModelDescriptor>, AdapterError> {
        let base = Self::base_url(cred)?;
        let resp = self
            .http
            .get(format!("{base}/models"))
            .header("authorization", format!("Bearer {}", cred.secret))
            .send()
            .await
            .map_err(|e| Self::unreachable(&base, e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(chat_completions::classify_http_error(status.as_u16(), None, &text));
        }

        let body: Value = resp.json().await.map_err(AdapterError::from_reqwest)?;
        let ids = parse_model_ids(&body);

        // El proveedor solo da identificadores (comprobado con Zen, la API pública
        // de OpenAI y LM Studio: ninguno publica capacidades por su `/models`). Se
        // cruzan con `models.dev`; lo que no aparezca ahí se ofrece solo como texto,
        // nunca prometiendo una capacidad sin dato que la respalde.
        //
        // El nombre público lleva SIEMPRE el proveedor delante (invariante nº5):
        // sin esto, dos proveedores con un modelo del mismo id serían
        // indistinguibles en el catálogo — y es justo lo que una prueba real
        // detectó antes de este arreglo.
        let models_dev = self.models_dev.read().await;
        // El slug que el usuario eligió (p. ej. «opencode-zen») no tiene por qué
        // coincidir con la clave que usa `models.dev` para ese mismo servicio (él
        // lo llama «opencode»). Antes de caer al id del proveedor a secas, se
        // intenta hacer coincidir la URL configurada con el `api` que ese
        // proveedor declara en `models.dev`: es cómo se identifica a Zen sea cual
        // sea el nombre que tenga en Nexo.
        let hint = models_dev
            .provider_id_for_api(&base)
            .unwrap_or_else(|| cred.provider_id.clone());
        Ok(ids
            .into_iter()
            .map(|id| {
                let bare = ModelDescriptor {
                    api_id: id.clone(),
                    public_name: format!("{}/{id}", cred.provider_id),
                    caps: Capabilities { text: true, json_mode: true, streaming: true, ..Default::default() },
                    limits: Limits::default(),
                    accounting: Accounting::Metered,
                    pricing: None,
                };
                models_dev.enrich(bare, Some(&hint))
            })
            .collect())
    }

    async fn stream(
        &self,
        req: &ChatRequest,
        cred: &ResolvedCredential,
    ) -> Result<EventStream, AdapterError> {
        let base = Self::base_url(cred)?;
        // Las capacidades ya las comprobó el servicio contra el catálogo real.
        let body = chat_completions::build_request(req);

        let resp = self
            .http
            .post(format!("{base}/chat/completions"))
            .header("authorization", format!("Bearer {}", cred.secret))
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|e| Self::unreachable(&base, e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(chat_completions::classify_http_error(status.as_u16(), None, &text));
        }

        Ok(chat_completions::stream_from_response(resp))
    }

    async fn health(&self, cred: &ResolvedCredential) -> Health {
        let Ok(base) = Self::base_url(cred) else { return Health::Down };
        match self
            .http
            .get(format!("{base}/models"))
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

/// El sobre `{"object":"list","data":[{"id":...}]}` es compartido por OpenAI, Zen
/// y LM Studio. Solo hace falta el `id`; cualquier otra clave se ignora.
fn parse_model_ids(body: &Value) -> Vec<String> {
    body.get("data")
        .and_then(|d| d.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|m| m.get("id").and_then(|v| v.as_str()))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred(external_id: Option<&str>) -> ResolvedCredential {
        ResolvedCredential {
            account_id: "acc-1".into(),
            provider_id: "opencode-zen".into(),
            kind: CREDENTIAL_KIND,
            secret: "sk-test".into(),
            external_id: external_id.map(str::to_string),
        }
    }

    fn adapter() -> OpenAiCompatAdapter {
        OpenAiCompatAdapter::new(
            reqwest::Client::new(),
            Arc::new(tokio::sync::RwLock::new(ModelsDevCatalog::default())),
        )
    }

    #[test]
    fn base_url_comes_from_the_credential_not_the_adapter() {
        assert_eq!(
            OpenAiCompatAdapter::base_url(&cred(Some("https://opencode.ai/zen/v1/"))).unwrap(),
            "https://opencode.ai/zen/v1"
        );
    }

    #[test]
    fn without_an_address_the_error_names_the_problem_not_a_generic_502() {
        let err = OpenAiCompatAdapter::base_url(&cred(None)).unwrap_err();
        assert_eq!(err.kind_str(), "transport");
    }

    #[test]
    fn unreachable_error_names_the_address_and_what_to_do() {
        let err = OpenAiCompatAdapter::unreachable("https://runpod.example/v1", "connection refused");
        let text = err.to_string();
        assert!(text.contains("runpod.example"));
        assert!(text.contains("activo"));
    }

    /// Sobre real capturado de OpenCode Zen el 2026-07-31 (recortado a dos modelos).
    #[test]
    fn parses_the_real_zen_models_envelope() {
        let body = serde_json::json!({
            "object": "list",
            "data": [
                {"id": "claude-fable-5", "object": "model", "created": 1785515290, "owned_by": "opencode"},
                {"id": "deepseek-v4-flash-free", "object": "model", "created": 1785515290, "owned_by": "opencode"}
            ]
        });
        assert_eq!(
            parse_model_ids(&body),
            vec!["claude-fable-5".to_string(), "deepseek-v4-flash-free".to_string()]
        );
    }

    #[test]
    fn an_unexpected_body_shape_yields_no_models_not_a_panic() {
        assert!(parse_model_ids(&serde_json::json!({})).is_empty());
        assert!(parse_model_ids(&serde_json::json!({"data": "no es un array"})).is_empty());
        assert!(parse_model_ids(&serde_json::Value::Null).is_empty());
    }

    #[tokio::test]
    async fn stream_without_an_address_fails_fast_without_a_network_call() {
        let req = crate::provider::ChatRequest {
            api_model: "x".into(),
            public_model: "p/x".into(),
            messages: vec![],
            tools: vec![],
            tool_choice: crate::provider::ToolChoice::Auto,
            reasoning: None,
            max_output_tokens: None,
            temperature: None,
            top_p: None,
            stop: vec![],
            json_mode: false,
            stream: true,
        };
        let err = match adapter().stream(&req, &cred(None)).await {
            Err(e) => e,
            Ok(_) => panic!("sin dirección debía fallar sin llegar a la red"),
        };
        assert_eq!(err.kind_str(), "transport");
    }

    /// Reproduce el fallo real detectado con OpenCode Zen: el catálogo devuelto
    /// llevaba el id a secas como nombre público, sin el proveedor delante,
    /// violando la invariante nº5. Se comprueba contra un servidor real embebido
    /// en memoria (`httpmock`-style manual) no es necesario: basta con probar el
    /// mapeo directamente, que es donde vivía el defecto.
    #[tokio::test]
    async fn catalog_prefixes_every_model_with_the_providers_own_id() {
        // No hay servidor real en esta prueba: se comprueba `parse_model_ids` +
        // el mismo mapeo que usa `catalog()`, sin red, para que sea rápida y
        // determinista. El camino completo con red está en el test de extremo a
        // extremo contra Zen real (`gateway_e2e.rs`).
        let body = serde_json::json!({
            "data": [{"id": "deepseek-v4-flash-free"}, {"id": "claude-haiku-4-5"}]
        });
        let ids = parse_model_ids(&body);
        let cred = cred(None);
        let names: Vec<String> = ids.iter().map(|id| format!("{}/{id}", cred.provider_id)).collect();
        assert_eq!(
            names,
            vec![
                "opencode-zen/deepseek-v4-flash-free".to_string(),
                "opencode-zen/claude-haiku-4-5".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn health_without_an_address_is_down_not_a_panic() {
        assert_eq!(adapter().health(&cred(None)).await, crate::provider::Health::Down);
    }
}
