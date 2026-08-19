//! Adaptador de Ollama: modelos que corren en la máquina del usuario.
//!
//! Descubre el catálogo por el endpoint **nativo** `/api/tags`, que publica las
//! capacidades reales de cada modelo, y sirve el chat por la superficie
//! **compatible con OpenAI**. Ver `specs/0013-proveedor-local-ollama/`.
//!
//! VERIFICADO CONTRA OLLAMA 0.32.14 el 2026-08-20, en la máquina del usuario:
//! informa de `usage` sin streaming y, con `stream_options.include_usage`, en un
//! último chunk; el SSE llega con `content-type: text/event-stream` y termina en
//! `[DONE]`; las llamadas a herramientas salen con la forma de OpenAI y
//! `finish_reason: tool_calls`; y **la cabecera `Authorization` se ignora por
//! completo** (responde igual con un Bearer inventado que sin cabecera), así que
//! no se manda ninguna.
//!
//! Lo frágil de aquí (invariante 7) es la forma de `/api/tags`: las capacidades
//! llegan como lista de etiquetas (`["completion","tools","thinking"]`), y hay
//! campos que a veces vienen a `null` o vacíos (`details.context_length`,
//! `details.family`). Si esa forma cambia, se cae al respaldo `/v1/models`, que
//! solo da identificadores.

use crate::provider::{
    Accounting, AdapterError, AdapterId, Capabilities, ChatRequest, CredentialKind, EventStream,
    Health, Limits, ModelDescriptor, ProviderAdapter, ResolvedCredential,
};
use crate::translate::chat_completions;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

pub const PROVIDER: &str = "ollama";
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";

pub struct OllamaAdapter {
    http: reqwest::Client,
    base_url: String,
}

impl OllamaAdapter {
    pub fn new(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            http,
            base_url: Self::normalize(&base_url.into()),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// El usuario puede pegar la dirección con `/v1` al final porque es lo que le
    /// pide cualquier otro cliente. Se normaliza en lugar de fallar por un
    /// detalle de formato.
    fn normalize(raw: &str) -> String {
        let trimmed = raw.trim().trim_end_matches('/');
        let stripped = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
        if stripped.is_empty() {
            DEFAULT_BASE_URL.to_string()
        } else {
            stripped.to_string()
        }
    }

    /// Dirección efectiva para esta llamada. La cuenta es la fuente de verdad
    /// sobre dónde está el servidor —la guarda en `external_id`—, y tomarla de
    /// ahí en cada petición es lo que permite cambiarla sin reiniciar Nexo.
    fn base_for(&self, cred: &ResolvedCredential) -> String {
        match cred.external_id.as_deref() {
            Some(url) if !url.trim().is_empty() => Self::normalize(url),
            _ => self.base_url.clone(),
        }
    }

    fn native_models_url(&self) -> String {
        format!("{}/api/tags", self.base_url)
    }

    /// Error de red hacia un servidor local: el mensaje tiene que decir la
    /// dirección y qué hacer.
    fn unreachable_at(base: &str, detail: impl std::fmt::Display) -> AdapterError {
        AdapterError::Transport {
            detail: format!(
                "Ollama no responde en {base}. Comprueba que está en marcha \
                 (`ollama serve`), o cambia la dirección en Nexo. ({detail})"
            ),
        }
    }
}

#[async_trait]
impl ProviderAdapter for OllamaAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::new(PROVIDER, CredentialKind::Local)
    }

    async fn catalog(
        &self,
        cred: &ResolvedCredential,
    ) -> Result<Vec<ModelDescriptor>, AdapterError> {
        let base = self.base_for(cred);
        // Primero el nativo: es el único que publica capacidades.
        match self.http.get(format!("{base}/api/tags")).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body: Value = resp.json().await.map_err(AdapterError::from_reqwest)?;
                let models = parse_native_models(&body);
                if !models.is_empty() {
                    return Ok(models);
                }
                tracing::warn!(
                    "el catálogo nativo de Ollama llegó vacío o con forma inesperada; \
                     se prueba la superficie compatible"
                );
            }
            Ok(resp) => {
                tracing::warn!(
                    status = resp.status().as_u16(),
                    "el endpoint nativo de Ollama no está disponible; se prueba el compatible"
                );
            }
            Err(e) => return Err(Self::unreachable_at(&base, e)),
        }

        // Respaldo: `/v1/models` solo da identificadores. Se asume texto y nada
        // más, porque prometer capacidades sin dato es lo que la invariante 2
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
                message: format!("Ollama no devolvió catálogo en {base}"),
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

        // Sin cabecera `Authorization`: Ollama la ignora, y mandar una clave
        // falsa para que la tire es teatro.
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
        match self.http.get(format!("{base}/api/tags")).send().await {
            Ok(r) if r.status().is_success() => Health::Ok,
            Ok(_) => Health::Degraded,
            Err(_) => Health::Down,
        }
    }
}

