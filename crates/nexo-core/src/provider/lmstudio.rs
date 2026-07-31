//! Adaptador de LM Studio: modelos que corren en la máquina del usuario.
//!
//! Descubre el catálogo por el endpoint **nativo** `/api/v0/models`, que publica
//! tipo, cuantización, contexto y estado de carga, y sirve el chat por la
//! superficie **compatible con OpenAI**. Ver `specs/0001-proveedor-local-lm-studio/`.
//!
//! VERIFICADO CONTRA LM STUDIO 0.4.20 el 2026-07-31: informa de `usage`, respeta
//! `stream_options.include_usage` y emite `[DONE]`. La primera petición a un modelo
//! no cargado tardó ~14 s porque lo carga en ese momento; con el modelo cargado,
//! 0,34 s. Por eso no se impone un tiempo máximo a estas peticiones.

use crate::provider::{
    Accounting, AdapterError, AdapterId, Capabilities, ChatRequest, CredentialKind, EventStream,
    Health, Limits, ModelDescriptor, ProviderAdapter, ResolvedCredential,
};
use crate::translate::chat_completions;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

pub const PROVIDER: &str = "lmstudio";
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:1234";

pub struct LmStudioAdapter {
    http: reqwest::Client,
    base_url: String,
}

impl LmStudioAdapter {
    pub fn new(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        let mut base_url = base_url.into().trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            base_url = DEFAULT_BASE_URL.to_string();
        }
        // El usuario puede pegar la URL con `/v1` porque es lo que le pide cualquier
        // otro cliente. Se normaliza en lugar de fallar por un detalle de formato.
        if let Some(stripped) = base_url.strip_suffix("/v1") {
            base_url = stripped.to_string();
        }
        Self { http, base_url }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Normaliza una dirección suelta con las mismas reglas que el constructor.
    fn normalize(raw: &str) -> String {
        let trimmed = raw.trim().trim_end_matches('/');
        let stripped = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
        if stripped.is_empty() {
            DEFAULT_BASE_URL.to_string()
        } else {
            stripped.to_string()
        }
    }

    /// Dirección efectiva para esta llamada.
    ///
    /// La cuenta es la fuente de verdad sobre dónde está el servidor: la guarda en
    /// `external_id`. Tomarla de ahí en cada petición, y no de la configuración
    /// leída al construir el adaptador, es lo que permite cambiar la dirección sin
    /// reiniciar Nexo.
    fn base_for(&self, cred: &ResolvedCredential) -> String {
        match cred.external_id.as_deref() {
            Some(url) if !url.trim().is_empty() => Self::normalize(url),
            _ => self.base_url.clone(),
        }
    }

    fn native_models_url(&self) -> String {
        format!("{}/api/v0/models", self.base_url)
    }

    /// Error de red hacia un servidor local: el mensaje tiene que decir la
    /// dirección y qué hacer. Un `502` genérico no ayuda a nadie.
    fn unreachable_at(base: &str, detail: impl std::fmt::Display) -> AdapterError {
        AdapterError::Transport {
            detail: format!(
                "LM Studio no responde en {base}. Comprueba que está abierto y que su \
                 servidor local está activo, o cambia la dirección en Nexo. ({detail})"
            ),
        }
    }

}

