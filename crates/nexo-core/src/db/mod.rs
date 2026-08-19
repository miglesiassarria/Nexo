//! Persistencia. SQLite embebida, única base de datos.

pub mod migrations;
pub mod stats;

use crate::config::Settings;
use crate::error::{CoreError, Result};
use crate::provider::{Accounting, CredentialKind, ModelDescriptor};
use crate::util;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        migrations::apply(&conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        migrations::apply(&conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // -- Configuración ------------------------------------------------------

    pub fn settings(&self) -> Result<Settings> {
        let conn = self.lock();
        let mut settings = Settings::default();
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (key, value) = row?;
            match key.as_str() {
                "port" => settings.port = value.parse().unwrap_or(settings.port),
                "allow_lan" => settings.allow_lan = value == "true",
                "retention_days" => {
                    settings.retention_days = value.parse().unwrap_or(settings.retention_days)
                }
                "content_retention_days" => {
                    settings.content_retention_days =
                        value.parse().unwrap_or(settings.content_retention_days)
                }
                "log_level" => settings.log_level = value,
                "manifest_version" => settings.manifest_version = value,
                "codex_client_version" => settings.codex_client_version = value,
                "lmstudio_base_url" => settings.lmstudio_base_url = value,
                "ollama_base_url" => settings.ollama_base_url = value,
                _ => {}
            }
        }
        Ok(settings)
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let conn = self.lock();
        let now = util::now_ms();
        for (key, value) in [
            ("port", s.port.to_string()),
            ("allow_lan", s.allow_lan.to_string()),
            ("retention_days", s.retention_days.to_string()),
            ("content_retention_days", s.content_retention_days.to_string()),
            ("log_level", s.log_level.clone()),
            ("manifest_version", s.manifest_version.clone()),
            ("codex_client_version", s.codex_client_version.clone()),
            ("lmstudio_base_url", s.lmstudio_base_url.clone()),
            ("ollama_base_url", s.ollama_base_url.clone()),
        ] {
            conn.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
                params![key, value, now],
            )?;
        }
        Ok(())
    }

    // -- Cuentas ------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_account(&self, account: &Account) -> Result<()> {
        // Una cuenta de suscripción sin reconocimiento de riesgo no es válida:
        // es la mitigación del ADR 0001 y se comprueba también aquí, no solo
        // en la interfaz.
        if account.credential_kind == CredentialKind::SubscriptionOauth
            && account.risk_ack_at.is_none()
        {
            return Err(CoreError::Forbidden(
                "una cuenta de suscripción exige que el usuario acepte antes el aviso de riesgo"
                    .into(),
            ));
        }

        let conn = self.lock();
        conn.execute(
            "INSERT INTO accounts
               (id, provider_id, credential_kind, label, keychain_ref, external_id,
                scopes, expires_at, status, risk_ack_at, created_at, last_used_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(provider_id, credential_kind, external_id) DO UPDATE SET
               label = ?4, keychain_ref = ?5, scopes = ?7, expires_at = ?8,
               status = ?9, risk_ack_at = COALESCE(accounts.risk_ack_at, ?10)",
            params![
                account.id,
                account.provider_id,
                account.credential_kind.as_str(),
                account.label,
                account.keychain_ref,
                account.external_id,
                account.scopes,
                account.expires_at,
                account.status,
                account.risk_ack_at,
                account.created_at,
                account.last_used_at,
            ],
        )?;
        Ok(())
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, provider_id, credential_kind, label, keychain_ref, external_id,
                    scopes, expires_at, status, risk_ack_at, created_at, last_used_at
             FROM accounts ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], Account::from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn account_for(
        &self,
        provider_id: &str,
        kind: CredentialKind,
    ) -> Result<Option<Account>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, provider_id, credential_kind, label, keychain_ref, external_id,
                    scopes, expires_at, status, risk_ack_at, created_at, last_used_at
             FROM accounts
             WHERE provider_id = ?1 AND credential_kind = ?2 AND status != 'revoked'
             ORDER BY created_at DESC LIMIT 1",
        )?;
        Ok(stmt
            .query_row(params![provider_id, kind.as_str()], Account::from_row)
            .optional()?)
    }

    pub fn account(&self, id: &str) -> Result<Option<Account>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, provider_id, credential_kind, label, keychain_ref, external_id,
                    scopes, expires_at, status, risk_ack_at, created_at, last_used_at
             FROM accounts WHERE id = ?1",
        )?;
        Ok(stmt.query_row(params![id], Account::from_row).optional()?)
    }

    pub fn set_account_tokens_meta(
        &self,
        account_id: &str,
        expires_at: i64,
        external_id: Option<&str>,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE accounts
             SET expires_at = ?2,
                 external_id = COALESCE(?3, external_id),
                 status = 'active',
                 last_used_at = ?4
             WHERE id = ?1",
            params![account_id, expires_at, external_id, util::now_ms()],
        )?;
        Ok(())
    }

    pub fn set_account_status(&self, account_id: &str, status: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "UPDATE accounts SET status = ?2 WHERE id = ?1",
            params![account_id, status],
        )?;
        Ok(())
    }

    pub fn delete_account(&self, account_id: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM accounts WHERE id = ?1", params![account_id])?;
        Ok(())
    }

    // -- Catálogo -----------------------------------------------------------

    pub fn replace_models(
        &self,
        provider_id: &str,
        kind: CredentialKind,
        models: &[ModelDescriptor],
        manifest_version: &str,
    ) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM models WHERE provider_id = ?1 AND credential_kind = ?2",
            params![provider_id, kind.as_str()],
        )?;
        let now = util::now_ms();
        for m in models {
            tx.execute(
                "INSERT INTO models
                   (provider_id, credential_kind, api_id, public_name, caps,
                    context_max, input_max, output_max, accounting,
                    price_input, price_output, price_cached_input, price_source,
                    manifest_version, available, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,1,?15)",
                params![
                    provider_id,
                    kind.as_str(),
                    m.api_id,
                    m.public_name,
                    serde_json::to_string(&m.caps)?,
                    m.limits.context_max,
                    m.limits.input_max,
                    m.limits.output_max,
                    m.accounting.as_str(),
                    m.pricing.map(|p| p.input_per_mtok_micros),
                    m.pricing.map(|p| p.output_per_mtok_micros),
                    m.pricing.and_then(|p| p.cached_input_per_mtok_micros),
                    m.pricing.map(|_| "manifest"),
                    manifest_version,
                    now,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn catalog_rows(&self) -> Result<Vec<CatalogRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT provider_id, credential_kind, api_id, public_name, caps,
                    context_max, input_max, output_max, accounting,
                    price_input, price_output, available
             FROM models ORDER BY provider_id, credential_kind, api_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(CatalogRow {
                provider_id: r.get(0)?,
                credential_kind: r.get(1)?,
                api_id: r.get(2)?,
                public_name: r.get(3)?,
                caps: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
                context_max: r.get(5)?,
                input_max: r.get(6)?,
                output_max: r.get(7)?,
                accounting: r.get(8)?,
                price_input: r.get(9)?,
                price_output: r.get(10)?,
                available: r.get::<_, i64>(11)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Resuelve un nombre público a la pareja (proveedor, credencial, api_id).
    ///
    /// Acepta `proveedor/modelo` y, por compatibilidad con clientes que no
    /// permiten cambiar el nombre, también `modelo` a secas cuando no sea
    /// ambiguo. La preferencia entre vías la decide `prefer`.
    pub fn resolve_model(
        &self,
        requested: &str,
        prefer: Option<CredentialKind>,
    ) -> Result<Option<ResolvedModel>> {
        let conn = self.lock();
        let (provider_filter, name) = match requested.split_once('/') {
            Some((p, m)) => (Some(p.to_string()), m.to_string()),
            None => (None, requested.to_string()),
        };

        let mut stmt = conn.prepare(
            "SELECT provider_id, credential_kind, api_id, public_name, accounting,
                    caps, context_max, input_max, output_max
             FROM models
             WHERE available = 1
               AND (?1 IS NULL OR provider_id = ?1)
               AND (api_id = ?2 OR public_name = ?3)",
        )?;
        let mut candidates: Vec<ResolvedModel> = stmt
            .query_map(params![provider_filter, name, requested], |r| {
                Ok(ResolvedModel {
                    provider_id: r.get(0)?,
                    credential_kind: CredentialKind::parse(&r.get::<_, String>(1)?)
                        .unwrap_or(CredentialKind::ApiKey),
                    api_id: r.get(2)?,
                    public_name: r.get(3)?,
                    accounting: r.get::<_, String>(4)?,
                    caps: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                    limits: crate::provider::Limits {
                        context_max: r.get::<_, Option<i64>>(6)?.map(|v| v as u32),
                        input_max: r.get::<_, Option<i64>>(7)?.map(|v| v as u32),
                        output_max: r.get::<_, Option<i64>>(8)?.map(|v| v as u32),
                    },
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        if candidates.is_empty() {
            return Ok(None);
        }

        // La suscripción se prefiere por defecto: es la razón de ser de Nexo y
        // no tiene coste marginal.
        let preferred = prefer.unwrap_or(CredentialKind::SubscriptionOauth);
        candidates.sort_by_key(|c| if c.credential_kind == preferred { 0 } else { 1 });
        Ok(candidates.into_iter().next())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub provider_id: String,
    pub credential_kind: CredentialKind,
    pub label: String,
    pub keychain_ref: Option<String>,
    pub external_id: Option<String>,
    pub scopes: Option<String>,
    pub expires_at: Option<i64>,
    pub status: String,
    pub risk_ack_at: Option<i64>,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

impl Account {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            provider_id: r.get(1)?,
            credential_kind: CredentialKind::parse(&r.get::<_, String>(2)?)
                .unwrap_or(CredentialKind::ApiKey),
            label: r.get(3)?,
            keychain_ref: r.get(4)?,
            external_id: r.get(5)?,
            scopes: r.get(6)?,
            expires_at: r.get(7)?,
            status: r.get(8)?,
            risk_ack_at: r.get(9)?,
            created_at: r.get(10)?,
            last_used_at: r.get(11)?,
        })
    }

    /// Un margen evita renovar justo cuando el token acaba de caducar.
    pub fn is_expired(&self, skew_ms: i64) -> bool {
        match self.expires_at {
            None => false,
            Some(exp) => util::now_ms() + skew_ms >= exp,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogRow {
    pub provider_id: String,
    pub credential_kind: String,
    pub api_id: String,
    pub public_name: String,
    pub caps: crate::provider::Capabilities,
    pub context_max: Option<i64>,
    pub input_max: Option<i64>,
    pub output_max: Option<i64>,
    pub accounting: String,
    pub price_input: Option<i64>,
    pub price_output: Option<i64>,
    pub available: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub provider_id: String,
    pub credential_kind: CredentialKind,
    pub api_id: String,
    pub public_name: String,
    pub accounting: String,
    /// Capacidades reales de esta pareja modelo+credencial, tal como las
    /// publicó el proveedor o las declara el manifiesto.
    pub caps: crate::provider::Capabilities,
    pub limits: crate::provider::Limits,
}

impl ResolvedModel {
    /// Descriptor equivalente, para comprobar capacidades sin volver a consultar.
    pub fn descriptor(&self) -> ModelDescriptor {
        ModelDescriptor {
            api_id: self.api_id.clone(),
            public_name: self.public_name.clone(),
            caps: self.caps.clone(),
            limits: self.limits.clone(),
            accounting: self.accounting_enum(),
            pricing: None,
        }
    }

    pub fn accounting_enum(&self) -> Accounting {
        match self.accounting.as_str() {
            "subscription" => Accounting::Subscription,
            "local" => Accounting::Local,
            _ => Accounting::Metered,
        }
    }
}

// ---------------------------------------------------------------------------
// Proveedores añadidos por el usuario (nombre, dirección y clave)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub compat: String,
    pub created_at: i64,
}

impl Db {
    /// Da de alta un proveedor añadido por el usuario. El `id` es el slug del
    /// nombre y es la clave primaria: dos proveedores con el mismo nombre
    /// (o que slugifiquen igual) no pueden coexistir.
    pub fn create_custom_provider(&self, name: &str, base_url: &str) -> Result<CustomProvider> {
        let name = name.trim();
        let base_url = base_url.trim().trim_end_matches('/');
        if name.is_empty() {
            return Err(CoreError::Config("el proveedor necesita un nombre".into()));
        }
        if base_url.is_empty() {
            return Err(CoreError::Config("el proveedor necesita una URL".into()));
        }
        let id = crate::util::slugify(name);
        if id.is_empty() {
            return Err(CoreError::Config(
                "el nombre no produce un identificador válido; añade alguna letra o número".into(),
            ));
        }

        let provider = CustomProvider {
            id: id.clone(),
            name: name.to_string(),
            base_url: base_url.to_string(),
            compat: "openai_compat".into(),
            created_at: util::now_ms(),
        };

        let conn = self.lock();
        conn.execute(
            "INSERT INTO custom_providers (id, name, base_url, compat, created_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![provider.id, provider.name, provider.base_url, provider.compat, provider.created_at],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                CoreError::Config(format!(
                    "ya existe un proveedor con un nombre que produce el mismo identificador «{id}»; \
                     elige otro nombre"
                ))
            }
            other => CoreError::Db(other),
        })?;
        Ok(provider)
    }

    pub fn custom_providers(&self) -> Result<Vec<CustomProvider>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, base_url, compat, created_at
             FROM custom_providers ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(CustomProvider {
                id: r.get(0)?,
                name: r.get(1)?,
                base_url: r.get(2)?,
                compat: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn custom_provider(&self, id: &str) -> Result<Option<CustomProvider>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, base_url, compat, created_at
             FROM custom_providers WHERE id = ?1",
        )?;
        Ok(stmt
            .query_row(params![id], |r| {
                Ok(CustomProvider {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    base_url: r.get(2)?,
                    compat: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })
            .optional()?)
    }

    /// Cambia la URL de un proveedor añadido. Como la dirección viaja en la
    /// credencial de la cuenta (no en el proveedor), el llamador debe actualizar
    /// también `accounts.external_id` para que surta efecto sin reiniciar.
    pub fn update_custom_provider_url(&self, id: &str, base_url: &str) -> Result<()> {
        let base_url = base_url.trim().trim_end_matches('/');
        let conn = self.lock();
        let n = conn.execute(
            "UPDATE custom_providers SET base_url = ?2 WHERE id = ?1",
            params![id, base_url],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("no hay proveedor con id {id}")));
        }
        Ok(())
    }

    /// Borra el proveedor. No borra la cuenta ni el catálogo asociados: eso lo
    /// decide quien orquesta (el servicio), porque implica también borrar el
    /// secreto del Keychain.
    pub fn delete_custom_provider(&self, id: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM custom_providers WHERE id = ?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod custom_provider_tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn creates_and_lists_a_provider() {
        let db = db();
        let p = db.create_custom_provider("OpenCode Zen", "https://opencode.ai/zen/v1").unwrap();
        assert_eq!(p.id, "opencode-zen");
        assert_eq!(p.base_url, "https://opencode.ai/zen/v1");
        assert_eq!(p.compat, "openai_compat");
        assert_eq!(db.custom_providers().unwrap().len(), 1);
    }

    #[test]
    fn trailing_slash_in_url_is_stripped() {
        let db = db();
        let p = db.create_custom_provider("Runpod", "https://runpod.io/v1/").unwrap();
        assert_eq!(p.base_url, "https://runpod.io/v1");
    }

    #[test]
    fn duplicate_name_is_rejected_not_overwritten() {
        // Criterio 3 de la especificación 0002.
        let db = db();
        db.create_custom_provider("Runpod", "https://a.example/v1").unwrap();
        let err = db
            .create_custom_provider("Runpod", "https://b.example/v1")
            .unwrap_err();
        assert!(matches!(err, CoreError::Config(_)));
        assert!(err.to_string().contains("runpod"));

        // No se sobrescribió: sigue apuntando a la primera URL.
        let providers = db.custom_providers().unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].base_url, "https://a.example/v1");
    }

    #[test]
    fn names_that_slugify_the_same_also_collide() {
        // «Mi Proveedor» y «mi   proveedor!!» producen el mismo id: es la
        // colisión que la clave primaria debe atrapar, no solo el nombre exacto.
        let db = db();
        db.create_custom_provider("Mi Proveedor", "https://a.example/v1").unwrap();
        assert!(db
            .create_custom_provider("mi   proveedor!!", "https://b.example/v1")
            .is_err());
    }

    #[test]
    fn blank_name_or_url_is_rejected() {
        let db = db();
        assert!(db.create_custom_provider("   ", "https://a.example").is_err());
        assert!(db.create_custom_provider("Runpod", "  ").is_err());
    }

    #[test]
    fn only_punctuation_name_is_rejected_with_a_clear_reason() {
        let db = db();
        let err = db.create_custom_provider("···", "https://a.example").unwrap_err();
        assert!(matches!(err, CoreError::Config(_)));
    }

    #[test]
    fn deleting_an_unknown_provider_is_not_an_error() {
        assert!(db().delete_custom_provider("no-existe").is_ok());
    }

    #[test]
    fn updating_url_of_an_unknown_provider_fails_clearly() {
        assert!(matches!(
            db().update_custom_provider_url("no-existe", "https://x"),
            Err(CoreError::NotFound(_))
        ));
    }

    #[test]
    fn update_url_strips_trailing_slash_too() {
        let db = db();
        db.create_custom_provider("Runpod", "https://a.example/v1").unwrap();
        db.update_custom_provider_url("runpod", "https://b.example/v1/").unwrap();
        assert_eq!(
            db.custom_provider("runpod").unwrap().unwrap().base_url,
            "https://b.example/v1"
        );
    }

    #[test]
    fn get_by_id_returns_none_when_absent() {
        assert!(db().custom_provider("no-existe").unwrap().is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn account(kind: CredentialKind, risk_ack: Option<i64>) -> Account {
        Account {
            id: util::new_id("acc"),
            provider_id: "openai".into(),
            credential_kind: kind,
            label: "test".into(),
            keychain_ref: Some("account/x/access".into()),
            external_id: Some("ext-1".into()),
            scopes: None,
            expires_at: Some(util::now_ms() + 60_000),
            status: "active".into(),
            risk_ack_at: risk_ack,
            created_at: util::now_ms(),
            last_used_at: None,
        }
    }

    #[test]
    fn subscription_account_without_risk_ack_is_rejected() {
        let db = db();
        let err = db
            .upsert_account(&account(CredentialKind::SubscriptionOauth, None))
            .unwrap_err();
        assert!(matches!(err, CoreError::Forbidden(_)));
        assert_eq!(db.accounts().unwrap().len(), 0);
    }

    #[test]
    fn subscription_account_with_risk_ack_is_stored() {
        let db = db();
        db.upsert_account(&account(CredentialKind::SubscriptionOauth, Some(1)))
            .unwrap();
        assert_eq!(db.accounts().unwrap().len(), 1);
    }

    #[test]
    fn api_key_account_needs_no_risk_ack() {
        let db = db();
        db.upsert_account(&account(CredentialKind::ApiKey, None))
            .unwrap();
        assert_eq!(db.accounts().unwrap().len(), 1);
    }

    #[test]
    fn settings_roundtrip_and_keep_safe_defaults() {
        let db = db();
        assert!(!db.settings().unwrap().allow_lan);
        let mut s = db.settings().unwrap();
        s.port = 9999;
        s.retention_days = 30;
        db.save_settings(&s).unwrap();
        let loaded = db.settings().unwrap();
        assert_eq!(loaded.port, 9999);
        assert_eq!(loaded.retention_days, 30);
    }

    #[test]
    fn same_model_coexists_under_both_credential_kinds() {
        let db = db();
        db.replace_models(
            "openai",
            CredentialKind::ApiKey,
            &catalog::openai_apikey_models(),
            "test",
        )
        .unwrap();
        db.replace_models(
            "openai",
            CredentialKind::SubscriptionOauth,
            &catalog::chatgpt_subscription_models(),
            "test",
        )
        .unwrap();

        let rows = db.catalog_rows().unwrap();
        let gpt55: Vec<_> = rows.iter().filter(|r| r.api_id == "gpt-5.5").collect();
        assert_eq!(gpt55.len(), 2, "debe aparecer por las dos vías");

        let sub = gpt55
            .iter()
            .find(|r| r.credential_kind == "subscription_oauth")
            .unwrap();
        assert_eq!(sub.accounting, "subscription");
        assert!(sub.price_input.is_none(), "la vía de suscripción no tiene precio");

        let key = gpt55
            .iter()
            .find(|r| r.credential_kind == "api_key")
            .unwrap();
        assert!(key.price_input.is_some());
    }

    #[test]
    fn resolve_prefers_subscription_by_default() {
        let db = db();
        db.replace_models(
            "openai",
            CredentialKind::ApiKey,
            &catalog::openai_apikey_models(),
            "t",
        )
        .unwrap();
        db.replace_models(
            "openai",
            CredentialKind::SubscriptionOauth,
            &catalog::chatgpt_subscription_models(),
            "t",
        )
        .unwrap();

        let r = db.resolve_model("gpt-5.5", None).unwrap().unwrap();
        assert_eq!(r.credential_kind, CredentialKind::SubscriptionOauth);

        let r = db
            .resolve_model("openai/gpt-5.5", Some(CredentialKind::ApiKey))
            .unwrap()
            .unwrap();
        assert_eq!(r.credential_kind, CredentialKind::ApiKey);
    }

    #[test]
    fn resolve_unknown_model_returns_none() {
        let db = db();
        assert!(db.resolve_model("no-existe", None).unwrap().is_none());
    }

    #[test]
    fn expiry_uses_a_skew_margin() {
        let mut a = account(CredentialKind::ApiKey, None);
        a.expires_at = Some(util::now_ms() + 10_000);
        assert!(!a.is_expired(0));
        assert!(a.is_expired(30_000), "con margen de 30s ya debe considerarse caducado");
        a.expires_at = None;
        assert!(!a.is_expired(999_999));
    }
}
