//! MÓDULO FRÁGIL — OAuth de suscripción de ChatGPT.
//!
//! Todo lo que puede romperse sin aviso está aquí y solo aquí. Ningún otro
//! módulo debe conocer estas constantes ni la forma de estas peticiones.
//!
//! Nada de esto está documentado ni versionado por OpenAI. Se apoya en el
//! client_id público del cliente oficial de línea de comandos y en el backend
//! de la aplicación de ChatGPT. Ver `docs/adr/0001-oauth-de-suscripcion.md`.
//!
//! ÚLTIMA VERIFICACIÓN: 2026-07-30.
//!
//! Si algo aquí deja de funcionar, el adaptador debe devolver
//! `AdapterError::SubscriptionPathBroken` para que la interfaz pueda explicar
//! qué ha pasado y ofrecer el respaldo por API key.

use crate::error::{CoreError, Result};
use crate::util;
use serde::Deserialize;
use std::time::Duration;

// --- Constantes frágiles ---------------------------------------------------

/// Client_id público del cliente oficial de línea de comandos de OpenAI.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const ISSUER: &str = "https://auth.openai.com";
/// Backend de la aplicación de ChatGPT. No es la API pública.
pub const API_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/responses";
/// El puerto es parte del `redirect_uri` registrado: no es configurable.
pub const CALLBACK_PORT: u16 = 1455;
pub const CALLBACK_PATH: &str = "/auth/callback";
pub const SCOPE: &str = "openid profile email offline_access";
/// Nexo se identifica como Nexo. No suplantamos a otro cliente.
pub const ORIGINATOR: &str = "nexo";

pub fn redirect_uri() -> String {
    format!("http://localhost:{CALLBACK_PORT}{CALLBACK_PATH}")
}

// --- PKCE -----------------------------------------------------------------

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
        Self { verifier, challenge }
    }
}

// --- Construcción de la URL de autorización -------------------------------

pub fn authorize_url(pkce: &Pkce, state: &str) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", &redirect_uri()),
        ("scope", SCOPE),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", ORIGINATOR),
    ];
    let query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect();
    format!("{ISSUER}/oauth/authorize?{}", query.join("&"))
}

// --- Tokens ---------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
}

impl TokenResponse {
    pub fn expires_at_ms(&self) -> i64 {
        util::now_ms() + self.expires_in.unwrap_or(3600) * 1000
    }

    /// Identificador de cuenta, buscado en el `id_token` y, si no está, en el
    /// `access_token`.
    pub fn account_id(&self) -> Option<String> {
        self.id_token
            .as_deref()
            .and_then(account_id_from_jwt)
            .or_else(|| account_id_from_jwt(&self.access_token))
    }
}

pub fn account_id_from_jwt(token: &str) -> Option<String> {
    let claims = util::jwt_claims(token)?;
    account_id_from_claims(&claims)
}

pub fn account_id_from_claims(claims: &serde_json::Value) -> Option<String> {
    if let Some(v) = claims.get("chatgpt_account_id").and_then(|v| v.as_str()) {
        return Some(v.to_string());
    }
    if let Some(v) = claims
        .get("https://api.openai.com/auth")
        .and_then(|a| a.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
    {
        return Some(v.to_string());
    }
    claims
        .get("organizations")
        .and_then(|o| o.as_array())
        .and_then(|a| a.first())
        .and_then(|o| o.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub fn account_email(id_token: &str) -> Option<String> {
    util::jwt_claims(id_token)?
        .get("email")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub async fn exchange_code(
    http: &reqwest::Client,
    code: &str,
    pkce: &Pkce,
) -> Result<TokenResponse> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", &redirect_uri()),
        ("client_id", CLIENT_ID),
        ("code_verifier", &pkce.verifier),
    ];
    post_token(http, &form).await
}

pub async fn refresh(http: &reqwest::Client, refresh_token: &str) -> Result<TokenResponse> {
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", CLIENT_ID),
    ];
    post_token(http, &form).await
}

async fn post_token(http: &reqwest::Client, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let resp = http
        .post(format!("{ISSUER}/oauth/token"))
        .timeout(Duration::from_secs(30))
        .form(form)
        .send()
        .await
        .map_err(|e| CoreError::Auth(format!("no se pudo contactar con {ISSUER}: {e}")))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(CoreError::Auth(format!(
            "el proveedor rechazó la petición de token ({}): {}",
            status.as_u16(),
            truncate(&body, 400)
        )));
    }

    serde_json::from_str::<TokenResponse>(&body).map_err(|e| {
        // Forma inesperada en un flujo no versionado: señal de que ha cambiado.
        CoreError::Auth(format!(
            "respuesta de token con forma inesperada ({e}). \
             Es la señal de que el flujo no soportado ha cambiado: {}",
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
        let p = Pkce::generate();
        assert_eq!(p.verifier.len(), 43);
        assert_eq!(p.challenge, util::b64url(&util::sha256(p.verifier.as_bytes())));
        assert!(!p.challenge.contains('='));
        assert!(!p.challenge.contains('+'));
    }

    #[test]
    fn pkce_verifier_is_unreserved_only() {
        let p = Pkce::generate();
        assert!(p
            .verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
    }

    #[test]
    fn authorize_url_declares_nexo_as_originator() {
        let url = authorize_url(&Pkce::generate(), "st4te");
        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
        assert!(url.contains("originator=nexo"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=st4te"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
        assert!(url.contains("scope=openid%20profile%20email%20offline_access"));
    }

    #[test]
    fn account_id_prefers_direct_claim() {
        let claims = serde_json::json!({
            "chatgpt_account_id": "direct",
            "organizations": [{"id": "org"}]
        });
        assert_eq!(account_id_from_claims(&claims).as_deref(), Some("direct"));
    }

    #[test]
    fn account_id_falls_back_to_namespaced_then_org() {
        let ns = serde_json::json!({
            "https://api.openai.com/auth": {"chatgpt_account_id": "ns"}
        });
        assert_eq!(account_id_from_claims(&ns).as_deref(), Some("ns"));

        let org = serde_json::json!({"organizations": [{"id": "org-1"}]});
        assert_eq!(account_id_from_claims(&org).as_deref(), Some("org-1"));

        let empty = serde_json::json!({});
        assert_eq!(account_id_from_claims(&empty), None);
    }
}