#[async_trait]
impl ProviderAdapter for LmStudioAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::new(PROVIDER, CredentialKind::Local)
    }

    async fn catalog(
        &self,
        cred: &ResolvedCredential,
    ) -> Result<Vec<ModelDescriptor>, AdapterError> {
        let base = self.base_for(cred);
        // Primero el endpoint nativo: es el único que publica capacidades.
        match self.http.get(format!("{base}/api/v0/models")).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body: Value = resp.json().await.map_err(AdapterError::from_reqwest)?;
                let models = parse_native_models(&body);
                if !models.is_empty() {
                    return Ok(models);
                }
                tracing::warn!(
                    "el catálogo nativo de LM Studio llegó vacío o con forma inesperada; \
                     se prueba la superficie compatible"
                );
            }
            Ok(resp) => {
                tracing::warn!(
                    status = resp.status().as_u16(),
                    "el endpoint nativo de LM Studio no está disponible; se prueba el compatible"
                );
            }
            Err(e) => return Err(Self::unreachable_at(&base, e)),
        }

        // Respaldo: `/v1/models` solo da identificadores. Se asume texto y nada
        // más, porque prometer capacidades sin dato es lo que la invariante nº2
        // prohíbe.
        let resp = self
            .http
            .get(format!("{base}/v1/models"))
            .send()
            .await
            .map_err(|e| Self::unreachable_at(&base, e))?;
        if !resp.status().is_success() {
            return Err(AdapterError::Upstream {
                status: resp.status().as_u16(),
                provider_code: None,
                message: format!("LM Studio no devolvió catálogo en {base}"),
            });
        }
        let body: Value = resp.json().await.map_err(AdapterError::from_reqwest)?;
        Ok(parse_compat_models(&body))
    }

    async fn stream(
        &self,
        req: &ChatRequest,
        cred: &ResolvedCredential,
    ) -> Result<EventStream, AdapterError> {
        // Las capacidades ya las comprobó el servicio contra el catálogo real.
        let body = chat_completions::build_request(req);
        let base = self.base_for(cred);

        let resp = self
            .http
            .post(format!("{base}/v1/chat/completions"))
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|e| Self::unreachable_at(&base, e))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(chat_completions::classify_http_error(
                status.as_u16(),
                None,
                &text,
            ));
        }

        Ok(chat_completions::stream_from_response(resp))
    }

    async fn health(&self, cred: &ResolvedCredential) -> Health {
        // Listar modelos en un servidor local no cuesta nada.
        let base = self.base_for(cred);
        match self.http.get(format!("{base}/api/v0/models")).send().await {
            Ok(r) if r.status().is_success() => Health::Ok,
            Ok(_) => Health::Degraded,
            Err(_) => Health::Down,
        }
    }
}

/// Estado de LM Studio para la interfaz.
#[derive(Debug, Clone, Serialize)]
pub struct LmStudioStatus {
    pub base_url: String,
    pub reachable: bool,
    pub models: usize,
    pub loaded: usize,
    pub detail: Option<String>,
}

/// Comprueba que en esa dirección hay LM Studio, no solo algo que responde.
///
/// Se exige la forma de su endpoint nativo. El puerto 1234 lo usa más de un
/// programa, y dar por bueno cualquier `200` acabaría ofreciendo el catálogo de
/// otro producto como si fuera de LM Studio.
pub async fn probe(http: &reqwest::Client, base_url: &str) -> LmStudioStatus {
    let adapter = LmStudioAdapter::new(http.clone(), base_url);
    let url = adapter.native_models_url();
    let normalized = adapter.base_url().to_string();

    let resp = match http.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            return LmStudioStatus {
                base_url: normalized,
                reachable: false,
                models: 0,
                loaded: 0,
                detail: Some(format!("no responde: {e}")),
            }
        }
    };

    if !resp.status().is_success() {
        return LmStudioStatus {
            base_url: normalized,
            reachable: false,
            models: 0,
            loaded: 0,
            detail: Some(format!(
                "responde con HTTP {} en {url}, que no es lo que devuelve LM Studio",
                resp.status().as_u16()
            )),
        };
    }

    let body: Value = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            return LmStudioStatus {
                base_url: normalized,
                reachable: false,
                models: 0,
                loaded: 0,
                detail: Some(format!("respondió algo que no es json: {e}")),
            }
        }
    };

    match body.get("data").and_then(|d| d.as_array()) {
        None => LmStudioStatus {
            base_url: normalized,
            reachable: false,
            models: 0,
            loaded: 0,
            detail: Some(
                "hay algo escuchando en esa dirección, pero no responde como LM Studio"
                    .into(),
            ),
        },
        Some(items) => LmStudioStatus {
            base_url: normalized,
            reachable: true,
            models: items.len(),
            loaded: items
                .iter()
                .filter(|m| m.get("state").and_then(|v| v.as_str()) == Some("loaded"))
                .count(),
            detail: None,
        },
    }
}