/// Traduce `/api/tags` a descriptores de modelo.
///
/// Las capacidades salen de la lista que Ollama publica, no de suposiciones
/// sobre el nombre: `completion` → texto, `tools` → herramientas, `vision` →
/// visión, `thinking` → razonamiento. Lo que no declara, no se promete.
fn parse_native_models(body: &Value) -> Vec<ModelDescriptor> {
    let Some(items) = body.get("models").and_then(|d| d.as_array()) else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|m| {
            let id = m
                .get("model")
                .or_else(|| m.get("name"))
                .and_then(|v| v.as_str())?;

            let tags: Vec<&str> = m
                .get("capabilities")
                .and_then(|v| v.as_array())
                .map(|caps| caps.iter().filter_map(|c| c.as_str()).collect())
                .unwrap_or_default();
            let has = |tag: &str| tags.contains(&tag);

            // Sin `completion` no es un modelo de chat. Hoy eso solo pasa con
            // los de embeddings, que Nexo todavía no sirve: declararlo sin
            // texto hace que `check_capabilities` rechace el chat con un 422
            // explicativo, sin escribir ninguna comprobación nueva.
            let text = has("completion");

            let caps = Capabilities {
                text,
                vision: has("vision"),
                audio: false,
                tools: has("tools"),
                reasoning: has("thinking"),
                // Ambas verificadas contra la superficie compatible real.
                json_mode: text,
                streaming: text,
                // Ollama los soporta, pero Nexo no tiene ruta de embeddings:
                // declararlos sería prometer algo que el gateway no sirve.
                embeddings: false,
                // Ollama dice que el modelo razona, no que acepte niveles.
                reasoning_levels: vec![],
            };

            // `context_length` viene a `null` en algunos modelos: se deja sin
            // límite conocido en lugar de inventarse uno.
            let context = m
                .get("details")
                .and_then(|d| d.get("context_length"))
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);

            Some(ModelDescriptor {
                api_id: id.to_string(),
                public_name: format!("{PROVIDER}/{id}"),
                caps,
                limits: Limits {
                    context_max: context,
                    input_max: context,
                    output_max: None,
                },
                accounting: Accounting::Local,
                // Corre en la máquina del usuario: no lleva precio. Ni cero.
                pricing: None,
            })
        })
        .collect()
}

/// Respaldo cuando el nativo no responde: `{"object":"list","data":[{"id":…}]}`.
/// Solo hay identificadores, así que solo se promete texto.
fn parse_compat_models(body: &Value) -> Vec<ModelDescriptor> {
    let Some(items) = body.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(|v| v.as_str())?;
            Some(ModelDescriptor {
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
        })
        .collect()
}

/// Estado de Ollama para la interfaz.
#[derive(Debug, Clone, Serialize)]
pub struct OllamaStatus {
    pub base_url: String,
    pub reachable: bool,
    pub models: usize,
    pub loaded: usize,
    pub detail: Option<String>,
}

/// Comprueba que en esa dirección hay Ollama, no solo algo que responde.
///
/// Se exige la forma de su endpoint nativo. Dar por bueno cualquier `200`
/// acabaría ofreciendo el catálogo de otro producto como si fuera de Ollama —el
/// mismo error que ya se evitó con LM Studio.
pub async fn probe(http: &reqwest::Client, base_url: &str) -> OllamaStatus {
    let adapter = OllamaAdapter::new(http.clone(), base_url);
    let url = adapter.native_models_url();
    let base = adapter.base_url().to_string();

    let resp = match http.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            return OllamaStatus {
                base_url: base,
                reachable: false,
                models: 0,
                loaded: 0,
                detail: Some(format!("no responde: {e}")),
            }
        }
    };

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        return OllamaStatus {
            base_url: base,
            reachable: false,
            models: 0,
            loaded: 0,
            detail: Some(format!("respondió {status} en /api/tags")),
        };
    }

    let body: Value = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            return OllamaStatus {
                base_url: base,
                reachable: false,
                models: 0,
                loaded: 0,
                detail: Some(format!("respuesta ilegible: {e}")),
            }
        }
    };

    // La forma manda: `models` tiene que ser un array. Algo que devuelve `200`
    // con otra cosa no es Ollama.
    let Some(items) = body.get("models").and_then(|v| v.as_array()) else {
        return OllamaStatus {
            base_url: base,
            reachable: false,
            models: 0,
            loaded: 0,
            detail: Some("responde, pero no con la forma de Ollama".into()),
        };
    };

    let loaded = loaded_count(http, &base).await;
    OllamaStatus {
        base_url: base,
        reachable: true,
        models: items.len(),
        loaded,
        detail: None,
    }
}

