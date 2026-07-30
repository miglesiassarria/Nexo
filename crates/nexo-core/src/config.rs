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
    /// Exponer el gateway fuera de localhost. Desactivado por defecto y sin
    /// implementación de transporte seguro: el gateway se niega a arrancar en
    /// 0.0.0.0 mientras no exista.
    pub allow_lan: bool,
    pub retention_days: i64,
    pub content_retention_days: i64,
    pub log_level: String,
    pub manifest_version: String,
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
        }
    }
}

impl Settings {
    pub fn bind_addr(&self) -> std::net::SocketAddr {
        // El acceso por red requiere autenticación, autorización y transporte
        // seguro. Hasta que existan, `allow_lan` no cambia el bind.
        std::net::SocketAddr::from(([127, 0, 0, 1], self.port))
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
    fn allow_lan_does_not_widen_the_bind_yet() {
        let s = Settings { allow_lan: true, ..Default::default() };
        assert_eq!(
            s.bind_addr().ip().to_string(),
            "127.0.0.1",
            "sin transporte seguro no se expone fuera de localhost"
        );
    }
}
