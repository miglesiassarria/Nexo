//! Aplicaciones cliente: emisión, autenticación y revocación de tokens.
//!
//! Un token identifica una aplicación. Es la única forma de saber quién llama,
//! porque la mayoría de herramientas solo permiten configurar una URL base y
//! una clave. Los tokens se guardan hasheados: el secreto en claro solo existe
//! en el momento de la emisión.

use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::provider::CredentialKind;
use crate::util;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

pub const TOKEN_PREFIX: &str = "nx_";

#[derive(Debug, Clone, Serialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub token_prefix: String,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
    pub revoked_at: Option<i64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Grant {
    pub provider_id: String,
    pub credential_kind: String,
    pub model_pattern: String,
    pub allow_tools: bool,
    pub allow_multimodal: bool,
    pub log_content: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Limit {
    pub provider_id: String,
    pub credential_kind: String,
    pub window_seconds: i64,
    pub max_requests: Option<i64>,
    pub max_input_tokens: Option<i64>,
    pub max_output_tokens: Option<i64>,
}

/// Token recién emitido. El secreto solo se devuelve aquí, una vez.
#[derive(Debug, Clone, Serialize)]
pub struct IssuedApp {
    pub app: App,
    pub token: String,
}

pub fn generate_token() -> String {
    format!("{TOKEN_PREFIX}{}", util::b64url(&util::random_bytes(32)))
}

impl Db {
    pub fn create_app(&self, name: &str, notes: Option<&str>) -> Result<IssuedApp> {
        if name.trim().is_empty() {
            return Err(CoreError::Config("la aplicación necesita un nombre".into()));
        }
        let token = generate_token();
        let id = util::new_id("app");
        let now = util::now_ms();
        let prefix: String = token.chars().take(TOKEN_PREFIX.len() + 6).collect();

        let conn = self.lock();
        conn.execute(
            "INSERT INTO apps (id, name, token_hash, token_prefix, created_at, notes)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                id,
                name.trim(),
                util::sha256_hex(token.as_bytes()),
                prefix,
                now,
                notes
            ],
        )?;

        Ok(IssuedApp {
            app: App {
                id,
                name: name.trim().to_string(),
                token_prefix: prefix,
                created_at: now,
                last_seen_at: None,
                revoked_at: None,
                notes: notes.map(str::to_string),
            },
            token,
        })
    }

    pub fn apps(&self) -> Result<Vec<App>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, token_prefix, created_at, last_seen_at, revoked_at, notes
             FROM apps ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(App {
                id: r.get(0)?,
                name: r.get(1)?,
                token_prefix: r.get(2)?,
                created_at: r.get(3)?,
                last_seen_at: r.get(4)?,
                revoked_at: r.get(5)?,
                notes: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Autentica un bearer token. Devuelve la aplicación si está activa.
    pub fn authenticate(&self, token: &str) -> Result<Option<App>> {
        let hash = util::sha256_hex(token.as_bytes());
        let conn = self.lock();
        let app = conn
            .query_row(
                "SELECT id, name, token_prefix, created_at, last_seen_at, revoked_at, notes
                 FROM apps WHERE token_hash = ?1",
                params![hash],
                |r| {
                    Ok(App {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        token_prefix: r.get(2)?,
                        created_at: r.get(3)?,
                        last_seen_at: r.get(4)?,
                        revoked_at: r.get(5)?,
                        notes: r.get(6)?,
                    })
                },
            )
            .optional()?;

        match app {
            Some(app) if app.revoked_at.is_none() => {
                conn.execute(
                    "UPDATE apps SET last_seen_at = ?2 WHERE id = ?1",
                    params![app.id, util::now_ms()],
                )?;
                Ok(Some(app))
            }
            // Un token revocado no autentica, y no se distingue de uno
            // inexistente hacia el cliente.
            _ => Ok(None),
        }
    }

    pub fn revoke_app(&self, app_id: &str) -> Result<()> {
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE apps SET revoked_at = ?2 WHERE id = ?1 AND revoked_at IS NULL",
            params![app_id, util::now_ms()],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!(
                "no hay aplicación activa con id {app_id}"
            )));
        }
        Ok(())
    }

    pub fn delete_app(&self, app_id: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM apps WHERE id = ?1", params![app_id])?;
        Ok(())
    }

    // -- Permisos -----------------------------------------------------------

    pub fn set_grant(&self, app_id: &str, grant: &Grant) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO app_grants
               (app_id, provider_id, credential_kind, model_pattern,
                allow_tools, allow_multimodal, log_content)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(app_id, provider_id, credential_kind, model_pattern)
             DO UPDATE SET allow_tools = ?5, allow_multimodal = ?6, log_content = ?7",
            params![
                app_id,
                grant.provider_id,
                grant.credential_kind,
                grant.model_pattern,
                grant.allow_tools as i64,
                grant.allow_multimodal as i64,
                grant.log_content as i64,
            ],
        )?;
        Ok(())
    }

    pub fn remove_grant(
        &self,
        app_id: &str,
        provider_id: &str,
        credential_kind: &str,
        model_pattern: &str,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM app_grants
             WHERE app_id = ?1 AND provider_id = ?2 AND credential_kind = ?3
               AND model_pattern = ?4",
            params![app_id, provider_id, credential_kind, model_pattern],
        )?;
        Ok(())
    }

    pub fn grants(&self, app_id: &str) -> Result<Vec<Grant>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT provider_id, credential_kind, model_pattern,
                    allow_tools, allow_multimodal, log_content
             FROM app_grants WHERE app_id = ?1",
        )?;
        let rows = stmt.query_map(params![app_id], |r| {
            Ok(Grant {
                provider_id: r.get(0)?,
                credential_kind: r.get(1)?,
                model_pattern: r.get(2)?,
                allow_tools: r.get::<_, i64>(3)? != 0,
                allow_multimodal: r.get::<_, i64>(4)? != 0,
                log_content: r.get::<_, i64>(5)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // -- Límites ------------------------------------------------------------

    pub fn set_limit(&self, app_id: &str, limit: &Limit) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO app_limits
               (app_id, provider_id, credential_kind, window_seconds,
                max_requests, max_input_tokens, max_output_tokens)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(app_id, provider_id, credential_kind, window_seconds)
             DO UPDATE SET max_requests = ?5, max_input_tokens = ?6,
                           max_output_tokens = ?7",
            params![
                app_id,
                limit.provider_id,
                limit.credential_kind,
                limit.window_seconds,
                limit.max_requests,
                limit.max_input_tokens,
                limit.max_output_tokens,
            ],
        )?;
        Ok(())
    }

    pub fn limits(&self, app_id: &str) -> Result<Vec<Limit>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT provider_id, credential_kind, window_seconds,
                    max_requests, max_input_tokens, max_output_tokens
             FROM app_limits WHERE app_id = ?1",
        )?;
        let rows = stmt.query_map(params![app_id], |r| {
            Ok(Limit {
                provider_id: r.get(0)?,
                credential_kind: r.get(1)?,
                window_seconds: r.get(2)?,
                max_requests: r.get(3)?,
                max_input_tokens: r.get(4)?,
                max_output_tokens: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Concede acceso y, si la vía es de suscripción, fija a la vez el límite
    /// obligatorio. No existe forma de conceder lo uno sin lo otro: es lo que
    /// hace que los ocho parámetros viajen juntos en lugar de por separado.
    #[allow(clippy::too_many_arguments)]
    pub fn grant_with_mandatory_limit(
        &self,
        app_id: &str,
        provider_id: &str,
        kind: CredentialKind,
        allow_tools: bool,
        allow_multimodal: bool,
        max_requests: Option<i64>,
        window_seconds: Option<i64>,
    ) -> Result<()> {
        self.set_grant(
            app_id,
            &Grant {
                provider_id: provider_id.to_string(),
                credential_kind: kind.as_str().to_string(),
                model_pattern: "*".into(),
                allow_tools,
                allow_multimodal,
                log_content: false,
            },
        )?;

        if kind.requires_app_limit() {
            let window =
                window_seconds.unwrap_or(crate::config::DEFAULT_SUBSCRIPTION_LIMIT_WINDOW_SECS);
            let max = max_requests
                .unwrap_or(crate::config::DEFAULT_SUBSCRIPTION_LIMIT_REQUESTS)
                .max(1);
            self.set_limit(
                app_id,
                &Limit {
                    provider_id: provider_id.to_string(),
                    credential_kind: kind.as_str().to_string(),
                    window_seconds: window,
                    max_requests: Some(max),
                    max_input_tokens: None,
                    max_output_tokens: None,
                },
            )?;
        }
        Ok(())
    }

    /// Reemplaza de golpe qué modelos de una vía puede usar una aplicación.
    ///
    /// Una fila por modelo marcado, y el conjunto se sustituye entero en una
    /// transacción: marcar, desmarcar y «marcar los sesenta visibles» son la misma
    /// operación con distinto conjunto. Un comando por modelo serían sesenta
    /// escrituras con estados intermedios visibles si una fallara a mitad.
    ///
    /// Un conjunto vacío retira la vía: no existe «concedida sin modelos», porque
    /// sería un estado que no sirve nada y no se distingue de no estar concedida.
    ///
    /// Las capacidades son de la vía, no del modelo, así que se escriben iguales en
    /// todas sus filas. El límite obligatorio de suscripción se fija aquí por el
    /// mismo motivo que en `grant_with_mandatory_limit`: no hay forma de conceder lo
    /// uno sin lo otro (ADR 0001).
    #[allow(clippy::too_many_arguments)]
    pub fn replace_app_models(
        &self,
        app_id: &str,
        provider_id: &str,
        kind: CredentialKind,
        models: &[String],
        allow_tools: bool,
        allow_multimodal: bool,
        max_requests: Option<i64>,
        window_seconds: Option<i64>,
    ) -> Result<()> {
        {
            let mut conn = self.lock();
            let tx = conn.transaction()?;
            tx.execute(
                "DELETE FROM app_grants
                 WHERE app_id = ?1 AND provider_id = ?2 AND credential_kind = ?3",
                params![app_id, provider_id, kind.as_str()],
            )?;
            for model in models {
                let model = model.trim();
                if model.is_empty() {
                    continue;
                }
                tx.execute(
                    "INSERT INTO app_grants
                       (app_id, provider_id, credential_kind, model_pattern,
                        allow_tools, allow_multimodal, log_content)
                     VALUES (?1,?2,?3,?4,?5,?6,0)",
                    params![
                        app_id,
                        provider_id,
                        kind.as_str(),
                        model,
                        allow_tools as i64,
                        allow_multimodal as i64,
                    ],
                )?;
            }
            tx.commit()?;
        }

        if models.iter().any(|m| !m.trim().is_empty()) && kind.requires_app_limit() {
            let window =
                window_seconds.unwrap_or(crate::config::DEFAULT_SUBSCRIPTION_LIMIT_WINDOW_SECS);
            let max = max_requests
                .unwrap_or(crate::config::DEFAULT_SUBSCRIPTION_LIMIT_REQUESTS)
                .max(1);
            self.set_limit(
                app_id,
                &Limit {
                    provider_id: provider_id.to_string(),
                    credential_kind: kind.as_str().to_string(),
                    window_seconds: window,
                    max_requests: Some(max),
                    max_input_tokens: None,
                    max_output_tokens: None,
                },
            )?;
        }
        Ok(())
    }

    /// Invariante del ADR 0001: ningún grant de suscripción sin límite.
    /// Devuelve los `app_id` que la incumplen.
    pub fn apps_missing_mandatory_limits(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT g.app_id
             FROM app_grants g
             WHERE g.credential_kind = 'subscription_oauth'
               AND NOT EXISTS (
                 SELECT 1 FROM app_limits l
                 WHERE l.app_id = g.app_id
                   AND l.provider_id = g.provider_id
                   AND l.credential_kind = g.credential_kind
                   AND l.max_requests IS NOT NULL
               )",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn token_is_prefixed_and_long_enough() {
        let t = generate_token();
        assert!(t.starts_with(TOKEN_PREFIX));
        assert!(t.len() > 40);
        assert_ne!(t, generate_token());
    }

    #[test]
    fn plaintext_token_is_never_stored() {
        let db = db();
        let issued = db.create_app("cliente", None).unwrap();
        let conn = db.lock();
        let stored: String = conn
            .query_row("SELECT token_hash FROM apps", [], |r| r.get(0))
            .unwrap();
        assert_ne!(stored, issued.token);
        assert_eq!(stored, util::sha256_hex(issued.token.as_bytes()));
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM apps WHERE token_hash = ?1",
                params![issued.token],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "el token en claro no debe existir en la base");
    }

    #[test]
    fn authenticate_accepts_valid_token_and_updates_last_seen() {
        let db = db();
        let issued = db.create_app("cliente", None).unwrap();
        let app = db.authenticate(&issued.token).unwrap().expect("autenticado");
        assert_eq!(app.id, issued.app.id);
        assert!(db.apps().unwrap()[0].last_seen_at.is_some());
    }

    #[test]
    fn authenticate_rejects_unknown_and_revoked_tokens() {
        let db = db();
        assert!(db.authenticate("nx_desconocido").unwrap().is_none());

        let issued = db.create_app("cliente", None).unwrap();
        db.revoke_app(&issued.app.id).unwrap();
        assert!(
            db.authenticate(&issued.token).unwrap().is_none(),
            "un token revocado no debe autenticar"
        );
    }

    #[test]
    fn revoking_one_app_does_not_affect_others() {
        let db = db();
        let a = db.create_app("a", None).unwrap();
        let b = db.create_app("b", None).unwrap();
        db.revoke_app(&a.app.id).unwrap();
        assert!(db.authenticate(&a.token).unwrap().is_none());
        assert!(db.authenticate(&b.token).unwrap().is_some());
    }

    #[test]
    fn revoking_twice_reports_not_found() {
        let db = db();
        let a = db.create_app("a", None).unwrap();
        db.revoke_app(&a.app.id).unwrap();
        assert!(matches!(
            db.revoke_app(&a.app.id).unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn app_needs_a_name() {
        assert!(db().create_app("   ", None).is_err());
    }

    // -- Modelos permitidos por vía (spec 0004) -----------------------------

    fn models(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn replace_app_models_stores_one_row_per_marked_model() {
        let db = db();
        let app = db.create_app("cliente", None).unwrap().app;
        db.replace_app_models(
            &app.id,
            "zen",
            CredentialKind::ApiKey,
            &models(&["zen/uno", "zen/dos"]),
            true,
            false,
            None,
            None,
        )
        .unwrap();

        let mut patterns: Vec<String> =
            db.grants(&app.id).unwrap().into_iter().map(|g| g.model_pattern).collect();
        patterns.sort();
        assert_eq!(patterns, vec!["zen/dos", "zen/uno"]);
        // Las capacidades son de la vía: iguales en todas sus filas.
        assert!(db.grants(&app.id).unwrap().iter().all(|g| g.allow_tools && !g.allow_multimodal));
    }

    #[test]
    fn replace_app_models_does_not_leave_the_previous_selection_behind() {
        let db = db();
        let app = db.create_app("cliente", None).unwrap().app;
        let put = |list: &[&str]| {
            db.replace_app_models(
                &app.id,
                "zen",
                CredentialKind::ApiKey,
                &models(list),
                true,
                true,
                None,
                None,
            )
            .unwrap()
        };

        put(&["zen/uno", "zen/dos", "zen/tres"]);
        put(&["zen/dos"]);

        let patterns: Vec<String> =
            db.grants(&app.id).unwrap().into_iter().map(|g| g.model_pattern).collect();
        assert_eq!(patterns, vec!["zen/dos"], "reemplazar sustituye, no acumula");
    }

    #[test]
    fn an_empty_selection_withdraws_the_route() {
        let db = db();
        let app = db.create_app("cliente", None).unwrap().app;
        db.replace_app_models(
            &app.id,
            "zen",
            CredentialKind::ApiKey,
            &models(&["zen/uno"]),
            true,
            true,
            None,
            None,
        )
        .unwrap();
        assert_eq!(db.grants(&app.id).unwrap().len(), 1);

        db.replace_app_models(&app.id, "zen", CredentialKind::ApiKey, &[], true, true, None, None)
            .unwrap();
        assert!(
            db.grants(&app.id).unwrap().is_empty(),
            "sin modelos marcados no hay vía concedida: no existe el estado intermedio"
        );
    }

    #[test]
    fn replacing_one_route_leaves_the_others_alone() {
        let db = db();
        let app = db.create_app("cliente", None).unwrap().app;
        db.replace_app_models(
            &app.id,
            "zen",
            CredentialKind::ApiKey,
            &models(&["zen/uno"]),
            true,
            true,
            None,
            None,
        )
        .unwrap();
        db.replace_app_models(
            &app.id,
            "lmstudio",
            CredentialKind::Local,
            &models(&["lmstudio/a", "lmstudio/b"]),
            true,
            true,
            None,
            None,
        )
        .unwrap();

        // Y borrar una vía entera no toca la otra.
        db.replace_app_models(&app.id, "zen", CredentialKind::ApiKey, &[], true, true, None, None)
            .unwrap();
        let grants = db.grants(&app.id).unwrap();
        assert_eq!(grants.len(), 2);
        assert!(grants.iter().all(|g| g.provider_id == "lmstudio"));
    }

    /// El ADR 0001 no se relaja por poder elegir modelos: marcar uno solo de la vía de
    /// suscripción sigue creando su límite obligatorio.
    #[test]
    fn marking_a_single_subscription_model_still_creates_the_mandatory_limit() {
        let db = db();
        let app = db.create_app("cliente", None).unwrap().app;
        db.replace_app_models(
            &app.id,
            "openai",
            CredentialKind::SubscriptionOauth,
            &models(&["openai/gpt-5.5"]),
            true,
            true,
            None,
            None,
        )
        .unwrap();

        let limits = db.limits(&app.id).unwrap();
        assert_eq!(limits.len(), 1);
        assert_eq!(
            limits[0].max_requests,
            Some(crate::config::DEFAULT_SUBSCRIPTION_LIMIT_REQUESTS)
        );
        assert!(db.apps_missing_mandatory_limits().unwrap().is_empty());
    }

    #[test]
    fn subscription_grant_always_creates_a_limit() {
        let db = db();
        let app = db.create_app("cliente", None).unwrap();
        db.grant_with_mandatory_limit(
            &app.app.id,
            "openai",
            CredentialKind::SubscriptionOauth,
            true,
            true,
            None,
            None,
        )
        .unwrap();

        let limits = db.limits(&app.app.id).unwrap();
        assert_eq!(limits.len(), 1);
        assert_eq!(
            limits[0].max_requests,
            Some(crate::config::DEFAULT_SUBSCRIPTION_LIMIT_REQUESTS)
        );
        assert!(db.apps_missing_mandatory_limits().unwrap().is_empty());
    }

    #[test]
    fn api_key_grant_does_not_force_a_limit() {
        let db = db();
        let app = db.create_app("cliente", None).unwrap();
        db.grant_with_mandatory_limit(
            &app.app.id,
            "openai",
            CredentialKind::ApiKey,
            true,
            true,
            None,
            None,
        )
        .unwrap();
        assert!(db.limits(&app.app.id).unwrap().is_empty());
        assert!(db.apps_missing_mandatory_limits().unwrap().is_empty());
    }

    #[test]
    fn invariant_detects_a_bare_subscription_grant() {
        let db = db();
        let app = db.create_app("cliente", None).unwrap();
        // Grant «a pelo», sin pasar por el camino que impone el límite.
        db.set_grant(
            &app.app.id,
            &Grant {
                provider_id: "openai".into(),
                credential_kind: "subscription_oauth".into(),
                model_pattern: "*".into(),
                allow_tools: false,
                allow_multimodal: false,
                log_content: false,
            },
        )
        .unwrap();
        assert_eq!(
            db.apps_missing_mandatory_limits().unwrap(),
            vec![app.app.id],
            "la invariante del ADR 0001 debe detectarlo"
        );
    }

    #[test]
    fn limit_of_zero_is_clamped_to_one() {
        let db = db();
        let app = db.create_app("cliente", None).unwrap();
        db.grant_with_mandatory_limit(
            &app.app.id,
            "openai",
            CredentialKind::SubscriptionOauth,
            false,
            false,
            Some(0),
            Some(60),
        )
        .unwrap();
        assert_eq!(db.limits(&app.app.id).unwrap()[0].max_requests, Some(1));
    }
}
