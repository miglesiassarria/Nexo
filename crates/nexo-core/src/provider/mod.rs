//! Contrato de proveedor. Ver `docs/contrato-proveedor.md`.
//!
//! Un adaptador no se identifica solo por el proveedor, sino por la pareja
//! proveedor + tipo de credencial: la misma cuenta ofrece catálogo,
//! capacidades, límites y contabilidad distintos según cómo se autenticó.

pub mod chatgpt_subscription;
pub mod lmstudio;
pub mod openai_compat;
pub mod mock;
pub mod openai_apikey;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    /// API pública y documentada del proveedor. Facturación por token.
    ApiKey,
    /// Flujo OAuth del cliente oficial del proveedor. No soportado por el
    /// proveedor: sin coste marginal, catálogo recortado, sin métricas de uso.
    /// Ver `docs/adr/0001-oauth-de-suscripcion.md`.
    SubscriptionOauth,
    /// Runtime en la máquina del usuario. Sin credencial ni coste.
    Local,
    /// Proveedor de pruebas. No sale de la máquina.
    Mock,
}

impl CredentialKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::SubscriptionOauth => "subscription_oauth",
            Self::Local => "local",
            Self::Mock => "mock",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "api_key" => Some(Self::ApiKey),
            "subscription_oauth" => Some(Self::SubscriptionOauth),
            "local" => Some(Self::Local),
            "mock" => Some(Self::Mock),
            _ => None,
        }
    }

    /// Las rutas de suscripción exigen límite por aplicación (ADR 0001,
    /// mitigación del riesgo de multiplexación).
    pub fn requires_app_limit(&self) -> bool {
        matches!(self, Self::SubscriptionOauth)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AdapterId {
    pub provider: String,
    pub kind: CredentialKind,
}

impl AdapterId {
    pub fn new(provider: impl Into<String>, kind: CredentialKind) -> Self {
        Self { provider: provider.into(), kind }
    }

    pub fn slug(&self) -> String {
        format!("{}:{}", self.provider, self.kind.as_str())
    }
}

/// Credencial resuelta en memoria para una llamada concreta.
///
/// Se construye bajo demanda a partir del almacén seguro del sistema y nunca
/// se persiste. Lo que vive en SQLite es la referencia y los metadatos.
#[derive(Clone)]
pub struct ResolvedCredential {
    pub account_id: String,
    /// A qué proveedor pertenece esta cuenta. Los adaptadores fijos (LM Studio,
    /// OpenAI…) ya conocen su proveedor por su propio tipo y no lo necesitan; el
    /// adaptador genérico sí, porque una sola instancia sirve a varios y con esto
    /// sabe con qué nombre prefijar sus modelos (invariante «el proveedor va
    /// siempre delante») y qué proveedor de `models.dev` consultar primero.
    pub provider_id: String,
    pub kind: CredentialKind,
    /// API key o access token, según el tipo.
    pub secret: String,
    /// Identificador de cuenta del proveedor, cuando lo haya.
    pub external_id: Option<String>,
}

impl std::fmt::Debug for ResolvedCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedCredential")
            .field("account_id", &self.account_id)
            .field("kind", &self.kind)
            .field("secret", &"<oculto>")
            .field("external_id", &self.external_id)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Petición: superconjunto, no mínimo común denominador
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub api_model: String,
    pub public_model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub tool_choice: ToolChoice,
    pub reasoning: Option<ReasoningEffort>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Vec<String>,
    pub json_mode: bool,
    pub stream: bool,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<ContentPart>,
    /// Presente en mensajes de rol `Tool`.
    pub tool_call_id: Option<String>,
    /// Llamadas a herramientas emitidas por el asistente en turnos previos.
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone)]
pub enum ContentPart {
    Text(String),
    /// URL o data URI de una imagen.
    ImageUrl(String),
    Audio { mime: String, base64: String },
    File { name: String, mime: String, base64: String },
}

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Named(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
    /// Presente en la vía de suscripción de ChatGPT.
    XHigh,
}

impl ReasoningEffort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }

    /// El inverso de `as_str`. Devuelve `None` para un nivel que Nexo no sabe
    /// enviar: el catálogo puede publicar niveles nuevos antes de que este enum
    /// los conozca, y ofrecerlos sin poder representarlos sería prometer algo
    /// que no se cumple (ver D5 del diseño de la especificación 0009).
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "minimal" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::XHigh),
            _ => None,
        }
    }
}

