//! Políticas, permisos y límites por aplicación.
//!
//! Los límites en rutas de suscripción NO son una preferencia: son la
//! mitigación del riesgo de multiplexación del ADR 0001. Nexo multiplexa N
//! aplicaciones sobre una cuota personal, y ese patrón es el que un proveedor
//! interpreta como abuso.

use crate::apps::{Grant, Limit};
use crate::db::Db;
use crate::provider::{AdapterError, ChatRequest, ContentPart, CredentialKind};
use crate::util;
use std::collections::HashMap;
use std::sync::Mutex;

/// Contador deslizante en memoria, reconstruible desde `requests`.
#[derive(Default)]
pub struct RateTracker {
    windows: Mutex<HashMap<String, Vec<i64>>>,
}

impl RateTracker {
    fn key(app_id: &str, provider_id: &str, kind: &str, window_secs: i64) -> String {
        format!("{app_id}|{provider_id}|{kind}|{window_secs}")
    }

    /// Peticiones registradas en la ventana, purgando las que ya salieron.
    pub fn count(&self, app_id: &str, provider_id: &str, kind: &str, window_secs: i64) -> i64 {
        let key = Self::key(app_id, provider_id, kind, window_secs);
        let cutoff = util::now_ms() - window_secs * 1000;
        let mut windows = self.windows.lock().unwrap();
        let entry = windows.entry(key).or_default();
        entry.retain(|ts| *ts >= cutoff);
        entry.len() as i64
    }

    pub fn record(&self, app_id: &str, provider_id: &str, kind: &str, window_secs: i64) {
        let key = Self::key(app_id, provider_id, kind, window_secs);
        let mut windows = self.windows.lock().unwrap();
        windows.entry(key).or_default().push(util::now_ms());
    }

    /// Siembra el contador desde la base de datos al arrancar.
    pub fn seed(&self, app_id: &str, provider_id: &str, kind: &str, window_secs: i64, n: i64) {
        let key = Self::key(app_id, provider_id, kind, window_secs);
        let now = util::now_ms();
        let mut windows = self.windows.lock().unwrap();
        windows.insert(key, vec![now; n.max(0) as usize]);
    }
}

/// Resultado de evaluar las políticas para una petición.
pub struct PolicyDecision {
    pub grant: Grant,
    /// Límite aplicable y consumo actual, para informar al cliente.
    pub limit: Option<(Limit, i64)>,
}

pub struct PolicyEngine {
    db: Db,
    tracker: RateTracker,
}

impl PolicyEngine {
    pub fn new(db: Db) -> Self {
        Self { db, tracker: RateTracker::default() }
    }

    pub fn tracker(&self) -> &RateTracker {
        &self.tracker
    }

    /// Reconstruye los contadores desde el histórico.
    pub fn warm_up(&self) -> crate::Result<()> {
        for app in self.db.apps()? {
            if app.revoked_at.is_some() {
                continue;
            }
            for limit in self.db.limits(&app.id)? {
                let since = util::now_ms() - limit.window_seconds * 1000;
                let n = self.db.requests_in_window(
                    &app.id,
                    &limit.provider_id,
                    &limit.credential_kind,
                    since,
                )?;
                self.tracker.seed(
                    &app.id,
                    &limit.provider_id,
                    &limit.credential_kind,
                    limit.window_seconds,
                    n,
                );
            }
        }
        Ok(())
    }

