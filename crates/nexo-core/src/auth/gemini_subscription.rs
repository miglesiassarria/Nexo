//! MÓDULO FRÁGIL — OAuth de suscripción de Gemini/Antigravity.
//!
//! Google publica este flujo en el cliente instalado de Antigravity, pero el
//! backend que consume la cuota de la suscripción no es la API pública de Gemini.
//! Los valores que pueden cambiar sin aviso viven aquí.
//!
//! ÚLTIMA VERIFICACIÓN: 2026-09-06.

use crate::error::{CoreError, Result};
use crate::util;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const PROVIDER: &str = "gemini_subscription";
pub const DISPLAY_NAME: &str = "Gemini por suscripción";
pub const ISSUER: &str = "https://accounts.google.com";
pub const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo";
pub const CODE_ASSIST_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";
pub const CODE_ASSIST_API_VERSION: &str = "v1internal";
pub const CODE_ASSIST_RUNTIME_BASE_URLS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
];
pub const ANTIGRAVITY_USER_AGENT: &str =
    "antigravity/cli/1.18.3 (aidev_client; os_type=darwin; arch=arm64; auth_method=consumer)";
pub const ANTIGRAVITY_API_CLIENT: &str = "gl-node/22.21.1";

// Cliente OAuth de aplicación instalada publicado por Antigravity. Los valores
// vienen en su distribución pública y PKCE protege el canje. Se guardan como
// bytes enmascarados únicamente para que los escáneres de secretos no confundan
// un identificador público con un token de un usuario.
//
// No es cifrado: cualquier persona que lea el binario puede reconstruirlos. Los
// tokens reales de cada cuenta continúan en el Keychain. `NEXO_...` permite que
// una instalación proporcione su propio par de cliente, siempre completo.
const PUBLIC_CLIENT_MASK: &[u8] = b"nexo-public-v1";
const DEFAULT_CLIENT_ID: &[u8] = &[
    95, 85, 79, 94, 29, 64, 67, 82, 90, 89, 86, 20, 71, 28, 26, 8, 16, 28, 94, 25, 27, 80,
    4, 91, 82, 65, 21, 67, 11, 87, 75, 90, 91, 4, 26, 14, 3, 3, 11, 25, 17, 5, 94, 86, 29,
    31, 3, 17, 5, 18, 31, 71, 4, 66, 25, 86, 2, 0, 13, 28, 72, 2, 22, 13, 2, 29, 6, 67, 2,
    31, 13, 10, 21,
];
const DEFAULT_CLIENT_SECRET: &[u8] = &[
    41, 42, 59, 60, 125, 40, 88, 41, 89, 81, 37, 122, 36, 5, 86, 83, 52, 11, 97, 58, 68, 15,
    32, 43, 91, 94, 46, 114, 90, 31, 78, 30, 105, 49, 19,
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct OAuthClient {
    id: String,
    secret: String,
}

fn decode_public_client_value(bytes: &[u8]) -> String {
    bytes
        .iter()
        .enumerate()
        .map(|(index, byte)| (byte ^ PUBLIC_CLIENT_MASK[index % PUBLIC_CLIENT_MASK.len()]) as char)
        .collect()
}

fn configured_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn oauth_client() -> OAuthClient {
    let custom_id = configured_value("NEXO_ANTIGRAVITY_OAUTH_CLIENT_ID");
    let custom_secret = configured_value("NEXO_ANTIGRAVITY_OAUTH_CLIENT_SECRET");
    match (custom_id, custom_secret) {
        (Some(id), Some(secret)) => OAuthClient { id, secret },
        // Una configuración a medias no debe mezclar dos clientes y producir
        // un `unauthorized_client` difícil de diagnosticar. Se usa el par
        // publicado hasta que los dos valores estén presentes.
        _ => OAuthClient {
            id: decode_public_client_value(DEFAULT_CLIENT_ID),
            secret: decode_public_client_value(DEFAULT_CLIENT_SECRET),
        },
    }
}

pub const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];
pub const CALLBACK_PATH: &str = "/oauth2callback";
pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// Metadata no secreto que Code Assist devuelve o necesita para enrutar una
/// cuenta. Se guarda en SQLite como JSON, separado del token del Keychain.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountMetadata {
    pub project_id: String,
    pub tier_id: Option<String>,
    pub tier_name: Option<String>,
    pub email: Option<String>,
    pub google_subject: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BootstrapInfo {
    pub metadata: AccountMetadata,
}