/// Capacidades que una petición puede exigir. Se usa para rechazar de forma
/// explícita en lugar de degradar en silencio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Text,
    Vision,
    Audio,
    Tools,
    Reasoning,
    JsonMode,
    Streaming,
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Vision => "vision",
            Self::Audio => "audio",
            Self::Tools => "tools",
            Self::Reasoning => "reasoning",
            Self::JsonMode => "json_mode",
            Self::Streaming => "streaming",
        }
    }
}

impl ChatRequest {
    /// Capacidades que esta petición exige del destino.
    pub fn required_capabilities(&self) -> Vec<Capability> {
        let mut out = vec![Capability::Text];
        if self.stream {
            out.push(Capability::Streaming);
        }
        if !self.tools.is_empty() || self.tool_choice != ToolChoice::Auto {
            out.push(Capability::Tools);
        }
        if self.reasoning.is_some() {
            out.push(Capability::Reasoning);
        }
        if self.json_mode {
            out.push(Capability::JsonMode);
        }
        for m in &self.messages {
            for p in &m.parts {
                match p {
                    ContentPart::ImageUrl(_) | ContentPart::File { .. } => {
                        if !out.contains(&Capability::Vision) {
                            out.push(Capability::Vision);
                        }
                    }
                    ContentPart::Audio { .. } => {
                        if !out.contains(&Capability::Audio) {
                            out.push(Capability::Audio);
                        }
                    }
                    ContentPart::Text(_) => {}
                }
            }
        }
        out
    }

    /// Texto plano de los mensajes de sistema, concatenado.
    pub fn system_text(&self) -> Option<String> {
        let joined: Vec<String> = self
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| m.text())
            .filter(|t| !t.is_empty())
            .collect();
        if joined.is_empty() {
            None
        } else {
            Some(joined.join("\n\n"))
        }
    }
}

impl Message {
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

// ---------------------------------------------------------------------------
// Respuesta
// ---------------------------------------------------------------------------

/// Vocabulario común de eventos al que traduce cada adaptador, independiente
/// del formato del origen.
#[derive(Debug, Clone)]
pub enum ChatEvent {
    /// Marca el punto de medición del tiempo hasta el primer token.
    Started { provider_request_id: Option<String> },
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args_json: String },
    /// Argumentos completos comunicados al cerrar el item. Sustituyen los
    /// deltas acumulados y cubren proveedores que no emiten deltas.
    ToolCallArgumentsDone { id: String, args_json: String },
    ToolCallEnd { id: String },
    /// Puede no llegar nunca: en las rutas de suscripción no llega.
    Usage(UsageReport),
    Finished { reason: FinishReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Cancelled,
    Error,
}

impl FinishReason {
    pub fn as_openai(&self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ToolCalls => "tool_calls",
            Self::ContentFilter => "content_filter",
            Self::Cancelled => "stop",
            Self::Error => "stop",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct UsageReport {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cached_input_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
    pub source: UsageSource,
    /// El objeto de uso tal como lo devolvió el proveedor, sin transformar.
    /// Normalizar sirve para comparar; perder el original impide auditar.
    pub raw: Option<serde_json::Value>,
}

impl UsageReport {
    pub fn total_tokens(&self) -> Option<u32> {
        match (self.input_tokens, self.output_tokens) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
        }
    }

    pub fn unavailable() -> Self {
        Self { source: UsageSource::Unavailable, ..Default::default() }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    /// Lo comunicó el proveedor. Es un dato.
    Reported,
    /// Nexo lo calculó. Es una estimación y debe presentarse como tal.
    Estimated,
    /// Ni reportado ni estimable con fiabilidad.
    #[default]
    Unavailable,
}

impl UsageSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reported => "reported",
            Self::Estimated => "estimated",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Cómo debe interpretarse la cifra de coste de una petición.
///
/// Cuatro estados, no dos: mostrar cero euros por una petición cubierta por
/// suscripción es cierto y engañoso a la vez, porque la cuota consumida es
/// desconocida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostBasis {
    Reported,
    Estimated,
    Subscription,
    Unavailable,
}

impl CostBasis {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reported => "reported",
            Self::Estimated => "estimated",
            Self::Subscription => "subscription",
            Self::Unavailable => "unavailable",
        }
    }
}