/// Traduce el catálogo nativo de LM Studio.
fn parse_native_models(body: &Value) -> Vec<ModelDescriptor> {
    let Some(items) = body.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(|v| v.as_str())?;
            let kind = m.get("type").and_then(|v| v.as_str()).unwrap_or("llm");
            let tool_use = m
                .get("capabilities")
                .and_then(|v| v.as_array())
                .map(|caps| caps.iter().any(|c| c.as_str() == Some("tool_use")))
                .unwrap_or(false);

            let is_embeddings = kind == "embeddings";
            let caps = Capabilities {
                // Un modelo de embeddings NO hace texto. Declararlo así hace que
                // `check_capabilities` rechace el chat con 422 sin escribir ninguna
                // comprobación nueva.
                text: !is_embeddings,
                vision: kind == "vlm",
                audio: false,
                tools: tool_use && !is_embeddings,
                reasoning: false,
                json_mode: !is_embeddings,
                streaming: !is_embeddings,
                embeddings: is_embeddings,
            };

            let context = m
                .get("max_context_length")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);

            Some(ModelDescriptor {
                api_id: id.to_string(),
                public_name: format!("{PROVIDER}/{id}"),
                caps,
                limits: Limits { context_max: context, input_max: context, output_max: None },
                accounting: Accounting::Local,
                // Sin precio: ejecutar en la propia máquina no cuesta por token.
                pricing: None,
            })
        })
        .collect()
}

/// Respaldo cuando solo está la superficie compatible: solo hay identificadores.
fn parse_compat_models(body: &Value) -> Vec<ModelDescriptor> {
    let Some(items) = body.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()))
        .map(|id| ModelDescriptor {
            api_id: id.to_string(),
            public_name: format!("{PROVIDER}/{id}"),
            caps: Capabilities {
                text: true,
                json_mode: true,
                streaming: true,
                ..Default::default()
            },
            limits: Limits::default(),
            accounting: Accounting::Local,
            pricing: None,
        })
        .collect()
}

/// Metadatos que solo sirven para mostrarlos: cuantización, arquitectura y si el
/// modelo está cargado. No caben en `ModelDescriptor`, que es el contrato común.
#[derive(Debug, Clone, Serialize)]
pub struct LocalModelDetail {
    pub api_id: String,
    pub kind: String,
    pub quantization: Option<String>,
    pub arch: Option<String>,
    pub runtime: Option<String>,
    pub loaded: bool,
}