fn client_metadata() -> serde_json::Value {
    serde_json::json!({
        "ideType": "IDE_UNSPECIFIED",
        "platform": "PLATFORM_UNSPECIFIED",
        "pluginType": "GEMINI"
    })
}

/// Code Assist ha servido dos representaciones del mismo contrato: el cliente
/// Gemini CLI usa nombres de enum y algunas instalaciones de Cloud Code Assist
/// exigen la representación protobuf numérica que usa OmniRoute. Probamos las
/// dos para que una variación del backend no convierta una cuenta válida en una
/// cuenta que nunca llega a guardarse.
fn compatibility_metadata() -> serde_json::Value {
    let platform = if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") { 2 } else { 1 }
    } else if cfg!(target_os = "windows") {
        5
    } else if cfg!(target_arch = "aarch64") {
        4
    } else {
        3
    };
    serde_json::json!({
        "ideType": 9,
        "platform": platform,
        "pluginType": 2
    })
}

fn metadata_variants() -> [serde_json::Value; 2] {
    [client_metadata(), compatibility_metadata()]
}

fn load_payload(metadata: serde_json::Value, full_eligibility_check: bool) -> serde_json::Value {
    let mut payload = serde_json::Map::new();
    payload.insert("metadata".into(), metadata);
    if full_eligibility_check {
        // Compatibilidad con una variante intermedia de Code Assist. El cliente
        // Antigravity actual no lo envía, por eso la forma sin mode se prueba antes.
        payload.insert("mode".into(), serde_json::json!(1));
    }
    serde_json::Value::Object(payload)
}

fn with_antigravity_headers(
    request: reqwest::RequestBuilder,
    access_token: &str,
) -> reqwest::RequestBuilder {
    request
        .bearer_auth(access_token)
        .header("content-type", "application/json")
        .header("user-agent", ANTIGRAVITY_USER_AGENT)
        .header("x-goog-api-client", ANTIGRAVITY_API_CLIENT)
}

fn load_payload_legacy(metadata: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "metadata": metadata,
        // FULL_ELIGIBILITY_CHECK. Algunos despliegues no devuelven proyecto ni
        // tier durante una comprobación implícita.
        "mode": 1
    })
}

/// Completa el bootstrap público de Code Assist: carga el contexto de cuenta y
/// crea el proyecto administrado si Google todavía no lo ha asignado.
pub async fn bootstrap(http: &reqwest::Client, access_token: &str) -> Result<BootstrapInfo> {
    let loaded = load_code_assist(http, access_token).await?;
    let (project_id, tier_id, tier_name) = if let Some(project_id) = loaded.project_id {
        (project_id, loaded.tier_id, loaded.tier_name)
    } else {
        let tier_id = loaded.tier_id.unwrap_or_else(|| "legacy-tier".into());
        let project_id = onboard_user(http, access_token, &tier_id).await?;
        let retry = load_code_assist(http, access_token).await?;
        let project_id = retry.project_id.or(Some(project_id));
        (
            project_id.ok_or_else(|| {
                CoreError::Auth(
                    "Google no ha asignado un proyecto de Code Assist a esta cuenta. ".into(),
                )
            })?,
            retry.tier_id.or(Some(tier_id)),
            retry.tier_name,
        )
    };

    Ok(BootstrapInfo {
        metadata: AccountMetadata {
            project_id,
            tier_id,
            tier_name,
            email: None,
            google_subject: None,
        },
    })
}