// ---------------------------------------------------------------------------
// Catálogo
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDescriptor {
    /// Como lo llama el proveedor.
    pub api_id: String,
    /// Como lo expone Nexo. Lleva siempre el proveedor delante.
    pub public_name: String,
    pub caps: Capabilities,
    pub limits: Limits,
    pub accounting: Accounting,
    pub pricing: Option<Pricing>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub text: bool,
    pub vision: bool,
    pub audio: bool,
    pub tools: bool,
    pub reasoning: bool,
    pub json_mode: bool,
    pub streaming: bool,
    pub embeddings: bool,
    /// Niveles de esfuerzo que el modelo declara admitir, tal como los nombra el
    /// proveedor (invariante 6: se conserva el dato original). Vacío significa
    /// «no se sabe», no «ninguno»: la mayoría de vías no publican esta lista.
    ///
    /// `serde(default)` no es decorativo: las filas de catálogo ya guardadas en
    /// disco no tienen este campo, y sin él `serde` fallaría al deserializarlas.
    /// Como `catalog_rows` hace `unwrap_or_default()` al parsear, ese fallo no
    /// se vería como error sino como un modelo **sin ninguna capacidad**, que es
    /// la peor forma de romperse: en silencio y pareciendo un dato legítimo.
    #[serde(default)]
    pub reasoning_levels: Vec<String>,
}

impl Capabilities {
    pub fn supports(&self, cap: Capability) -> bool {
        match cap {
            Capability::Text => self.text,
            Capability::Vision => self.vision,
            Capability::Audio => self.audio,
            Capability::Tools => self.tools,
            Capability::Reasoning => self.reasoning,
            Capability::JsonMode => self.json_mode,
            Capability::Streaming => self.streaming,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Limits {
    pub context_max: Option<u32>,
    pub input_max: Option<u32>,
    pub output_max: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accounting {
    /// El proveedor factura por token e informa del uso.
    Metered,
    /// Cubierto por la suscripción del usuario. Coste marginal cero, cuota
    /// consumida desconocida. NO es lo mismo que gratis.
    Subscription,
    /// Ejecución local. Sin coste ni cuota.
    Local,
}

impl Accounting {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Metered => "metered",
            Self::Subscription => "subscription",
            Self::Local => "local",
        }
    }

    /// Ningún proveedor comunica el coste, solo los tokens. Por eso una ruta
    /// medida con uso reportado sigue produciendo un coste `Estimated`: la
    /// cifra sale de la tabla de precios de Nexo, no del proveedor.
    pub fn cost_basis_for(&self, usage: UsageSource) -> CostBasis {
        match (self, usage) {
            (Self::Subscription, _) => CostBasis::Subscription,
            (Self::Local, _) => CostBasis::Reported,
            (Self::Metered, UsageSource::Reported) => CostBasis::Estimated,
            (Self::Metered, UsageSource::Estimated) => CostBasis::Estimated,
            (Self::Metered, UsageSource::Unavailable) => CostBasis::Unavailable,
        }
    }
}

/// Micros de euro/dólar por millón de tokens. La unidad la fija el manifiesto.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Pricing {
    pub input_per_mtok_micros: i64,
    pub output_per_mtok_micros: i64,
    pub cached_input_per_mtok_micros: Option<i64>,
}

impl Pricing {
    pub fn cost_micros(&self, usage: &UsageReport) -> Option<i64> {
        let input = usage.input_tokens? as i64;
        let output = usage.output_tokens.unwrap_or(0) as i64;
        let cached = usage.cached_input_tokens.unwrap_or(0) as i64;
        let fresh_input = (input - cached).max(0);
        let cached_rate = self
            .cached_input_per_mtok_micros
            .unwrap_or(self.input_per_mtok_micros);
        Some(
            (fresh_input * self.input_per_mtok_micros
                + cached * cached_rate
                + output * self.output_per_mtok_micros)
                / 1_000_000,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Ok,
    Degraded,
    Down,
    /// Cuando comprobarlo consumiría cuota facturable o de suscripción.
    Unknown,
}

// ---------------------------------------------------------------------------
// Errores
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, thiserror::Error)]
pub enum AdapterError {
    /// Capacidad solicitada no soportada por esta pareja proveedor+credencial.
    #[error("capacidad no soportada: {capability}")]
    Unsupported { capability: String, hint: Option<String> },

    #[error("autenticación: {reason}")]
    Auth { reason: String, reauth_required: bool },

    #[error("el proveedor limitó la tasa")]
    RateLimited { retry_after: Option<Duration> },

    /// Límite de Nexo, no del proveedor. Se distingue en las estadísticas.
    #[error("límite local de la aplicación {app_id}")]
    LocalLimit { app_id: String, window_secs: u64, detail: String },