pub fn parse_details(body: &Value) -> Vec<LocalModelDetail> {
    let Some(items) = body.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|m| {
            Some(LocalModelDetail {
                api_id: m.get("id")?.as_str()?.to_string(),
                kind: m
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("llm")
                    .to_string(),
                quantization: m
                    .get("quantization")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                arch: m.get("arch").and_then(|v| v.as_str()).map(str::to_string),
                runtime: m
                    .get("compatibility_type")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                loaded: m.get("state").and_then(|v| v.as_str()) == Some("loaded"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{
        check_capabilities, ContentPart, Message, Role, ToolChoice, ToolDef,
    };

    /// Respuesta real de LM Studio 0.4.20, capturada el 2026-07-31.
    fn real_native_sample() -> Value {
        serde_json::json!({
            "object": "list",
            "data": [
                {
                    "id": "qwen/qwen3.6-35b-a3b",
                    "object": "model",
                    "type": "vlm",
                    "publisher": "qwen",
                    "arch": "qwen3_5_moe",
                    "compatibility_type": "mlx",
                    "quantization": "4bit",
                    "state": "not-loaded",
                    "max_context_length": 262144,
                    "capabilities": ["tool_use"]
                },
                {
                    "id": "gemma-4-12b-it-mlx",
                    "object": "model",
                    "type": "vlm",
                    "publisher": "lmstudio-community",
                    "arch": "gemma4_unified",
                    "compatibility_type": "mlx",
                    "quantization": "8bit",
                    "state": "loaded",
                    "max_context_length": 262144
                },
                {
                    "id": "text-embedding-nomic-embed-text-v1.5",
                    "object": "model",
                    "type": "embeddings",
                    "publisher": "nomic-ai",
                    "arch": "nomic-bert",
                    "compatibility_type": "gguf",
                    "quantization": "Q4_K_M",
                    "state": "not-loaded",
                    "max_context_length": 2048
                }
            ]
        })
    }

    fn model(id: &str) -> ModelDescriptor {
        parse_native_models(&real_native_sample())
            .into_iter()
            .find(|m| m.api_id == id)
            .expect("modelo en la muestra")
    }

    fn chat_request(api_model: &str) -> ChatRequest {
        ChatRequest {
            api_model: api_model.into(),
            public_model: format!("{PROVIDER}/{api_model}"),
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

    // -- Criterio 1: catálogo descubierto con sus metadatos ------------------

    #[test]
    fn discovers_every_model_with_provider_prefix_and_context() {
        let models = parse_native_models(&real_native_sample());
        assert_eq!(models.len(), 3);
        for m in &models {
            assert!(m.public_name.starts_with("lmstudio/"));
            assert_eq!(m.accounting, Accounting::Local);
            assert!(m.pricing.is_none(), "lo local no cuesta por token");
        }
        assert_eq!(model("qwen/qwen3.6-35b-a3b").limits.context_max, Some(262_144));
        assert_eq!(
            model("text-embedding-nomic-embed-text-v1.5").limits.context_max,
            Some(2048)
        );
    }

    // -- Criterio 2: el tipo decide la visión -------------------------------

    #[test]
    fn vlm_declares_vision_and_embeddings_model_does_not() {
        assert!(model("qwen/qwen3.6-35b-a3b").caps.vision);
        assert!(!model("text-embedding-nomic-embed-text-v1.5").caps.vision);
    }

    #[test]
    fn tools_only_when_the_server_says_tool_use() {
        assert!(
            model("qwen/qwen3.6-35b-a3b").caps.tools,
            "declara capabilities: [tool_use]"
        );
        assert!(
            !model("gemma-4-12b-it-mlx").caps.tools,
            "sin el campo capabilities no se prometen herramientas"
        );
    }

    // -- Criterio 3: un modelo de embeddings rechaza chat con 422 ------------

    #[test]
    fn asking_chat_of_an_embeddings_model_is_refused_with_422() {
        let embeddings = model("text-embedding-nomic-embed-text-v1.5");
        assert!(embeddings.caps.embeddings);
        assert!(!embeddings.caps.text, "un modelo de embeddings no hace texto");

        let err = check_capabilities(&chat_request("text-embedding-nomic-embed-text-v1.5"), &embeddings)
            .unwrap_err();
        assert_eq!(err.http_status(), 422);
        assert_eq!(err.kind_str(), "unsupported");
        assert!(
            format!("{err}").contains("text"),
            "el error debe nombrar la capacidad que falta: {err}"
        );
    }

    #[test]
    fn a_chat_model_passes_the_capability_check() {
        assert!(check_capabilities(
            &chat_request("gemma-4-12b-it-mlx"),
            &model("gemma-4-12b-it-mlx")
        )
        .is_ok());
    }

    #[test]
    fn tools_against_a_model_without_tool_use_are_refused() {
        let mut req = chat_request("gemma-4-12b-it-mlx");
        req.tools = vec![ToolDef {
            name: "t".into(),
            description: None,
            parameters: serde_json::json!({}),
        }];
        assert_eq!(
            check_capabilities(&req, &model("gemma-4-12b-it-mlx"))
                .unwrap_err()
                .http_status(),
            422
        );
    }

    // -- Respaldo y robustez ------------------------------------------------

    #[test]
    fn compat_fallback_claims_only_text() {
        let body = serde_json::json!({"data": [{"id": "algun-modelo"}]});
        let m = &parse_compat_models(&body)[0];
        assert_eq!(m.public_name, "lmstudio/algun-modelo");
        assert!(m.caps.text);
        assert!(!m.caps.vision, "sin dato no se promete visión");
        assert!(!m.caps.tools);
        assert_eq!(m.limits.context_max, None);
    }

    #[test]
    fn a_body_with_the_wrong_shape_yields_nothing() {
        assert!(parse_native_models(&serde_json::json!({})).is_empty());
        assert!(parse_native_models(&serde_json::json!({"data": "no soy un array"})).is_empty());
        assert!(parse_compat_models(&serde_json::json!({"models": []})).is_empty());
    }

    #[test]
    fn details_expose_quantization_and_load_state() {
        let details = parse_details(&real_native_sample());
        let gemma = details.iter().find(|d| d.api_id == "gemma-4-12b-it-mlx").unwrap();
        assert!(gemma.loaded);
        assert_eq!(gemma.quantization.as_deref(), Some("8bit"));
        assert_eq!(gemma.runtime.as_deref(), Some("mlx"));
        assert_eq!(gemma.arch.as_deref(), Some("gemma4_unified"));

        let qwen = details.iter().find(|d| d.api_id == "qwen/qwen3.6-35b-a3b").unwrap();
        assert!(!qwen.loaded, "estaba not-loaded en la muestra");
    }

    // -- Normalización de la dirección --------------------------------------

    #[test]
    fn base_url_is_normalised() {
        let http = reqwest::Client::new();
        for input in [
            "http://127.0.0.1:1234",
            "http://127.0.0.1:1234/",
            "http://127.0.0.1:1234/v1",
            "  http://127.0.0.1:1234/v1  ",
        ] {
            let a = LmStudioAdapter::new(http.clone(), input);
            assert_eq!(a.base_url(), "http://127.0.0.1:1234", "entrada: {input:?}");
        }
        assert_eq!(
            LmStudioAdapter::new(http, "").base_url(),
            DEFAULT_BASE_URL,
            "vacío cae al valor por defecto"
        );
    }

    #[test]
    fn urls_are_built_from_the_normalised_base() {
        let a = LmStudioAdapter::new(reqwest::Client::new(), "http://localhost:4321/v1/");
        assert_eq!(a.native_models_url(), "http://localhost:4321/api/v0/models");
        assert_eq!(a.base_url(), "http://localhost:4321");
    }

    // -- Criterio 7: mensaje útil cuando no responde ------------------------

    #[test]
    fn unreachable_error_names_the_address_and_what_to_do() {
        let err = LmStudioAdapter::unreachable_at("http://127.0.0.1:1234", "connection refused");
        assert_eq!(err.kind_str(), "transport");
        let text = err.to_string();
        assert!(text.contains("127.0.0.1:1234"), "debe nombrar la dirección: {text}");
        assert!(text.contains("abierto"), "debe decir qué hacer: {text}");
    }

    #[test]
    fn the_account_address_wins_over_the_configured_one() {
        // Es lo que permite cambiar la dirección sin reiniciar Nexo.
        let a = LmStudioAdapter::new(reqwest::Client::new(), "http://127.0.0.1:1234");
        let cred = |url: Option<&str>| ResolvedCredential {
            account_id: "acc".into(),
            kind: CredentialKind::Local,
            secret: String::new(),
            external_id: url.map(str::to_string),
        };
        assert_eq!(
            a.base_for(&cred(Some("http://localhost:4321/v1"))),
            "http://localhost:4321"
        );
        assert_eq!(a.base_for(&cred(None)), "http://127.0.0.1:1234");
        assert_eq!(a.base_for(&cred(Some("   "))), "http://127.0.0.1:1234");
    }

    #[tokio::test]
    async fn probe_rejects_something_that_is_not_lm_studio() {
        // Un puerto cerrado no es LM Studio, y el motivo tiene que ser legible.
        let status = probe(&reqwest::Client::new(), "http://127.0.0.1:1").await;
        assert!(!status.reachable);
        assert!(status.detail.is_some());
        assert_eq!(status.models, 0);
    }
}