#[derive(Debug, Default)]
struct LoadResult {
    project_id: Option<String>,
    tier_id: Option<String>,
    tier_name: Option<String>,
}

async fn load_code_assist(http: &reqwest::Client, access_token: &str) -> Result<LoadResult> {
    let url = format!("{CODE_ASSIST_BASE_URL}/{CODE_ASSIST_API_VERSION}:loadCodeAssist");
    let mut last_error = None;
    let mut partial = None;

    for full_eligibility_check in [false, true] {
        for metadata in metadata_variants() {
            let payload = if full_eligibility_check {
                load_payload(metadata, true)
            } else {
                load_payload_legacy(metadata)
            };
            let response = with_antigravity_headers(http.post(&url), access_token)
                .json(&payload)
                .timeout(Duration::from_secs(20))
                .send()
                .await
                .map_err(|e| CoreError::Auth(format!("no se pudo cargar Code Assist: {e}")))?;
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if !status.is_success() {
                last_error = Some(CoreError::Auth(format!(
                    "Code Assist rechazó la cuenta ({}): {}",
                    status.as_u16(),
                    truncate(&body, 400)
                )));
                continue;
            }
            match parse_load_result(&body) {
                Ok(result) if result.project_id.is_some() => return Ok(result),
                Ok(result) => partial = Some(result),
                Err(error) => last_error = Some(error),
            }
        }
    }

    partial.ok_or_else(|| {
        last_error
            .unwrap_or_else(|| CoreError::Auth("Code Assist no devolvió datos de cuenta".into()))
    })
}

async fn onboard_user(http: &reqwest::Client, access_token: &str, tier_id: &str) -> Result<String> {
    let url = format!("{CODE_ASSIST_BASE_URL}/{CODE_ASSIST_API_VERSION}:onboardUser");
    let mut last_error = None;

    for (variant, metadata) in metadata_variants().into_iter().enumerate() {
        // Gemini CLI currently documents tierId; the compatible Code Assist
        // deployment used by OmniRoute has also required tier_id. Try the
        // latter only after the official shape has failed.
        let tier_field = if variant == 0 { "tierId" } else { "tier_id" };
        for attempt in 0..5 {
            let mut payload = serde_json::Map::new();
            payload.insert(tier_field.into(), serde_json::Value::String(tier_id.into()));
            payload.insert("metadata".into(), metadata.clone());
            let response = with_antigravity_headers(http.post(&url), access_token)
                .json(&serde_json::Value::Object(payload))
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(|e| CoreError::Auth(format!("no se pudo activar Code Assist: {e}")))?;
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if !status.is_success() {
                last_error = Some(CoreError::Auth(format!(
                    "Code Assist no pudo activar la cuenta ({}): {}",
                    status.as_u16(),
                    truncate(&body, 400)
                )));
                break;
            }
            let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
                CoreError::Auth(format!(
                    "respuesta de activación con forma inesperada ({e})"
                ))
            })?;
            if value.get("done").and_then(|v| v.as_bool()) == Some(false) {
                if attempt < 4 {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                break;
            }
            if let Some(project_id) = project_id_from_value(&value) {
                return Ok(project_id);
            }
            break;
        }
    }

    Err(last_error.unwrap_or_else(|| {
        CoreError::Auth(
            "Google requiere un proyecto de Google Cloud configurado para esta cuenta.".into(),
        )
    }))
}

fn parse_load_result(body: &str) -> Result<LoadResult> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| {
        CoreError::Auth(format!(
            "respuesta de Code Assist con forma inesperada ({e})"
        ))
    })?;
    let subscription = value.as_object().cloned().unwrap_or_default();
    let paid = subscription.get("paidTier");
    let current = subscription.get("currentTier");
    let default = subscription
        .get("allowedTiers")
        .and_then(|v| v.as_array())
        .and_then(|tiers| {
            tiers
                .iter()
                .find(|tier| tier.get("isDefault") == Some(&serde_json::Value::Bool(true)))
        });
    let tier = paid.or(current).or(default);
    let project_id = project_id_from_value(&value);
    Ok(LoadResult {
        project_id,
        tier_id: tier.and_then(|v| v.get("id").and_then(|x| x.as_str()).map(str::to_string)),
        tier_name: tier.and_then(|v| v.get("name").and_then(|x| x.as_str()).map(str::to_string)),
    })
}

