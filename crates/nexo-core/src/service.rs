//! El servicio Nexo: orquesta credenciales, enrutado, políticas y métricas.

use crate::apps::{App, IssuedApp};
use crate::auth::{self, chatgpt};
use crate::catalog;
use crate::config::Settings;
use crate::db::{Account, Db, ResolvedModel};
use crate::error::{CoreError, Result};
use crate::gateway::wire::WireChatRequest;
use crate::policy::PolicyEngine;
use crate::provider::{
    chatgpt_subscription::ChatgptSubscriptionAdapter, mock::MockAdapter,
    openai_apikey::OpenAiApiKeyAdapter, Accounting, AdapterError, AdapterId, ChatEvent,
    ChatRequest, CostBasis, CredentialKind, EventStream, FinishReason, ProviderAdapter,
    ResolvedCredential, UsageReport, UsageSource,
};
use crate::secrets::{SecretRef, SecretStore, SystemSecretStore};
use crate::util;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Margen para renovar antes de que el token caduque de verdad.
const REFRESH_SKEW_MS: i64 = 120_000;

pub struct Nexo {
    db: Db,
    secrets: Arc<dyn SecretStore>,
    http: reqwest::Client,
    adapters: HashMap<String, Arc<dyn ProviderAdapter>>,
    policy: PolicyEngine,
    paused: AtomicBool,
    /// Motivo por el que el gateway no está escuchando, si no lo está.
    /// Sin esto, un puerto ocupado dejaría el panel diciendo «Activo».
    bind_error: std::sync::RwLock<Option<String>>,
    /// Serializa la renovación de tokens por cuenta: una sola en vuelo.
    refresh_locks: tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Nexo {
    pub fn new(db: Db, secrets: Arc<dyn SecretStore>) -> Result<Arc<Self>> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("nexo/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()?;

        let mut adapters: HashMap<String, Arc<dyn ProviderAdapter>> = HashMap::new();
        for adapter in [
            Arc::new(ChatgptSubscriptionAdapter::new(http.clone())) as Arc<dyn ProviderAdapter>,
            Arc::new(OpenAiApiKeyAdapter::new(http.clone())),
            Arc::new(MockAdapter::default()),
        ] {
            adapters.insert(adapter.id().slug(), adapter);
        }

        let policy = PolicyEngine::new(db.clone());

        let nexo = Arc::new(Self {
            db,
            secrets,
            http,
            adapters,
            policy,
            paused: AtomicBool::new(false),
            bind_error: std::sync::RwLock::new(None),
            refresh_locks: tokio::sync::Mutex::new(HashMap::new()),
        });

        nexo.sync_catalog()?;
        nexo.policy.warm_up()?;
        Ok(nexo)
    }

    pub fn open_default(secrets: Arc<dyn SecretStore>) -> Result<Arc<Self>> {
        Self::new(Db::open(&default_db_path())?, secrets)
    }

    pub fn open_with_system_secrets() -> Result<Arc<Self>> {
        Self::open_default(Arc::new(SystemSecretStore))
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.secrets
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn set_bind_error(&self, detail: Option<String>) {
        if let Ok(mut slot) = self.bind_error.write() {
            *slot = detail;
        }
    }

    pub fn bind_error(&self) -> Option<String> {
        self.bind_error.read().ok().and_then(|s| s.clone())
    }

    /// El gateway solo está sirviendo si reservó el puerto y no está en pausa.
    pub fn is_listening(&self) -> bool {
        self.bind_error().is_none()
    }

    /// Escribe el manifiesto de modelos en el catálogo, por vía de acceso.
    pub fn sync_catalog(&self) -> Result<()> {
        self.db.replace_models(
            "openai",
            CredentialKind::SubscriptionOauth,
            &catalog::chatgpt_subscription_models(),
            catalog::MANIFEST_VERSION,
        )?;
        self.db.replace_models(
            "openai",
            CredentialKind::ApiKey,
            &catalog::openai_apikey_models(),
            catalog::MANIFEST_VERSION,
        )?;
        self.db.replace_models(
            "mock",
            CredentialKind::Mock,
            &[MockAdapter::descriptor()],
            catalog::MANIFEST_VERSION,
        )?;
        Ok(())
    }

    // -- OAuth --------------------------------------------------------------

    /// Ejecuta el flujo completo de conexión de una cuenta de ChatGPT.
    ///
    /// `risk_acknowledged` debe ser el instante en que el usuario aceptó el
    /// aviso del ADR 0001. Sin él la cuenta no se guarda.
    pub async fn connect_chatgpt_subscription(
        &self,
        risk_acknowledged_at: i64,
        open_browser: impl FnOnce(&str) -> Result<()>,
    ) -> Result<Account> {
        if risk_acknowledged_at <= 0 {
            return Err(CoreError::Forbidden(
                "hay que aceptar el aviso de riesgo antes de conectar una suscripción".into(),
            ));
        }

        let pkce = chatgpt::Pkce::generate();
        let state = auth::new_state();
        let url = chatgpt::authorize_url(&pkce, &state);

        // El servidor tiene que estar escuchando antes de abrir el navegador.
        let waiter = tokio::spawn({
            let state = state.clone();
            async move {
                auth::callback::wait_for_code(
                    chatgpt::CALLBACK_PORT,
                    chatgpt::CALLBACK_PATH,
                    &state,
                    auth::callback::DEFAULT_TIMEOUT,
                )
                .await
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;

        open_browser(&url)?;

        let code = waiter
            .await
            .map_err(|e| CoreError::Auth(format!("el flujo OAuth se interrumpió: {e}")))??;

        let tokens = chatgpt::exchange_code(&self.http, &code, &pkce).await?;
        let account_id = util::new_id("acc");
        let external_id = tokens.account_id();
        let email = tokens.id_token.as_deref().and_then(chatgpt::account_email);

        self.secrets
            .set(&SecretRef::access(&account_id), &tokens.access_token)?;
        if let Some(refresh) = &tokens.refresh_token {
            self.secrets
                .set(&SecretRef::refresh(&account_id), refresh)?;
        }

        let account = Account {
            id: account_id.clone(),
            provider_id: chatgpt_subscription_provider().to_string(),
            credential_kind: CredentialKind::SubscriptionOauth,
            label: email
                .clone()
                .map(|e| format!("ChatGPT ({e})"))
                .unwrap_or_else(|| "ChatGPT".to_string()),
            keychain_ref: Some(SecretRef::access(&account_id).as_str().to_string()),
            external_id,
            scopes: Some(chatgpt::SCOPE.to_string()),
            expires_at: Some(tokens.expires_at_ms()),
            status: "active".into(),
            risk_ack_at: Some(risk_acknowledged_at),
            created_at: util::now_ms(),
            last_used_at: None,
        };

        self.db.upsert_account(&account)?;
        tracing::info!(account = %account.id, "cuenta de suscripción de ChatGPT conectada");
        Ok(account)
    }

    /// Guarda una API key de OpenAI como cuenta.
    pub fn connect_openai_api_key(&self, key: &str, label: Option<&str>) -> Result<Account> {
        let key = key.trim();
        if key.is_empty() {
            return Err(CoreError::Config("la API key está vacía".into()));
        }
        let account_id = util::new_id("acc");
        self.secrets.set(&SecretRef::api_key(&account_id), key)?;

        let account = Account {
            id: account_id.clone(),
            provider_id: "openai".into(),
            credential_kind: CredentialKind::ApiKey,
            label: label.unwrap_or("OpenAI (API key)").to_string(),
            keychain_ref: Some(SecretRef::api_key(&account_id).as_str().to_string()),
            external_id: Some(format!("apikey-{}", &util::sha256_hex(key.as_bytes())[..12])),
            scopes: None,
            expires_at: None,
            status: "active".into(),
            risk_ack_at: None,
            created_at: util::now_ms(),
            last_used_at: None,
        };
        self.db.upsert_account(&account)?;
        Ok(account)
    }

    // -- Aplicaciones -------------------------------------------------------

    /// Crea una aplicación y le concede acceso a las vías que ya tienen una
    /// cuenta conectada.
    ///
    /// Una aplicación sin permisos ve un catálogo vacío, y desde el cliente eso
    /// es indistinguible de «Nexo no funciona»: la mayoría de herramientas
    /// muestran «no se encontraron modelos» tanto ante un 401 como ante una
    /// lista vacía. El acceso se sigue pudiendo quitar con un clic, pero el
    /// camino por defecto tiene que llevar a algo que funcione.
    pub fn create_app_with_access(&self, name: &str, notes: Option<&str>) -> Result<IssuedApp> {
        let issued = self.db.create_app(name, notes)?;

        for account in self.db.accounts()? {
            if account.status == "revoked" {
                continue;
            }
            self.db.grant_with_mandatory_limit(
                &issued.app.id,
                &account.provider_id,
                account.credential_kind,
                true,
                true,
                None,
                None,
            )?;
        }

        Ok(issued)
    }

    /// Desconecta una cuenta y elimina sus secretos del equipo.
    pub fn disconnect_account(&self, account_id: &str) -> Result<()> {
        for key in [
            SecretRef::access(account_id),
            SecretRef::refresh(account_id),
            SecretRef::api_key(account_id),
        ] {
            if let Err(e) = self.secrets.delete(&key) {
                tracing::warn!(error = %e, "no se pudo borrar un secreto del almacén");
            }
        }
        self.db.delete_account(account_id)?;
        Ok(())
    }

    /// Resuelve la credencial de una cuenta, renovando si hace falta.
    async fn resolve_credential(
        &self,
        account: &Account,
    ) -> std::result::Result<ResolvedCredential, AdapterError> {
        match account.credential_kind {
            CredentialKind::Mock | CredentialKind::Local => Ok(ResolvedCredential {
                account_id: account.id.clone(),
                kind: account.credential_kind,
                secret: String::new(),
                external_id: None,
            }),
            CredentialKind::ApiKey => {
                let secret = self
                    .secrets
                    .get(&SecretRef::api_key(&account.id))
                    .map_err(|e| AdapterError::Auth {
                        reason: format!("no se pudo leer la API key del almacén seguro: {e}"),
                        reauth_required: true,
                    })?
                    .ok_or_else(|| AdapterError::Auth {
                        reason: "la API key ya no está en el almacén seguro".into(),
                        reauth_required: true,
                    })?;
                Ok(ResolvedCredential {
                    account_id: account.id.clone(),
                    kind: account.credential_kind,
                    secret,
                    external_id: account.external_id.clone(),
                })
            }
            CredentialKind::SubscriptionOauth => self.resolve_oauth(account).await,
        }
    }

    async fn resolve_oauth(
        &self,
        account: &Account,
    ) -> std::result::Result<ResolvedCredential, AdapterError> {
        // Un único refresco concurrente por cuenta.
        let lock = {
            let mut locks = self.refresh_locks.lock().await;
            locks
                .entry(account.id.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;

        // Se relee: otra tarea puede haber renovado mientras esperábamos.
        let current = self
            .db
            .account(&account.id)
            .map_err(|e| AdapterError::Transport { detail: e.to_string() })?
            .ok_or_else(|| AdapterError::Auth {
                reason: "la cuenta ya no existe".into(),
                reauth_required: true,
            })?;

        if !current.is_expired(REFRESH_SKEW_MS) {
            if let Some(secret) = self
                .secrets
                .get(&SecretRef::access(&current.id))
                .map_err(|e| AdapterError::Transport { detail: e.to_string() })?
            {
                return Ok(ResolvedCredential {
                    account_id: current.id.clone(),
                    kind: current.credential_kind,
                    secret,
                    external_id: current.external_id.clone(),
                });
            }
        }

        let refresh_token = self
            .secrets
            .get(&SecretRef::refresh(&current.id))
            .map_err(|e| AdapterError::Transport { detail: e.to_string() })?
            .ok_or_else(|| AdapterError::Auth {
                reason: "no hay refresh token: vuelve a conectar la cuenta de ChatGPT".into(),
                reauth_required: true,
            })?;

        let tokens = chatgpt::refresh(&self.http, &refresh_token)
            .await
            .map_err(|e| {
                let _ = self.db.set_account_status(&current.id, "expired");
                AdapterError::Auth {
                    reason: format!("no se pudo renovar la autorización de ChatGPT: {e}"),
                    reauth_required: true,
                }
            })?;

        self.secrets
            .set(&SecretRef::access(&current.id), &tokens.access_token)
            .map_err(|e| AdapterError::Transport { detail: e.to_string() })?;
        if let Some(new_refresh) = &tokens.refresh_token {
            let _ = self
                .secrets
                .set(&SecretRef::refresh(&current.id), new_refresh);
        }
        let external_id = tokens.account_id().or(current.external_id.clone());
        let _ = self.db.set_account_tokens_meta(
            &current.id,
            tokens.expires_at_ms(),
            external_id.as_deref(),
        );

        Ok(ResolvedCredential {
            account_id: current.id.clone(),
            kind: current.credential_kind,
            secret: tokens.access_token,
            external_id,
        })
    }

    // -- Catálogo por aplicación -------------------------------------------

    /// Modelos que una aplicación concreta puede usar, con la vía anotada.
    pub fn models_for_app(&self, app_id: &str) -> Result<Vec<Value>> {
        let grants = self.db.grants(app_id)?;
        let accounts = self.db.accounts()?;
        let rows = self.db.catalog_rows()?;

        let mut out = Vec::new();
        for row in rows {
            let permitted = grants.iter().any(|g| {
                g.provider_id == row.provider_id && g.credential_kind == row.credential_kind
            });
            if !permitted {
                continue;
            }
            let kind = CredentialKind::parse(&row.credential_kind)
                .unwrap_or(CredentialKind::ApiKey);
            let connected = kind == CredentialKind::Mock
                || accounts.iter().any(|a| {
                    a.provider_id == row.provider_id
                        && a.credential_kind == kind
                        && a.status != "revoked"
                });
            if !connected {
                continue;
            }

            out.push(json!({
                "id": row.public_name,
                "object": "model",
                "owned_by": row.provider_id,
                "created": 0,
                // Bloque propio: la compatibilidad de formato no es
                // equivalencia de capacidades, así que se explicita.
                "nexo": {
                    "provider": row.provider_id,
                    "credential_kind": row.credential_kind,
                    "accounting": row.accounting,
                    "api_id": row.api_id,
                    "context_max": row.context_max,
                    "output_max": row.output_max,
                    "capabilities": row.caps,
                    "priced": row.price_input.is_some(),
                },
            }));
        }
        Ok(out)
    }

    // -- Ciclo de una petición --------------------------------------------

    /// Resuelve modelo, permisos, límites y credencial. No envía nada todavía.
    ///
    /// Un rechazo también se registra: si no, un límite aplicado o un permiso
    /// denegado serían invisibles en el panel, que es justo donde el usuario
    /// va a buscar por qué su herramienta ha dejado de funcionar.
    pub async fn prepare(
        &self,
        app_id: &str,
        wire: WireChatRequest,
    ) -> std::result::Result<Prepared, AdapterError> {
        let requested = wire.model.clone();
        match self.prepare_inner(app_id, wire).await {
            Ok(prepared) => Ok(prepared),
            Err(err) => {
                self.record_rejection(app_id, &requested, &err);
                Err(err)
            }
        }
    }

    /// Registra un rechazo previo a la ejecución, con lo que se sepa del
    /// destino. Un modelo no reconocido se anota como `unknown`.
    fn record_rejection(&self, app_id: &str, requested_model: &str, err: &AdapterError) {
        let resolved = self.db.resolve_model(requested_model, None).ok().flatten();
        let (provider_id, kind, public_model, api_model) = match &resolved {
            Some(r) => (
                r.provider_id.clone(),
                r.credential_kind.as_str().to_string(),
                r.public_name.clone(),
                r.api_id.clone(),
            ),
            None => (
                "unknown".to_string(),
                "unknown".to_string(),
                requested_model.to_string(),
                requested_model.to_string(),
            ),
        };

        let event = crate::db::stats::RequestEvent {
            id: util::new_id("req"),
            ts: util::now_ms(),
            app_id: app_id.to_string(),
            provider_id,
            credential_kind: kind,
            account_id: None,
            public_model,
            api_model,
            operation: "chat".into(),
            streamed: false,
            status: crate::db::stats::RequestStatus::Error,
            error_kind: Some(err.kind_str().to_string()),
            http_status: Some(err.http_status()),
            latency_ms: Some(0),
            ttft_ms: None,
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            reasoning_tokens: None,
            usage_source: UsageSource::Unavailable,
            cost_micros: None,
            cost_basis: CostBasis::Unavailable,
            fallback_from: None,
            provider_usage_raw: None,
            provider_request_id: None,
        };

        if let Err(e) = self.db.record_request(&event) {
            tracing::error!(error = %e, "no se pudo registrar el rechazo");
        }
    }

    async fn prepare_inner(
        &self,
        app_id: &str,
        wire: WireChatRequest,
    ) -> std::result::Result<Prepared, AdapterError> {
        let requested = wire.model.clone();
        let resolved = self
            .db
            .resolve_model(&requested, None)
            .map_err(|e| AdapterError::Transport { detail: e.to_string() })?
            .ok_or_else(|| AdapterError::Unsupported {
                capability: "model".into(),
                hint: Some(format!(
                    "«{requested}» no está en el catálogo. Consulta GET /v1/models."
                )),
            })?;

        let req = wire
            .into_internal(resolved.api_id.clone(), resolved.public_name.clone())
            .map_err(|e| AdapterError::Unsupported {
                capability: "request".into(),
                hint: Some(e),
            })?;

        self.policy.check(
            app_id,
            &resolved.provider_id,
            resolved.credential_kind,
            &resolved.public_name,
            &req,
        )?;

        let (account, cred) = self.credential_for(&resolved).await?;

        // Se cuenta al admitir, no al terminar: si no, varias peticiones
        // concurrentes pasarían todas el mismo control.
        self.policy
            .record(app_id, &resolved.provider_id, resolved.credential_kind);

        Ok(Prepared {
            app_id: app_id.to_string(),
            adapter_slug: AdapterId::new(
                resolved.provider_id.clone(),
                resolved.credential_kind,
            )
            .slug(),
            accounting: resolved.accounting_enum(),
            account_id: account.map(|a| a.id),
            resolved,
            req,
            cred,
            started: Instant::now(),
            started_ms: util::now_ms(),
            fallback_from: None,
        })
    }

    async fn credential_for(
        &self,
        resolved: &ResolvedModel,
    ) -> std::result::Result<(Option<Account>, ResolvedCredential), AdapterError> {
        if resolved.credential_kind == CredentialKind::Mock {
            return Ok((
                None,
                ResolvedCredential {
                    account_id: "mock".into(),
                    kind: CredentialKind::Mock,
                    secret: String::new(),
                    external_id: None,
                },
            ));
        }

        let account = self
            .db
            .account_for(&resolved.provider_id, resolved.credential_kind)
            .map_err(|e| AdapterError::Transport { detail: e.to_string() })?
            .ok_or_else(|| AdapterError::Auth {
                reason: format!(
                    "no hay ninguna cuenta de {} conectada por la vía {}. Conéctala en Nexo.",
                    resolved.provider_id,
                    resolved.credential_kind.as_str()
                ),
                reauth_required: true,
            })?;

        let cred = self.resolve_credential(&account).await?;
        Ok((Some(account), cred))
    }

    /// Abre el stream del adaptador, con respaldo a API key si la vía de
    /// suscripción está rota.
    pub async fn open_stream(
        &self,
        prepared: &Prepared,
    ) -> std::result::Result<EventStream, AdapterError> {
        let adapter = self
            .adapters
            .get(&prepared.adapter_slug)
            .ok_or_else(|| AdapterError::Transport {
                detail: format!("no hay adaptador para {}", prepared.adapter_slug),
            })?;

        match adapter.stream(&prepared.req, &prepared.cred).await {
            Ok(stream) => Ok(stream),
            Err(err) => {
                if !prepared.can_fall_back() || !is_fallback_worthy(&err) {
                    return Err(err);
                }
                tracing::warn!(
                    error = %err,
                    "la vía de suscripción falló; se intenta el respaldo por API key"
                );
                if let Some(account) = self.db.account_for("openai", CredentialKind::ApiKey).ok().flatten() {
                    let _ = self.db.set_account_status(
                        prepared.account_id.as_deref().unwrap_or_default(),
                        "broken",
                    );
                    let cred = self.resolve_credential(&account).await?;
                    let slug = AdapterId::new("openai", CredentialKind::ApiKey).slug();
                    if let Some(fallback) = self.adapters.get(&slug) {
                        return fallback.stream(&prepared.req, &cred).await;
                    }
                }
                Err(err)
            }
        }
    }

    /// Registra el resultado de una petición completada.
    pub fn finish(&self, prepared: &Prepared, collector: &Collector) {
        let usage = collector.usage();
        let basis = prepared.accounting.cost_basis_for(usage.source);
        let cost = if basis == CostBasis::Estimated {
            catalog::openai_apikey_models()
                .into_iter()
                .find(|m| m.api_id == prepared.resolved.api_id)
                .and_then(|m| m.pricing)
                .and_then(|p| p.cost_micros(&usage))
        } else {
            None
        };

        let status = if collector.error_kind().is_some() {
            crate::db::stats::RequestStatus::Error
        } else if collector.finish_reason() == Some(FinishReason::Cancelled) {
            crate::db::stats::RequestStatus::Cancelled
        } else {
            crate::db::stats::RequestStatus::Ok
        };

        let event = crate::db::stats::RequestEvent {
            id: util::new_id("req"),
            ts: prepared.started_ms,
            app_id: prepared.app_id.clone(),
            provider_id: prepared.resolved.provider_id.clone(),
            credential_kind: prepared.resolved.credential_kind.as_str().to_string(),
            account_id: prepared.account_id.clone(),
            public_model: prepared.resolved.public_name.clone(),
            api_model: prepared.resolved.api_id.clone(),
            operation: "chat".into(),
            streamed: prepared.req.stream,
            status,
            error_kind: collector.error_kind().map(str::to_string),
            http_status: collector.http_status(),
            latency_ms: Some(prepared.started.elapsed().as_millis() as i64),
            ttft_ms: collector.ttft_ms(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            usage_source: usage.source,
            cost_micros: cost,
            cost_basis: basis,
            fallback_from: prepared.fallback_from.clone(),
            provider_usage_raw: usage.raw.clone(),
            provider_request_id: collector.provider_request_id(),
        };

        if let Err(e) = self.db.record_request(&event) {
            tracing::error!(error = %e, "no se pudo registrar la petición");
        }
    }

    /// Registra un fallo que impidió abrir el stream.
    pub fn finish_failed(&self, prepared: &Prepared, err: &AdapterError) {
        let mut collector = Collector::new();
        collector.observe_error(err);
        self.finish(prepared, &collector);
    }

    // -- Estado -------------------------------------------------------------

    pub fn status(&self, settings: &Settings) -> Result<GatewayStatus> {
        let accounts = self.db.accounts()?;
        Ok(GatewayStatus {
            paused: self.is_paused(),
            bind_error: self.bind_error(),
            port: settings.port,
            base_url: format!("http://127.0.0.1:{}/v1", settings.port),
            accounts: accounts.len(),
            subscription_connected: accounts.iter().any(|a| {
                a.credential_kind == CredentialKind::SubscriptionOauth && a.status == "active"
            }),
            api_key_connected: accounts
                .iter()
                .any(|a| a.credential_kind == CredentialKind::ApiKey && a.status == "active"),
            broken_accounts: accounts.iter().filter(|a| a.status == "broken").count(),
            apps: self.db.apps()?.iter().filter(|a| a.revoked_at.is_none()).count(),
            apps_missing_limits: self.db.apps_missing_mandatory_limits()?,
            manifest_version: catalog::MANIFEST_VERSION.to_string(),
        })
    }

    pub fn active_apps(&self) -> Result<Vec<App>> {
        Ok(self
            .db
            .apps()?
            .into_iter()
            .filter(|a| a.revoked_at.is_none())
            .collect())
    }
}

fn chatgpt_subscription_provider() -> &'static str {
    crate::provider::chatgpt_subscription::PROVIDER
}

/// Solo se cae al respaldo cuando el fallo es de la ruta, no del cliente.
fn is_fallback_worthy(err: &AdapterError) -> bool {
    matches!(
        err,
        AdapterError::SubscriptionPathBroken { .. }
            | AdapterError::Auth { .. }
            | AdapterError::Malformed { .. }
    )
}

pub fn default_db_path() -> PathBuf {
    let base = std::env::var_os("NEXO_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                PathBuf::from(h)
                    .join("Library")
                    .join("Application Support")
                    .join("Nexo")
            })
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("nexo.sqlite")
}

/// Todo lo necesario para ejecutar y registrar una petición.
#[derive(Debug)]
pub struct Prepared {
    pub app_id: String,
    pub adapter_slug: String,
    pub accounting: Accounting,
    pub account_id: Option<String>,
    pub resolved: ResolvedModel,
    pub req: ChatRequest,
    pub cred: ResolvedCredential,
    pub started: Instant,
    pub started_ms: i64,
    pub fallback_from: Option<String>,
}

impl Prepared {
    pub fn public_model(&self) -> &str {
        &self.resolved.public_name
    }

    fn can_fall_back(&self) -> bool {
        self.resolved.credential_kind == CredentialKind::SubscriptionOauth
    }
}

impl Prepared {
    /// Alias legible desde el gateway.
    pub fn public_model_owned(&self) -> String {
        self.resolved.public_name.clone()
    }
}

/// Acumula lo que hace falta para registrar el evento y cerrar el stream.
#[derive(Default)]
pub struct Collector {
    started: Option<Instant>,
    ttft_ms: Option<i64>,
    usage: Option<UsageReport>,
    finish_reason: Option<FinishReason>,
    error_kind: Option<String>,
    http_status: Option<u16>,
    provider_request_id: Option<String>,
    closed: bool,
}

impl Collector {
    pub fn new() -> Self {
        Self { started: Some(Instant::now()), ..Default::default() }
    }

    pub fn observe(&mut self, event: &ChatEvent) {
        match event {
            ChatEvent::Started { provider_request_id } => {
                self.provider_request_id = provider_request_id.clone();
                self.started = Some(Instant::now());
            }
            ChatEvent::TextDelta { .. } | ChatEvent::ReasoningDelta { .. } => {
                if self.ttft_ms.is_none() {
                    self.ttft_ms = self
                        .started
                        .map(|s| s.elapsed().as_millis() as i64);
                }
            }
            ChatEvent::Usage(u) => self.usage = Some(u.clone()),
            ChatEvent::Finished { reason } => {
                self.finish_reason = Some(*reason);
                self.closed = true;
            }
            _ => {}
        }
    }

    pub fn observe_error(&mut self, err: &AdapterError) {
        self.error_kind = Some(err.kind_str().to_string());
        self.http_status = Some(err.http_status());
        self.finish_reason = Some(match err {
            AdapterError::Cancelled => FinishReason::Cancelled,
            _ => FinishReason::Error,
        });
        self.closed = true;
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Nunca inventa cifras: si el proveedor no informó, queda no disponible.
    pub fn usage(&self) -> UsageReport {
        self.usage.clone().unwrap_or_else(|| UsageReport {
            source: UsageSource::Unavailable,
            ..Default::default()
        })
    }

    pub fn ttft_ms(&self) -> Option<i64> {
        self.ttft_ms
    }

    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.finish_reason
    }

    pub fn error_kind(&self) -> Option<&str> {
        self.error_kind.as_deref()
    }

    pub fn http_status(&self) -> Option<u16> {
        self.http_status
    }

    pub fn provider_request_id(&self) -> Option<String> {
        self.provider_request_id.clone()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GatewayStatus {
    pub paused: bool,
    /// Presente cuando el gateway no pudo reservar el puerto.
    pub bind_error: Option<String>,
    pub port: u16,
    pub base_url: String,
    pub accounts: usize,
    pub subscription_connected: bool,
    pub api_key_connected: bool,
    pub broken_accounts: usize,
    pub apps: usize,
    /// Incumplimientos de la invariante del ADR 0001.
    pub apps_missing_limits: Vec<String>,
    pub manifest_version: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecretStore;

    fn nexo() -> Arc<Nexo> {
        Nexo::new(
            Db::open_in_memory().unwrap(),
            Arc::new(MemorySecretStore::default()),
        )
        .unwrap()
    }

    #[test]
    fn catalog_is_populated_on_start_for_both_routes() {
        let n = nexo();
        let rows = n.db().catalog_rows().unwrap();
        assert!(rows.iter().any(|r| r.credential_kind == "subscription_oauth"));
        assert!(rows.iter().any(|r| r.credential_kind == "api_key"));
        assert!(rows.iter().any(|r| r.credential_kind == "mock"));
    }

    #[test]
    fn models_for_app_hides_routes_without_grant() {
        let n = nexo();
        let app = n.db().create_app("cliente", None).unwrap();
        assert!(n.models_for_app(&app.app.id).unwrap().is_empty());

        n.db()
            .grant_with_mandatory_limit(
                &app.app.id,
                "mock",
                CredentialKind::Mock,
                false,
                false,
                None,
                None,
            )
            .unwrap();
        let models = n.models_for_app(&app.app.id).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["nexo"]["credential_kind"], "mock");
    }

    #[test]
    fn models_for_app_hides_routes_without_a_connected_account() {
        let n = nexo();
        let app = n.db().create_app("cliente", None).unwrap();
        n.db()
            .grant_with_mandatory_limit(
                &app.app.id,
                "openai",
                CredentialKind::ApiKey,
                false,
                false,
                None,
                None,
            )
            .unwrap();
        assert!(
            n.models_for_app(&app.app.id).unwrap().is_empty(),
            "sin cuenta conectada no se anuncia el modelo"
        );

        n.connect_openai_api_key("sk-test-123", None).unwrap();
        assert!(!n.models_for_app(&app.app.id).unwrap().is_empty());
    }

    #[test]
    fn subscription_models_are_announced_as_unpriced() {
        let n = nexo();
        let app = n.db().create_app("cliente", None).unwrap();
        n.db()
            .grant_with_mandatory_limit(
                &app.app.id,
                "openai",
                CredentialKind::SubscriptionOauth,
                false,
                false,
                None,
                None,
            )
            .unwrap();
        n.db()
            .upsert_account(&Account {
                id: "acc-sub".into(),
                provider_id: "openai".into(),
                credential_kind: CredentialKind::SubscriptionOauth,
                label: "ChatGPT".into(),
                keychain_ref: None,
                external_id: Some("ext".into()),
                scopes: None,
                expires_at: None,
                status: "active".into(),
                risk_ack_at: Some(1),
                created_at: 0,
                last_used_at: None,
            })
            .unwrap();

        let models = n.models_for_app(&app.app.id).unwrap();
        assert!(!models.is_empty());
        for m in models {
            assert_eq!(m["nexo"]["accounting"], "subscription");
            assert_eq!(m["nexo"]["priced"], false);
        }
    }

    #[tokio::test]
    async fn connecting_a_subscription_without_risk_ack_is_refused() {
        let n = nexo();
        let err = n
            .connect_chatgpt_subscription(0, |_| Ok(()))
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Forbidden(_)));
    }

    #[test]
    fn empty_api_key_is_refused() {
        assert!(nexo().connect_openai_api_key("   ", None).is_err());
    }

    #[test]
    fn disconnect_removes_account_and_secrets() {
        let n = nexo();
        let account = n.connect_openai_api_key("sk-test", None).unwrap();
        assert!(n
            .secrets()
            .get(&SecretRef::api_key(&account.id))
            .unwrap()
            .is_some());

        n.disconnect_account(&account.id).unwrap();
        assert!(n.db().account(&account.id).unwrap().is_none());
        assert!(
            n.secrets()
                .get(&SecretRef::api_key(&account.id))
                .unwrap()
                .is_none(),
            "el secreto debe desaparecer del almacén"
        );
    }

    #[tokio::test]
    async fn prepare_rejects_unknown_model() {
        let n = nexo();
        let app = n.db().create_app("cliente", None).unwrap();
        let wire: WireChatRequest = serde_json::from_value(json!({
            "model": "no-existe",
            "messages": [{"role": "user", "content": "hola"}]
        }))
        .unwrap();
        let err = n.prepare(&app.app.id, wire).await.unwrap_err();
        assert_eq!(err.http_status(), 422);
    }

    #[tokio::test]
    async fn prepare_rejects_model_without_grant() {
        let n = nexo();
        let app = n.db().create_app("cliente", None).unwrap();
        let wire: WireChatRequest = serde_json::from_value(json!({
            "model": "mock/mock-echo",
            "messages": [{"role": "user", "content": "hola"}]
        }))
        .unwrap();
        let err = n.prepare(&app.app.id, wire).await.unwrap_err();
        assert_eq!(err.http_status(), 401);
    }

    #[tokio::test]
    async fn end_to_end_through_the_mock_records_a_request() {
        let n = nexo();
        let app = n.db().create_app("cliente", None).unwrap();
        n.db()
            .grant_with_mandatory_limit(
                &app.app.id,
                "mock",
                CredentialKind::Mock,
                false,
                false,
                None,
                None,
            )
            .unwrap();

        let wire: WireChatRequest = serde_json::from_value(json!({
            "model": "mock/mock-echo",
            "messages": [{"role": "user", "content": "hola mundo"}],
            "stream": true
        }))
        .unwrap();

        let prepared = n.prepare(&app.app.id, wire).await.expect("prepared");
        let mut stream = n.open_stream(&prepared).await.expect("stream");
        let mut collector = Collector::new();
        let mut text = String::new();
        while let Some(ev) = futures::StreamExt::next(&mut stream).await {
            let ev = ev.expect("evento");
            collector.observe(&ev);
            if let ChatEvent::TextDelta { text: t } = &ev {
                text.push_str(t);
            }
        }
        assert_eq!(text, "eco: hola mundo");
        n.finish(&prepared, &collector);

        let recent = n.db().recent_requests(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].status, "ok");
        assert_eq!(recent[0].credential_kind, "mock");
        assert!(recent[0].ttft_ms.is_some());
    }

    #[tokio::test]
    async fn subscription_route_consumes_its_limit() {
        let n = nexo();
        let app = n.db().create_app("cliente", None).unwrap();
        n.db()
            .grant_with_mandatory_limit(
                &app.app.id,
                "mock",
                CredentialKind::Mock,
                false,
                false,
                Some(1),
                Some(60),
            )
            .unwrap();
        // El mock no exige límite, así que se añade uno a mano para probar el
        // consumo del contador.
        n.db()
            .set_limit(
                &app.app.id,
                &crate::apps::Limit {
                    provider_id: "mock".into(),
                    credential_kind: "mock".into(),
                    window_seconds: 60,
                    max_requests: Some(1),
                    max_input_tokens: None,
                    max_output_tokens: None,
                },
            )
            .unwrap();

        let make = || {
            serde_json::from_value::<WireChatRequest>(json!({
                "model": "mock/mock-echo",
                "messages": [{"role": "user", "content": "hola"}]
            }))
            .unwrap()
        };

        assert!(n.prepare(&app.app.id, make()).await.is_ok());
        let err = n.prepare(&app.app.id, make()).await.unwrap_err();
        assert_eq!(err.kind_str(), "local_limit");
    }

    #[test]
    fn status_reports_the_invariant_breach() {
        let n = nexo();
        let app = n.db().create_app("cliente", None).unwrap();
        n.db()
            .set_grant(
                &app.app.id,
                &crate::apps::Grant {
                    provider_id: "openai".into(),
                    credential_kind: "subscription_oauth".into(),
                    model_pattern: "*".into(),
                    allow_tools: false,
                    allow_multimodal: false,
                    log_content: false,
                },
            )
            .unwrap();
        let status = n.status(&Settings::default()).unwrap();
        assert_eq!(status.apps_missing_limits, vec![app.app.id]);
        assert!(!status.subscription_connected);
    }

    #[test]
    fn fallback_only_triggers_on_route_failures() {
        assert!(is_fallback_worthy(&AdapterError::SubscriptionPathBroken {
            provider: "openai".into(),
            detail: String::new(),
        }));
        assert!(is_fallback_worthy(&AdapterError::Malformed {
            detail: String::new()
        }));
        assert!(!is_fallback_worthy(&AdapterError::Unsupported {
            capability: "vision".into(),
            hint: None
        }));
        assert!(!is_fallback_worthy(&AdapterError::RateLimited {
            retry_after: None
        }));
        assert!(!is_fallback_worthy(&AdapterError::LocalLimit {
            app_id: "a".into(),
            window_secs: 1,
            detail: String::new()
        }));
    }

    #[test]
    fn collector_never_invents_usage() {
        let c = Collector::new();
        let u = c.usage();
        assert_eq!(u.source, UsageSource::Unavailable);
        assert_eq!(u.input_tokens, None);
        assert_eq!(u.total_tokens(), None);
    }

    #[test]
    fn collector_measures_ttft_on_first_delta_only() {
        let mut c = Collector::new();
        c.observe(&ChatEvent::Started { provider_request_id: Some("r".into()) });
        assert_eq!(c.ttft_ms(), None);
        c.observe(&ChatEvent::TextDelta { text: "a".into() });
        let first = c.ttft_ms();
        assert!(first.is_some());
        c.observe(&ChatEvent::TextDelta { text: "b".into() });
        assert_eq!(c.ttft_ms(), first);
        assert_eq!(c.provider_request_id().as_deref(), Some("r"));
    }

    #[test]
    fn collector_closes_on_error_with_its_kind() {
        let mut c = Collector::new();
        assert!(!c.is_closed());
        c.observe_error(&AdapterError::RateLimited { retry_after: None });
        assert!(c.is_closed());
        assert_eq!(c.error_kind(), Some("rate_limited"));
        assert_eq!(c.http_status(), Some(429));
    }
}