    #[error("el proveedor falló ({status}): {message}")]
    Upstream { status: u16, provider_code: Option<String>, message: String },

    #[error("transporte: {detail}")]
    Transport { detail: String },

    /// La respuesta del proveedor no encaja con lo esperado. Señal fuerte de
    /// que un flujo no soportado ha cambiado de forma.
    #[error("respuesta con forma inesperada: {detail}")]
    Malformed { detail: String },

    /// La ruta de suscripción ya no funciona. El mensaje al usuario debe
    /// explicarlo y ofrecer el respaldo por API key si está configurado.
    #[error("la vía de suscripción de {provider} ha dejado de funcionar: {detail}")]
    SubscriptionPathBroken { provider: String, detail: String },

    #[error("cancelado por el cliente")]
    Cancelled,
}

impl AdapterError {
    pub fn http_status(&self) -> u16 {
        match self {
            Self::Unsupported { .. } => 422,
            Self::Auth { .. } => 401,
            Self::RateLimited { .. } | Self::LocalLimit { .. } => 429,
            Self::Upstream { .. } | Self::Malformed { .. } | Self::SubscriptionPathBroken { .. } => 502,
            Self::Transport { .. } => 503,
            Self::Cancelled => 499,
        }
    }

    /// Etiqueta estable para las estadísticas. `LocalLimit` y `RateLimited`
    /// son distintos a propósito: confundirlos haría inútil el diagnóstico.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Unsupported { .. } => "unsupported",
            Self::Auth { .. } => "auth",
            Self::RateLimited { .. } => "rate_limited",
            Self::LocalLimit { .. } => "local_limit",
            Self::Upstream { .. } => "upstream",
            Self::Transport { .. } => "transport",
            Self::Malformed { .. } => "malformed",
            Self::SubscriptionPathBroken { .. } => "subscription_path_broken",
            Self::Cancelled => "cancelled",
        }
    }

    /// Código de error en el cuerpo, compatible con la forma de OpenAI.
    pub fn openai_code(&self) -> &'static str {
        match self {
            Self::Unsupported { .. } => "unsupported_capability",
            Self::Auth { .. } => "invalid_credentials",
            Self::RateLimited { .. } => "rate_limit_exceeded",
            Self::LocalLimit { .. } => "nexo_app_limit_exceeded",
            Self::Upstream { .. } => "upstream_error",
            Self::Transport { .. } => "transport_error",
            Self::Malformed { .. } => "malformed_upstream_response",
            Self::SubscriptionPathBroken { .. } => "subscription_path_broken",
            Self::Cancelled => "request_cancelled",
        }
    }

    pub fn from_reqwest(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Self::Transport { detail: format!("timeout: {e}") }
        } else if e.is_decode() {
            Self::Malformed { detail: e.to_string() }
        } else {
            Self::Transport { detail: e.to_string() }
        }
    }
}

pub type EventStream = BoxStream<'static, std::result::Result<ChatEvent, AdapterError>>;

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> AdapterId;

    /// Modelos disponibles con esta credencial concreta. No el catálogo
    /// completo del proveedor: lo que esta credencial puede realmente usar.
    async fn catalog(
        &self,
        cred: &ResolvedCredential,
    ) -> std::result::Result<Vec<ModelDescriptor>, AdapterError>;

    async fn stream(
        &self,
        req: &ChatRequest,
        cred: &ResolvedCredential,
    ) -> std::result::Result<EventStream, AdapterError>;

    /// Comprobación de salud. Debe ser barata y no consumir cuota facturable
    /// ni de suscripción. Si no hay endpoint gratuito, devolver `Unknown` en
    /// lugar de gastar una petición.
    async fn health(&self, _cred: &ResolvedCredential) -> Health {
        Health::Unknown
    }
}