    /// Comprueba permisos y límites. No modifica contadores.
    pub fn check(
        &self,
        app_id: &str,
        provider_id: &str,
        kind: CredentialKind,
        public_model: &str,
        req: &ChatRequest,
    ) -> Result<PolicyDecision, AdapterError> {
        let grants = self
            .db
            .grants(app_id)
            .map_err(|e| AdapterError::Transport { detail: e.to_string() })?;

        // Sin fila no hay permiso: el acceso se concede, no se deniega.
        let grant = grants
            .into_iter()
            .find(|g| {
                g.provider_id == provider_id
                    && g.credential_kind == kind.as_str()
                    && model_matches(&g.model_pattern, public_model)
            })
            .ok_or_else(|| AdapterError::Auth {
                reason: format!(
                    "esta aplicación no tiene permiso para usar {public_model} \
                     por la vía {}. Concédelo desde Nexo.",
                    kind.as_str()
                ),
                reauth_required: false,
            })?;

        if !grant.allow_tools && !req.tools.is_empty() {
            return Err(AdapterError::Unsupported {
                capability: "tools".into(),
                hint: Some(
                    "esta aplicación no tiene permiso para usar herramientas".into(),
                ),
            });
        }

        let sends_media = req.messages.iter().any(|m| {
            m.parts
                .iter()
                .any(|p| !matches!(p, ContentPart::Text(_)))
        });
        if !grant.allow_multimodal && sends_media {
            return Err(AdapterError::Unsupported {
                capability: "multimodal".into(),
                hint: Some(
                    "esta aplicación no tiene permiso para enviar contenido multimodal".into(),
                ),
            });
        }

        let limits = self
            .db
            .limits(app_id)
            .map_err(|e| AdapterError::Transport { detail: e.to_string() })?;
        let applicable: Vec<Limit> = limits
            .into_iter()
            .filter(|l| l.provider_id == provider_id && l.credential_kind == kind.as_str())
            .collect();

        // Cerrojo del ADR 0001: la vía de suscripción no funciona sin límite.
        // Es más seguro rechazar que dejar la cuota personal sin protección.
        if kind.requires_app_limit()
            && !applicable.iter().any(|l| l.max_requests.is_some())
        {
            return Err(AdapterError::LocalLimit {
                app_id: app_id.to_string(),
                window_secs: 0,
                detail: "las rutas de suscripción exigen un límite por aplicación y esta \
                         no tiene ninguno configurado. Nexo no atenderá la petición hasta \
                         que se defina."
                    .into(),
            });
        }

        let mut tightest: Option<(Limit, i64)> = None;
        for limit in applicable {
            let Some(max) = limit.max_requests else { continue };
            let used = self.tracker.count(
                app_id,
                provider_id,
                &limit.credential_kind,
                limit.window_seconds,
            );
            if used >= max {
                return Err(AdapterError::LocalLimit {
                    app_id: app_id.to_string(),
                    window_secs: limit.window_seconds as u64,
                    detail: format!(
                        "{used}/{max} peticiones consumidas en una ventana de {} s. \
                         Esta vía comparte la cuota de tu suscripción personal.",
                        limit.window_seconds
                    ),
                });
            }
            let remaining = max - used;
            match &tightest {
                Some((_, prev)) if *prev <= remaining => {}
                _ => tightest = Some((limit, remaining)),
            }
        }

        Ok(PolicyDecision { grant, limit: tightest })
    }

    /// Cuenta una petición admitida en todas sus ventanas.
    pub fn record(&self, app_id: &str, provider_id: &str, kind: CredentialKind) {
        if let Ok(limits) = self.db.limits(app_id) {
            for limit in limits {
                if limit.provider_id == provider_id && limit.credential_kind == kind.as_str() {
                    self.tracker.record(
                        app_id,
                        provider_id,
                        &limit.credential_kind,
                        limit.window_seconds,
                    );
                }
            }
        }
    }
}

