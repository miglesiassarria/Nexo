//! Configuración no sensible. Se persiste en la tabla `settings`.
//!
//! La configuración inicial debe ser segura: escucha local, acceso LAN
//! desactivado y sin registro de contenido.

use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 8787;
pub const DEFAULT_RETENTION_DAYS: i64 = 90;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub port: u16,
    /// Exponer el gateway fuera de localhost. Desactivado por defecto. El
    /// transporte va sin cifrar y exige aceptar el aviso correspondiente —
    /// ver ADR 0005.
    pub allow_lan: bool,
    pub retention_days: i64,
    pub content_retention_days: i64,
    pub log_level: String,
    pub manifest_version: String,
    /// Versión de cliente que se declara al pedir el catálogo de la vía de
    /// suscripción. Subirla expone familias de modelos más nuevas.
    pub codex_client_version: String,
    /// Dirección del servidor local de LM Studio.
    pub lmstudio_base_url: String,
    /// Dirección del servidor local de Ollama.
    pub ollama_base_url: String,
    /// Tamaño máximo permitido para peticiones entrantes de chat en bytes (None = sin límite de Nexo).
    pub max_request_body_bytes: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            allow_lan: false,
            retention_days: DEFAULT_RETENTION_DAYS,
            content_retention_days: 7,
            log_level: "info".into(),
            manifest_version: crate::catalog::MANIFEST_VERSION.into(),
            codex_client_version: crate::auth::chatgpt::DEFAULT_CLIENT_VERSION.into(),
            lmstudio_base_url: crate::provider::lmstudio::DEFAULT_BASE_URL.into(),
            ollama_base_url: crate::provider::ollama::DEFAULT_BASE_URL.into(),
            max_request_body_bytes: Some(crate::db::DEFAULT_MAX_REQUEST_BODY_BYTES),
        }
    }
}

impl Settings {
    /// La dirección que `Settings` pide. No es necesariamente la que el
    /// gateway usará de verdad: si el certificado de la red local no está
    /// listo, `Nexo::prepare_gateway_bind` puede decidir `127.0.0.1` aunque
    /// aquí `allow_lan` sea `true` — eso es una decisión de arranque, no de
    /// configuración pura, y por eso vive en `service.rs`, no aquí.
    pub fn bind_addr(&self) -> std::net::SocketAddr {
        if self.allow_lan {
            std::net::SocketAddr::from(([0, 0, 0, 0], self.port))
        } else {
            std::net::SocketAddr::from(([127, 0, 0, 1], self.port))
        }
    }
}

/// Límite por defecto para las rutas de suscripción.
///
/// No es una preferencia que se pueda dejar en blanco: es la mitigación del
/// riesgo de multiplexación del ADR 0001. Conservador a propósito.
pub const DEFAULT_SUBSCRIPTION_LIMIT_REQUESTS: i64 = 60;
pub const DEFAULT_SUBSCRIPTION_LIMIT_WINDOW_SECS: i64 = 3600;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe() {
        let s = Settings::default();
        assert!(!s.allow_lan, "el acceso LAN debe venir desactivado");
        assert_eq!(s.bind_addr().ip().to_string(), "127.0.0.1");
    }

    #[test]
    fn allow_lan_widens_the_bind() {
        let s = Settings { allow_lan: true, ..Default::default() };
        assert_eq!(
            s.bind_addr().ip().to_string(),
            "0.0.0.0",
            "con el modo red activo, Settings pide todas las interfaces"
        );
    }
}