/// Rechaza de forma explícita lo que el destino no puede hacer.
///
/// Degradar en silencio produce respuestas peores sin que el usuario sepa por
/// qué, y es el fallo que la promesa de «catálogo unificado» invita a cometer.
pub fn check_capabilities(
    req: &ChatRequest,
    model: &ModelDescriptor,
) -> std::result::Result<(), AdapterError> {
    for cap in req.required_capabilities() {
        if !model.caps.supports(cap) {
            return Err(AdapterError::Unsupported {
                capability: cap.as_str().to_string(),
                hint: Some(format!(
                    "el modelo {} no ofrece {} por esta vía de acceso",
                    model.public_name,
                    cap.as_str()
                )),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> ChatRequest {
        ChatRequest {
            api_model: "m".into(),
            public_model: "p/m".into(),
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
            stream: false,
        }
    }

    fn text_only_model() -> ModelDescriptor {
        ModelDescriptor {
            api_id: "m".into(),
            public_name: "p/m".into(),
            caps: Capabilities { text: true, streaming: true, ..Default::default() },
            limits: Limits::default(),
            accounting: Accounting::Subscription,
            pricing: None,
        }
    }

    #[test]
    fn vision_request_against_text_model_is_rejected_not_degraded() {
        let mut r = req();
        r.messages[0].parts.push(ContentPart::ImageUrl("data:image/png;base64,AA".into()));
        let err = check_capabilities(&r, &text_only_model()).unwrap_err();
        assert_eq!(err.http_status(), 422);
        assert_eq!(err.kind_str(), "unsupported");
    }

    #[test]
    fn tools_request_against_text_model_is_rejected() {
        let mut r = req();
        r.tools.push(ToolDef {
            name: "t".into(),
            description: None,
            parameters: serde_json::json!({}),
        });
        assert!(check_capabilities(&r, &text_only_model()).is_err());
    }

    #[test]
    fn plain_text_request_passes() {
        assert!(check_capabilities(&req(), &text_only_model()).is_ok());
    }

    #[test]
    fn subscription_always_yields_subscription_cost_basis() {
        assert_eq!(
            Accounting::Subscription.cost_basis_for(UsageSource::Reported),
            CostBasis::Subscription
        );
        assert_eq!(
            Accounting::Metered.cost_basis_for(UsageSource::Unavailable),
            CostBasis::Unavailable
        );
    }

    #[test]
    fn subscription_requires_app_limit() {
        assert!(CredentialKind::SubscriptionOauth.requires_app_limit());
        assert!(!CredentialKind::ApiKey.requires_app_limit());
    }

    /// Las filas de catálogo ya guardadas en disco no tienen
    /// `reasoning_levels`. Sin `#[serde(default)]` este parseo fallaría, y como
    /// `catalog_rows` hace `unwrap_or_default()`, el modelo aparecería **sin
    /// ninguna capacidad** en lugar de dar un error: se perderían `text`,
    /// `tools`, `streaming`… en silencio. Esta prueba es la que impide ese
    /// estropicio en cualquier campo que se añada aquí en el futuro.
    #[test]
    fn capabilities_without_reasoning_levels_still_deserialise() {
        let stored = r#"{"text":true,"vision":false,"audio":false,"tools":true,
                         "reasoning":true,"json_mode":true,"streaming":true,
                         "embeddings":false}"#;
        let caps: Capabilities =
            serde_json::from_str(stored).expect("una fila vieja debe seguir deserializando");
        assert!(caps.text, "no se puede perder una capacidad al añadir un campo");
        assert!(caps.tools);
        assert!(caps.streaming);
        assert!(caps.reasoning);
        assert!(
            caps.reasoning_levels.is_empty(),
            "sin dato es lista vacía, no un nivel inventado"
        );
    }

    #[test]
    fn reasoning_effort_parse_is_the_inverse_of_as_str() {
        for effort in [
            ReasoningEffort::Minimal,
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
        ] {
            assert_eq!(ReasoningEffort::parse(effort.as_str()), Some(effort));
        }
        // Un nivel que Nexo no conoce no se inventa: no es elegible (D5).
        assert_eq!(ReasoningEffort::parse("ultra"), None);
        assert_eq!(ReasoningEffort::parse(""), None);
    }

    #[test]
    fn local_limit_and_rate_limit_are_distinguishable() {
        let local = AdapterError::LocalLimit {
            app_id: "a".into(),
            window_secs: 60,
            detail: String::new(),
        };
        let remote = AdapterError::RateLimited { retry_after: None };
        assert_eq!(local.http_status(), remote.http_status());
        assert_ne!(local.kind_str(), remote.kind_str());
    }

    #[test]
    fn pricing_discounts_cached_input() {
        let p = Pricing {
            input_per_mtok_micros: 1_000_000,
            output_per_mtok_micros: 2_000_000,
            cached_input_per_mtok_micros: Some(100_000),
        };
        let usage = UsageReport {
            input_tokens: Some(1_000_000),
            output_tokens: Some(1_000_000),
            cached_input_tokens: Some(500_000),
            ..Default::default()
        };
        // 500k frescos a 1.0 + 500k cacheados a 0.1 + 1M salida a 2.0
        assert_eq!(p.cost_micros(&usage), Some(500_000 + 50_000 + 2_000_000));
    }
}