/// `*` comodín total, `prefijo*` por prefijo, o coincidencia exacta.
fn model_matches(pattern: &str, model: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    match pattern.strip_suffix('*') {
        Some(prefix) => model.starts_with(prefix),
        None => pattern == model,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Message, Role, ToolChoice, ToolDef};

    fn req() -> ChatRequest {
        ChatRequest {
            api_model: "gpt-5.5".into(),
            public_model: "openai/gpt-5.5".into(),
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

    fn setup(kind: CredentialKind, max: Option<i64>) -> (PolicyEngine, String) {
        let db = Db::open_in_memory().unwrap();
        let app = db.create_app("cliente", None).unwrap();
        db.set_grant(
            &app.app.id,
            &Grant {
                provider_id: "openai".into(),
                credential_kind: kind.as_str().into(),
                model_pattern: "*".into(),
                allow_tools: false,
                allow_multimodal: false,
                log_content: false,
            },
        )
        .unwrap();
        if let Some(max) = max {
            db.set_limit(
                &app.app.id,
                &Limit {
                    provider_id: "openai".into(),
                    credential_kind: kind.as_str().into(),
                    window_seconds: 60,
                    max_requests: Some(max),
                    max_input_tokens: None,
                    max_output_tokens: None,
                },
            )
            .unwrap();
        }
        (PolicyEngine::new(db), app.app.id)
    }

    #[test]
    fn patterns_match_as_documented() {
        assert!(model_matches("*", "openai/gpt-5.5"));
        assert!(model_matches("openai/*", "openai/gpt-5.5"));
        assert!(!model_matches("google/*", "openai/gpt-5.5"));
        assert!(model_matches("openai/gpt-5.5", "openai/gpt-5.5"));
        assert!(!model_matches("openai/gpt-5.4", "openai/gpt-5.5"));
    }

    #[test]
    fn without_a_grant_access_is_denied() {
        let db = Db::open_in_memory().unwrap();
        let app = db.create_app("cliente", None).unwrap();
        let engine = PolicyEngine::new(db);
        let err = engine
            .check(
                &app.app.id,
                "openai",
                CredentialKind::ApiKey,
                "openai/gpt-5.5",
                &req(),
            )
            .unwrap_err();
        assert_eq!(err.http_status(), 401);
    }

    #[test]
    fn subscription_without_limit_is_refused_not_allowed() {
        let (engine, app_id) = setup(CredentialKind::SubscriptionOauth, None);
        let err = engine
            .check(
                &app_id,
                "openai",
                CredentialKind::SubscriptionOauth,
                "openai/gpt-5.5",
                &req(),
            )
            .unwrap_err();
        assert_eq!(err.kind_str(), "local_limit");
        assert!(err.to_string().contains("límite local"));
    }

    #[test]
    fn api_key_without_limit_is_allowed() {
        let (engine, app_id) = setup(CredentialKind::ApiKey, None);
        let decision = engine
            .check(
                &app_id,
                "openai",
                CredentialKind::ApiKey,
                "openai/gpt-5.5",
                &req(),
            )
            .expect("la vía de API key no exige límite");
        assert!(decision.limit.is_none());
    }

    #[test]
    fn limit_is_enforced_after_being_consumed() {
        let (engine, app_id) = setup(CredentialKind::SubscriptionOauth, Some(2));
        for expected_remaining in [2, 1] {
            let d = engine
                .check(
                    &app_id,
                    "openai",
                    CredentialKind::SubscriptionOauth,
                    "openai/gpt-5.5",
                    &req(),
                )
                .expect("dentro del límite");
            assert_eq!(d.limit.as_ref().unwrap().1, expected_remaining);
            engine.record(&app_id, "openai", CredentialKind::SubscriptionOauth);
        }

        let err = engine
            .check(
                &app_id,
                "openai",
                CredentialKind::SubscriptionOauth,
                "openai/gpt-5.5",
                &req(),
            )
            .unwrap_err();
        assert_eq!(err.kind_str(), "local_limit");
        assert_eq!(err.http_status(), 429);
        assert!(err.to_string().contains("2/2") || format!("{err:?}").contains("2/2"));
    }

    #[test]
    fn tools_need_explicit_permission() {
        let (engine, app_id) = setup(CredentialKind::ApiKey, None);
        let mut r = req();
        r.tools = vec![ToolDef {
            name: "t".into(),
            description: None,
            parameters: serde_json::json!({}),
        }];
        let err = engine
            .check(&app_id, "openai", CredentialKind::ApiKey, "openai/gpt-5.5", &r)
            .unwrap_err();
        assert_eq!(err.http_status(), 422);
    }

    #[test]
    fn multimodal_needs_explicit_permission() {
        let (engine, app_id) = setup(CredentialKind::ApiKey, None);
        let mut r = req();
        r.messages[0]
            .parts
            .push(ContentPart::ImageUrl("data:image/png;base64,AA".into()));
        let err = engine
            .check(&app_id, "openai", CredentialKind::ApiKey, "openai/gpt-5.5", &r)
            .unwrap_err();
        assert_eq!(err.http_status(), 422);
    }

    #[test]
    fn tracker_forgets_entries_outside_the_window() {
        let tracker = RateTracker::default();
        tracker.seed("a", "openai", "subscription_oauth", 60, 5);
        assert_eq!(tracker.count("a", "openai", "subscription_oauth", 60), 5);
        // Ventana de cero segundos: todo queda fuera.
        assert_eq!(tracker.count("a", "openai", "subscription_oauth", 0), 0);
    }

    #[test]
    fn tracker_keys_do_not_collide_across_apps_or_routes() {
        let tracker = RateTracker::default();
        tracker.record("a", "openai", "subscription_oauth", 60);
        assert_eq!(tracker.count("b", "openai", "subscription_oauth", 60), 0);
        assert_eq!(tracker.count("a", "openai", "api_key", 60), 0);
    }
}