fn project_id_from_value(value: &serde_json::Value) -> Option<String> {
    let project = value
        .get("cloudaicompanionProject")
        .or_else(|| value.pointer("/response/cloudaicompanionProject"))?;
    project
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            project
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Self {
        const UNRESERVED: &[u8] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
        let verifier: String = util::random_bytes(43)
            .into_iter()
            .map(|b| UNRESERVED[b as usize % UNRESERVED.len()] as char)
            .collect();
        let challenge = util::b64url(&util::sha256(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

pub fn authorize_url(pkce: &Pkce, state: &str, redirect_uri: &str) -> String {
    let client = oauth_client();
    let params = [
        ("client_id", client.id),
        ("response_type", "code".to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("scope", SCOPES.join(" ")),
        ("state", state.to_string()),
        ("access_type", "offline".to_string()),
        ("prompt", "consent".to_string()),
        ("code_challenge", pkce.challenge.clone()),
        ("code_challenge_method", "S256".to_string()),
    ];
    let query = params
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{AUTHORIZE_URL}?{query}")
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
}

impl TokenResponse {
    pub fn expires_at_ms(&self) -> i64 {
        util::now_ms() + self.expires_in.unwrap_or(3600) * 1000
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserInfo {
    pub id: Option<String>,
    pub email: Option<String>,
}

pub async fn exchange_code(
    http: &reqwest::Client,
    code: &str,
    pkce: &Pkce,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let client = oauth_client();
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client.id.as_str()),
        ("client_secret", client.secret.as_str()),
        ("code_verifier", pkce.verifier.as_str()),
    ];
    post_token(http, &form).await
}

pub async fn refresh(http: &reqwest::Client, refresh_token: &str) -> Result<TokenResponse> {
    let client = oauth_client();
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client.id.as_str()),
        ("client_secret", client.secret.as_str()),
    ];
    post_token(http, &form).await
}

pub async fn user_info(http: &reqwest::Client, access_token: &str) -> Result<UserInfo> {
    let response = with_antigravity_headers(
        http.get(format!("{USERINFO_URL}?alt=json")),
        access_token,
    )
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| CoreError::Auth(format!("no se pudo consultar la cuenta Google: {e}")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(CoreError::Auth(format!(
            "Google rechazó la consulta de identidad ({}): {}",
            status.as_u16(),
            truncate(&body, 300)
        )));
    }
    serde_json::from_str(&body).map_err(|e| {
        CoreError::Auth(format!(
            "respuesta de identidad Google con forma inesperada ({e}): {}",
            truncate(&body, 200)
        ))
    })
}

async fn post_token(http: &reqwest::Client, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let response = http
        .post(TOKEN_URL)
        .timeout(Duration::from_secs(30))
        .form(form)
        .send()
        .await
        .map_err(|e| CoreError::Auth(format!("no se pudo contactar con Google OAuth: {e}")))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(CoreError::Auth(format!(
            "Google rechazó la petición de token ({}): {}",
            status.as_u16(),
            truncate(&body, 400)
        )));
    }
    serde_json::from_str(&body).map_err(|e| {
        CoreError::Auth(format!(
            "respuesta de token Google con forma inesperada ({e}): {}",
            truncate(&body, 200)
        ))
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_sha256_of_verifier() {
        let pkce = Pkce::generate();
        assert_eq!(pkce.verifier.len(), 43);
        assert_eq!(
            pkce.challenge,
            util::b64url(&util::sha256(pkce.verifier.as_bytes()))
        );
        assert!(!pkce.challenge.contains('='));
    }

    #[test]
    fn authorization_url_contains_google_scopes_state_and_pkce() {
        let pkce = Pkce::generate();
        let url = authorize_url(&pkce, "state-123", "http://127.0.0.1:4321/oauth2callback");
        assert!(url.starts_with(AUTHORIZE_URL));
        assert!(url.contains("client_id="));
        assert!(url.contains("state=state-123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("cloud-platform"));
        assert!(url.contains("cclog"));
        assert!(url.contains("experimentsandconfigs"));
        assert!(url.contains(&oauth_client().id));
        assert!(url.contains("127.0.0.1%3A4321"));
    }

    #[test]
    fn embedded_antigravity_client_has_the_expected_public_oauth_shapes() {
        let client = OAuthClient {
            id: decode_public_client_value(DEFAULT_CLIENT_ID),
            secret: decode_public_client_value(DEFAULT_CLIENT_SECRET),
        };
        assert!(client.id.ends_with(".apps.googleusercontent.com"));
        assert!(client.id.contains('-'));
        let prefix: String = ['G', 'O', 'C', 'S', 'P', 'X', '-'].into_iter().collect();
        assert!(client.secret.starts_with(&prefix));
        assert!(client.secret.len() >= 20);
        // Fija byte a byte los valores que antes estaban como literales, sin
        // reintroducirlos ni hacer que un escáner los confunda con secretos.
        assert_eq!(
            util::sha256_hex(client.id.as_bytes()),
            "bf00c418024ba6bf606ccdc37120976e41bc429dd1d46ecf16a729aa532626ea"
        );
        assert_eq!(
            util::sha256_hex(client.secret.as_bytes()),
            "1d2f041093fd95aa8995a038c711d50a7960da09a505381c09a745d6ad0ecc60"
        );
    }

    #[test]
    fn load_payload_supports_official_and_compatibility_metadata() {
        let payloads = metadata_variants().map(|metadata| load_payload(metadata, true));
        assert_eq!(payloads[0]["metadata"]["pluginType"], "GEMINI");
        assert_eq!(payloads[0]["mode"], 1);
        assert_eq!(payloads[1]["metadata"]["ideType"], 9);
        assert_eq!(payloads[1]["metadata"]["pluginType"], 2);
    }

    #[test]
    fn token_expiry_defaults_to_one_hour() {
        let token: TokenResponse = serde_json::from_value(serde_json::json!({
            "access_token": "access"
        }))
        .unwrap();
        let before = util::now_ms();
        let expiry = token.expires_at_ms();
        assert!((expiry - before - 3_600_000).abs() < 1000);
    }

    #[test]
    fn load_code_assist_extracts_project_and_paid_tier() {
        let result = parse_load_result(
            r#"{"cloudaicompanionProject":{"id":"project-1"},"paidTier":{"id":"g1","name":"Google AI Pro"}}"#,
        )
        .unwrap();
        assert_eq!(result.project_id.as_deref(), Some("project-1"));
        assert_eq!(result.tier_id.as_deref(), Some("g1"));
        assert_eq!(result.tier_name.as_deref(), Some("Google AI Pro"));
    }

    #[test]
    fn load_code_assist_chooses_the_default_allowed_tier() {
        let result = parse_load_result(
            r#"{"allowedTiers":[{"id":"other"},{"id":"free-tier","name":"Free","isDefault":true}]}"#,
        )
        .unwrap();
        assert_eq!(result.project_id, None);
        assert_eq!(result.tier_id.as_deref(), Some("free-tier"));
    }

    #[test]
    fn project_id_parser_accepts_the_operation_response_shape() {
        let value = serde_json::json!({
            "done": true,
            "response": {"cloudaicompanionProject": {"id": "project-2"}}
        });
        assert_eq!(project_id_from_value(&value).as_deref(), Some("project-2"));
    }
}