/// Cuántos modelos tiene Ollama cargados en memoria ahora mismo. Es solo
/// informativo: si `/api/ps` no contesta, cero y sin ruido.
async fn loaded_count(http: &reqwest::Client, base: &str) -> usize {
    let Ok(resp) = http.get(format!("{base}/api/ps")).send().await else {
        return 0;
    };
    if !resp.status().is_success() {
        return 0;
    }
    let Ok(body) = resp.json::<Value>().await else {
        return 0;
    };
    body.get("models")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0)
}

/// Metadatos que solo sirven para mostrarlos. No caben en `ModelDescriptor`,
/// que es el contrato común.
#[derive(Debug, Clone, Serialize)]
pub struct LocalModelDetail {
    pub api_id: String,
    pub parameters: Option<String>,
    pub quantization: Option<String>,
    pub family: Option<String>,
    pub size_bytes: Option<u64>,
}

pub fn parse_details(body: &Value) -> Vec<LocalModelDetail> {
    let Some(items) = body.get("models").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|m| {
            // Los campos vacíos de Ollama (`family: ""`, `parameter_size: ""`)
            // se tratan como ausentes: mostrar una cadena vacía en el panel es
            // peor que no mostrar nada.
            let non_empty = |v: Option<&Value>| {
                v.and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            let details = m.get("details");
            Some(LocalModelDetail {
                api_id: m
                    .get("model")
                    .or_else(|| m.get("name"))?
                    .as_str()?
                    .to_string(),
                parameters: non_empty(details.and_then(|d| d.get("parameter_size"))),
                quantization: non_empty(details.and_then(|d| d.get("quantization_level"))),
                family: non_empty(details.and_then(|d| d.get("family"))),
                size_bytes: m.get("size").and_then(|v| v.as_u64()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// La respuesta **real** de `/api/tags` de Ollama 0.32.14, capturada en la
    /// máquina del usuario el 2026-08-20. Se conserva tal cual, con sus rarezas:
    /// el segundo modelo trae `context_length: null`, `family: ""` y
    /// `parameter_size: ""`.
    fn real_api_tags() -> Value {
        json!({"models": [
            {
                "name": "qwen3:0.6b",
                "model": "qwen3:0.6b",
                "size": 522653767u64,
                "details": {
                    "format": "gguf",
                    "family": "qwen3",
                    "parameter_size": "751.63M",
                    "quantization_level": "Q4_K_M",
                    "context_length": 40960,
                    "embedding_length": 1024
                },
                "capabilities": ["completion", "tools", "thinking"]
            },
            {
                "name": "qwen3.8:27b-mlx",
                "model": "qwen3.8:27b-mlx",
                "size": 16000000000u64,
                "details": {
                    "format": "gguf",
                    "family": "",
                    "parameter_size": "",
                    "quantization_level": "nvfp4",
                    "context_length": null
                },
                "capabilities": ["completion", "vision", "tools", "thinking"]
            }
        ]})
    }

    #[test]
    fn ollama_adapter_is_a_local_provider() {
        let adapter = OllamaAdapter::new(reqwest::Client::new(), "");
        let id = adapter.id();
        assert_eq!(id.provider, PROVIDER);
        assert_eq!(
            id.kind,
            CredentialKind::Local,
            "Ollama corre en la máquina del usuario: no es una vía de API key"
        );
        assert_eq!(adapter.base_url(), DEFAULT_BASE_URL);
    }

    #[test]
    fn the_base_url_is_normalized_like_lm_studio() {
        let with_v1 = OllamaAdapter::new(reqwest::Client::new(), "http://127.0.0.1:11434/v1");
        assert_eq!(with_v1.base_url(), "http://127.0.0.1:11434");
        let trailing = OllamaAdapter::new(reqwest::Client::new(), "http://otro:1234/");
        assert_eq!(trailing.base_url(), "http://otro:1234");
        let empty = OllamaAdapter::new(reqwest::Client::new(), "   ");
        assert_eq!(empty.base_url(), DEFAULT_BASE_URL);
    }

    #[test]
    fn native_capabilities_come_from_what_ollama_declares() {
        let models = parse_native_models(&real_api_tags());
        assert_eq!(models.len(), 2);

        let small = &models[0];
        assert_eq!(small.api_id, "qwen3:0.6b");
        assert_eq!(small.public_name, "ollama/qwen3:0.6b");
        assert!(small.caps.text, "declara `completion`");
        assert!(small.caps.tools, "declara `tools`");
        assert!(small.caps.reasoning, "`thinking` es razonamiento");
        assert!(
            !small.caps.vision,
            "no declara `vision`: no se promete visión que el modelo no tiene"
        );
        assert_eq!(small.limits.context_max, Some(40960));

        let big = &models[1];
        assert!(big.caps.vision, "este sí declara `vision`");
        assert_eq!(
            big.limits.context_max, None,
            "`context_length: null` se queda sin límite conocido, no en un número inventado"
        );
    }

    #[test]
    fn ollama_models_are_accounted_as_local() {
        for m in parse_native_models(&real_api_tags()) {
            assert!(
                matches!(m.accounting, Accounting::Local),
                "un modelo que corre en el portátil no es medido: {}",
                m.public_name
            );
            assert!(
                m.pricing.is_none(),
                "no lleva precio, ni cero: {}",
                m.public_name
            );
        }
    }

    #[test]
    fn embeddings_are_never_promised_because_nexo_has_no_route_for_them() {
        let body = json!({"models": [{
            "model": "nomic-embed-text",
            "details": {},
            "capabilities": ["embedding"]
        }]});
        let models = parse_native_models(&body);
        let m = &models[0];
        assert!(!m.caps.embeddings, "no se promete lo que el gateway no sirve");
        assert!(
            !m.caps.text,
            "sin `completion` no hace chat, y así `check_capabilities` lo rechaza con 422"
        );
    }

    #[test]
    fn a_native_catalog_with_another_shape_yields_nothing_instead_of_guessing() {
        assert!(parse_native_models(&json!({"models": null})).is_empty());
        assert!(parse_native_models(&json!({"data": [{"id": "x"}]})).is_empty());
        assert!(parse_native_models(&json!({"models": "no es un array"})).is_empty());
    }

    #[test]
    fn the_compat_fallback_only_promises_text() {
        let body = json!({"object": "list", "data": [
            {"id": "qwen3:0.6b", "object": "model"},
            {"id": "otro:8b", "object": "model"}
        ]});
        let models = parse_compat_models(&body);
        assert_eq!(models.len(), 2);
        assert!(models[0].caps.text);
        assert!(
            !models[0].caps.tools && !models[0].caps.vision && !models[0].caps.reasoning,
            "sin el nativo no hay dato de capacidades: no se prometen"
        );
        assert!(matches!(models[0].accounting, Accounting::Local));
        // `data: null` es lo que devuelve Ollama sin ningún modelo descargado.
        assert!(parse_compat_models(&json!({"object": "list", "data": null})).is_empty());
    }

    #[test]
    fn details_treat_ollamas_empty_strings_as_absent() {
        let details = parse_details(&real_api_tags());
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].parameters.as_deref(), Some("751.63M"));
        assert_eq!(details[0].quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(details[0].family.as_deref(), Some("qwen3"));
        assert_eq!(details[0].size_bytes, Some(522653767));
        assert_eq!(
            details[1].family, None,
            "`family: \"\"` es ausente, no una cadena vacía que ensucie el panel"
        );
        assert_eq!(details[1].parameters, None);
        assert_eq!(details[1].quantization.as_deref(), Some("nvfp4"));
    }

    #[tokio::test]
    async fn probe_rejects_something_that_is_not_ollama() {
        // Un servidor que responde 200 con otra cosa en el puerto de Ollama.
        let app = axum::Router::new().route(
            "/api/tags",
            axum::routing::get(|| async { axum::Json(json!({"cosas": []})) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let status = probe(&reqwest::Client::new(), &format!("http://127.0.0.1:{port}")).await;
        assert!(
            !status.reachable,
            "dar por bueno cualquier 200 ofrecería el catálogo de otro producto"
        );
        assert!(status.detail.unwrap().contains("forma"));
    }
}
