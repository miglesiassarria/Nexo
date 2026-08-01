//! El servicio Nexo: orquesta credenciales, enrutado, políticas y métricas.

use crate::apps::{App, IssuedApp};
use crate::auth::{self, chatgpt};
use crate::catalog;
use crate::catalog::models_dev::{self, ModelsDevCatalog};
use crate::config::Settings;
use crate::db::{Account, CustomProvider, Db, ResolvedModel};
use crate::error::{CoreError, Result};
use crate::gateway::wire::WireChatRequest;
use crate::policy::PolicyEngine;
use crate::provider::{
    chatgpt_subscription::ChatgptSubscriptionAdapter, lmstudio, lmstudio::LmStudioAdapter,
    mock::MockAdapter, openai_apikey::OpenAiApiKeyAdapter, openai_compat,
    openai_compat::OpenAiCompatAdapter,
    Accounting, AdapterError, AdapterId, ChatEvent,
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
    /// Instancia única y compartida: sirve a todos los proveedores que el usuario
    /// añada (nombre, URL, clave), y también a los preajustes como OpenCode Zen.
    /// Sin estado por proveedor, así que no hace falta una por cada uno.
    custom_adapter: Arc<OpenAiCompatAdapter>,
    /// Capacidades y precios de terceros para el catálogo de los proveedores
    /// añadidos. Vacío al arrancar; `refresh_models_dev` lo rellena en segundo
    /// plano sin bloquear (T2 de la especificación 0002).
    models_dev: Arc<tokio::sync::RwLock<ModelsDevCatalog>>,
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

        let settings = db.settings().unwrap_or_default();
        let client_version = settings.codex_client_version.clone();
        let lmstudio_url = settings.lmstudio_base_url.clone();

        // La retención existía como botón en Configuración, pero nadie la
        // pulsaba: sin esto, `requests` crece sin límite mientras la app esté
        // instalada. Se aplica en cada arranque, con la configuración vigente.
        // El botón se conserva para el caso de bajar la retención y querer
        // que surta efecto ya, sin esperar al siguiente arranque.
        if let Err(e) =
            db.apply_retention(settings.retention_days, settings.content_retention_days)
        {
            tracing::warn!(error = %e, "no se pudo aplicar la retención al arrancar");
        }

        let mut adapters: HashMap<String, Arc<dyn ProviderAdapter>> = HashMap::new();
        for adapter in [
            Arc::new(ChatgptSubscriptionAdapter::with_client_version(
                http.clone(),
                client_version,
            )) as Arc<dyn ProviderAdapter>,
            Arc::new(OpenAiApiKeyAdapter::new(http.clone())),
            Arc::new(LmStudioAdapter::new(http.clone(), lmstudio_url)),
            Arc::new(MockAdapter::default()),
        ] {
            adapters.insert(adapter.id().slug(), adapter);
        }

        let policy = PolicyEngine::new(db.clone());
        let models_dev = Arc::new(tokio::sync::RwLock::new(ModelsDevCatalog::default()));
        let custom_adapter = Arc::new(OpenAiCompatAdapter::new(http.clone(), models_dev.clone()));

        let nexo = Arc::new(Self {
            db,
            secrets,
            http,
            adapters,
            custom_adapter,
            models_dev,
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

    /// Resuelve el adaptador de un proveedor, con respaldo al genérico.
    ///
    /// Primero mira los proveedores integrados (mapa fijo, construido al
    /// arrancar). Si no hay uno, y `provider_id` está en `custom_providers`, cae al
    /// adaptador OpenAI-compatible compartido: es lo que permite que un proveedor
    /// con nombre elegido por el usuario nunca esté en el mapa fijo y funcione
    /// igual (D1 del diseño de la especificación 0002).
    fn adapter_for(&self, provider_id: &str, kind: CredentialKind) -> Option<Arc<dyn ProviderAdapter>> {
        let slug = AdapterId::new(provider_id, kind).slug();
        if let Some(adapter) = self.adapters.get(&slug) {
            return Some(adapter.clone());
        }
        if kind == CredentialKind::ApiKey {
            if let Ok(Some(_)) = self.db.custom_provider(provider_id) {
                return Some(self.custom_adapter.clone() as Arc<dyn ProviderAdapter>);
            }
        }
        None
    }

    // -- Proveedores añadidos por el usuario ---------------------------------

    /// Añade un proveedor OpenAI-compatible: nombre, dirección y clave. Crea a la
    /// vez su cuenta (la clave va al Keychain) y refresca su catálogo.
    pub async fn add_custom_provider(
        &self,
        name: &str,
        base_url: &str,
        api_key: &str,
    ) -> Result<CustomProvider> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(CoreError::Config("el proveedor necesita una API key".into()));
        }

        let provider = self.db.create_custom_provider(name, base_url)?;

        // La clave se guarda bajo el id de la CUENTA, no el del proveedor: es el
        // mismo convenio que usa `connect_openai_api_key`, y es lo que
        // `disconnect_account` sabe borrar. Guardarla bajo otro id la habría
        // dejado huérfana para siempre al desconectar (lo encontró esta prueba).
        let account_id = util::new_id("acc");
        self.secrets.set(&SecretRef::api_key(&account_id), api_key)?;
        let account = Account {
            id: account_id.clone(),
            provider_id: provider.id.clone(),
            credential_kind: CredentialKind::ApiKey,
            label: provider.name.clone(),
            keychain_ref: Some(SecretRef::api_key(&account_id).as_str().to_string()),
            external_id: Some(provider.base_url.clone()),
            scopes: None,
            expires_at: None,
            status: "active".into(),
            risk_ack_at: None,
            created_at: util::now_ms(),
            last_used_at: None,
        };
        if let Err(e) = self.db.upsert_account(&account) {
            // No dejar un proveedor sin cuenta ni secreto huérfano.
            let _ = self.secrets.delete(&SecretRef::api_key(&account_id));
            let _ = self.db.delete_custom_provider(&provider.id);
            return Err(e);
        }

        for result in self.refresh_catalog_from_providers().await {
            if result.provider_id == provider.id {
                if let Some(error) = &result.error {
                    tracing::warn!(provider = %provider.id, %error, "el catálogo no se pudo descubrir al añadir el proveedor");
                }
            }
        }

        Ok(provider)
    }

    /// Cambia la dirección de un proveedor añadido. Actualiza también la cuenta,
    /// que es de donde la lee el adaptador en cada petición (sin reinicio).
    pub async fn update_custom_provider_url(&self, id: &str, base_url: &str) -> Result<()> {
        self.db.update_custom_provider_url(id, base_url)?;
        // Se relee la dirección ya normalizada (sin barra final) en lugar de
        // reutilizar el parámetro crudo: si no, la cuenta —que es de donde el
        // adaptador la lee de verdad— se quedaba con la versión sin normalizar.
        let normalized = self
            .db
            .custom_provider(id)?
            .map(|p| p.base_url)
            .unwrap_or_else(|| base_url.to_string());
        if let Some(account) = self.db.account_for(id, CredentialKind::ApiKey)? {
            self.db.set_account_tokens_meta(
                &account.id,
                account.expires_at.unwrap_or(0),
                Some(&normalized),
            )?;
        }
        for result in self.refresh_catalog_from_providers().await {
            if result.provider_id == id {
                if let Some(error) = &result.error {
                    tracing::warn!(provider = id, %error, "el catálogo no se pudo refrescar tras cambiar la dirección");
                }
            }
        }
        Ok(())
    }

    pub fn custom_providers(&self) -> Result<Vec<CustomProvider>> {
        self.db.custom_providers()
    }

    /// Borra el proveedor, su cuenta y su clave del Keychain. No borra las filas
    /// de catálogo ya descubiertas: quedan huérfanas y simplemente no se ofrecen a
    /// ninguna aplicación, igual que al desconectar cualquier otra cuenta.
    pub fn remove_custom_provider(&self, id: &str) -> Result<()> {
        if let Some(account) = self.db.account_for(id, CredentialKind::ApiKey)? {
            self.disconnect_account(&account.id)?;
        }
        self.db.delete_custom_provider(id)
    }

    /// Descarga (o cachea) `models.dev` y lo deja listo para que el adaptador
    /// genérico enriquezca sus catálogos. Pensado para llamarse en segundo plano
    /// al arrancar, sin bloquear: si falla, el catálogo sigue siendo solo texto.
    pub async fn refresh_models_dev(&self) -> usize {
        let cache_path = models_dev::default_cache_path(default_db_path().parent().unwrap_or(std::path::Path::new(".")));
        let catalog = models_dev::load(&self.http, &cache_path).await;
        let count = catalog.provider_count();
        *self.models_dev.write().await = catalog;
        count
    }

    /// Pregunta a cada proveedor conectado qué modelos ofrece realmente y
    /// reemplaza su parte del catálogo.
    ///
    /// El manifiesto local solo es el punto de partida: cuando el proveedor
    /// publica su catálogo, gana él. Así aparecen familias nuevas sin esperar a
    /// una versión de Nexo.
    ///
    /// Devuelve, por vía, cuántos modelos se descubrieron o el motivo del fallo.
    pub async fn refresh_catalog_from_providers(&self) -> Vec<CatalogRefresh> {
        let mut out = Vec::new();

        let accounts = match self.db.accounts() {
            Ok(a) => a,
            Err(e) => {
                return vec![CatalogRefresh {
                    provider_id: "-".into(),
                    credential_kind: "-".into(),
                    discovered: 0,
                    error: Some(e.to_string()),
                }]
            }
        };

        for account in accounts {
            if account.status == "revoked" {
                continue;
            }
            let Some(adapter) = self.adapter_for(&account.provider_id, account.credential_kind) else {
                continue;
            };

            let mut entry = CatalogRefresh {
                provider_id: account.provider_id.clone(),
                credential_kind: account.credential_kind.as_str().to_string(),
                discovered: 0,
                error: None,
            };

            match self.resolve_credential(&account).await {
                Err(e) => entry.error = Some(e.to_string()),
                Ok(cred) => match adapter.catalog(&cred).await {
                    Err(e) => entry.error = Some(e.to_string()),
                    Ok(models) => {
                        entry.discovered = models.len();
                        if let Err(e) = self.db.replace_models(
                            &account.provider_id,
                            account.credential_kind,
                            &models,
                            &format!("descubierto {}", util::now_ms()),
                        ) {
                            entry.error = Some(e.to_string());
                        } else {
                            tracing::info!(
                                provider = %account.provider_id,
                                kind = account.credential_kind.as_str(),
                                count = models.len(),
                                "catálogo actualizado desde el proveedor"
                            );
                        }
                    }
                },
            }

            out.push(entry);
        }

        out
    }

    // -- Proveedores locales -------------------------------------------------

    /// Busca LM Studio en la dirección configurada y, si es él, lo deja conectado.
    ///
    /// Confirma la forma de su respuesta, no solo que algo contesta: el puerto por
    /// defecto lo usa más de un programa y dar por bueno cualquier `200` acabaría
    /// ofreciendo el catálogo de otro producto.
    pub async fn detect_lmstudio(&self) -> Result<lmstudio::LmStudioStatus> {
        let base_url = self
            .db
            .settings()
            .map(|s| s.lmstudio_base_url)
            .unwrap_or_else(|_| lmstudio::DEFAULT_BASE_URL.to_string());

        let status = lmstudio::probe(&self.http, &base_url).await;

        if !status.reachable {
            // No se borra la cuenta: que LM Studio esté cerrado ahora no significa
            // que el usuario ya no lo use. Se marca y la interfaz lo explica.
            if let Some(account) = self
                .db
                .account_for(lmstudio::PROVIDER, CredentialKind::Local)?
            {
                let _ = self.db.set_account_status(&account.id, "expired");
            }
            return Ok(status);
        }

        let account = Account {
            id: util::new_id("acc"),
            provider_id: lmstudio::PROVIDER.to_string(),
            credential_kind: CredentialKind::Local,
            label: format!("LM Studio ({})", status.base_url),
            // Sin credencial: no hay nada que guardar en el almacén seguro.
            keychain_ref: None,
            external_id: Some(status.base_url.clone()),
            scopes: None,
            expires_at: None,
            status: "active".into(),
            risk_ack_at: None,
            created_at: util::now_ms(),
            last_used_at: None,
        };
        self.db.upsert_account(&account)?;

        for result in self.refresh_catalog_from_providers().await {
            if result.provider_id == lmstudio::PROVIDER {
                if let Some(error) = &result.error {
                    tracing::warn!(%error, "LM Studio responde pero su catálogo falló");
                }
            }
        }

        tracing::info!(
            base_url = %status.base_url,
            models = status.models,
            loaded = status.loaded,
            "LM Studio detectado"
        );
        Ok(status)
    }

    /// Cambia la dirección de LM Studio y vuelve a detectarlo.
    ///
    /// Reconstruir el adaptador exige rearrancar, así que se guarda el ajuste y se
    /// avisa. Mentir diciendo que ya está aplicado sería peor.
    pub async fn set_lmstudio_url(&self, base_url: &str) -> Result<lmstudio::LmStudioStatus> {
        let mut settings = self.db.settings()?;
        settings.lmstudio_base_url = base_url.trim().to_string();
        self.db.save_settings(&settings)?;
        self.detect_lmstudio().await
    }

    /// Estado actual, sin tocar la configuración.
    pub async fn lmstudio_status(&self) -> lmstudio::LmStudioStatus {
        let base_url = self
            .db
            .settings()
            .map(|s| s.lmstudio_base_url)
            .unwrap_or_else(|_| lmstudio::DEFAULT_BASE_URL.to_string());
        lmstudio::probe(&self.http, &base_url).await
    }

    /// Detalles de presentación de los modelos locales: cuantización y carga.
    pub async fn lmstudio_model_details(&self) -> Vec<lmstudio::LocalModelDetail> {
        let base_url = self
            .db
            .settings()
            .map(|s| s.lmstudio_base_url)
            .unwrap_or_else(|_| lmstudio::DEFAULT_BASE_URL.to_string());
        let url = format!(
            "{}/api/v0/models",
            base_url.trim_end_matches('/').trim_end_matches("/v1")
        );
        match self.http.get(url).send().await {
            Ok(r) if r.status().is_success() => match r.json::<Value>().await {
                Ok(body) => lmstudio::parse_details(&body),
                Err(_) => Vec::new(),
            },
            _ => Vec::new(),
        }
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

        // El catálogo real solo se puede pedir con una credencial válida, así
        // que este es el momento.
        for result in self.refresh_catalog_from_providers().await {
            if let Some(error) = &result.error {
                tracing::warn!(%error, "no se pudo descubrir el catálogo tras conectar");
            }
        }

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
    /// Emite una aplicación **sin conceder nada**.
    ///
    /// Antes concedía automáticamente todas las vías conectadas, lo que contradecía
    /// el principio que este mismo código declara en `policy.rs`: «el acceso se
    /// concede, no se deniega». Y con permisos por modelo ese automatismo sería peor
    /// todavía: daría los sesenta modelos de un proveedor a cualquier herramienta
    /// nueva. Los modelos se marcan después, en los permisos de la aplicación
    /// (spec 0004).
    pub fn create_app(&self, name: &str, notes: Option<&str>) -> Result<IssuedApp> {
        self.db.create_app(name, notes)
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
                provider_id: account.provider_id.clone(),
                kind: account.credential_kind,
                secret: String::new(),
                // Para un proveedor local, `external_id` es la dirección de su
                // servidor. Descartarla dejaba al adaptador usando la que leyó al
                // arrancar, así que cambiar la dirección no surtía efecto.
                external_id: account.external_id.clone(),
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
                    provider_id: account.provider_id.clone(),
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
                    provider_id: current.provider_id.clone(),
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
            provider_id: current.provider_id.clone(),
            kind: current.credential_kind,
            secret: tokens.access_token,
            external_id,
        })
    }

    // -- Catálogo por aplicación -------------------------------------------

    /// Modelos que una aplicación concreta puede usar, con la vía anotada.
    ///
    /// Registra la consulta, y cuando el resultado es vacío anota el motivo.
    /// Sin esto un catálogo vacío no deja rastro, y desde el cliente es
    /// indistinguible de un token inválido o de un fallo del gateway: los tres
    /// se ven como «no se encontraron modelos».
    pub fn models_for_app(&self, app_id: &str) -> Result<Vec<Value>> {
        let started = Instant::now();
        let models = self.build_models_for_app(app_id)?;

        let reason = if !models.is_empty() {
            None
        } else {
            Some(self.empty_catalog_reason(app_id)?)
        };

        self.record_catalog_query(app_id, models.len(), reason, started);
        Ok(models)
    }

    /// Por qué el catálogo de una aplicación salió vacío.
    ///
    /// Cuatro motivos distinguibles, porque los cuatro se arreglan de forma distinta y
    /// desde el cliente los cuatro se ven igual: «no se encontraron modelos». No hay un
    /// motivo para «vía concedida sin modelos marcados» porque ese estado no existe:
    /// marcar el primer modelo concede la vía y desmarcar el último la retira, así que
    /// son cero filas, indistinguibles de no haber concedido nada.
    fn empty_catalog_reason(&self, app_id: &str) -> Result<&'static str> {
        let grants = self.db.grants(app_id)?;
        if grants.is_empty() {
            return Ok("no_grants");
        }
        if self.db.accounts()?.iter().all(|a| a.status == "revoked") {
            return Ok("no_account");
        }

        let catalog = self.db.catalog_rows()?;
        if catalog.is_empty() {
            return Ok("empty_catalog");
        }
        // Hay permisos y hay catálogo, pero ninguno de los modelos marcados existe hoy:
        // el proveedor cambió sus identificadores o dejó de ofrecerlos. Las filas se
        // conservan a propósito (son intención del usuario), así que hay que poder
        // diagnosticar este caso desde el panel.
        let marked_but_gone = grants.iter().any(|g| {
            g.model_pattern != "*"
                && !catalog.iter().any(|r| {
                    r.provider_id == g.provider_id
                        && r.credential_kind == g.credential_kind
                        && r.public_name == g.model_pattern
                })
        });
        if marked_but_gone {
            return Ok("no_models_match");
        }
        Ok("empty_catalog")
    }

    /// Anota una consulta de catálogo como evento de operación `models`.
    ///
    /// No consume cuota ni coste, así que se registra aparte de la inferencia y
    /// el panel la excluye de los totales de uso.
    fn record_catalog_query(
        &self,
        app_id: &str,
        count: usize,
        reason: Option<&str>,
        started: Instant,
    ) {
        let event = crate::db::stats::RequestEvent {
            id: util::new_id("req"),
            ts: util::now_ms(),
            app_id: app_id.to_string(),
            provider_id: "-".into(),
            credential_kind: "-".into(),
            account_id: None,
            public_model: format!("{count} modelo(s)"),
            api_model: "-".into(),
            operation: "models".into(),
            streamed: false,
            status: if reason.is_some() {
                crate::db::stats::RequestStatus::Error
            } else {
                crate::db::stats::RequestStatus::Ok
            },
            error_kind: reason.map(str::to_string),
            http_status: Some(200),
            latency_ms: Some(started.elapsed().as_millis() as i64),
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
            tracing::error!(error = %e, "no se pudo registrar la consulta de catálogo");
        }
    }

    fn build_models_for_app(&self, app_id: &str) -> Result<Vec<Value>> {
        let grants = self.db.grants(app_id)?;
        let accounts = self.db.accounts()?;
        let rows = self.db.catalog_rows()?;

        let mut out = Vec::new();
        for row in rows {
            // La misma función que usa el gateway, no una condición parecida: cuando
            // eran dos, el catálogo anunciaba modelos que después se rechazaban.
            if crate::policy::grant_for(
                &grants,
                &row.provider_id,
                &row.credential_kind,
                &row.public_name,
            )
            .is_none()
            {
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

    /// Vías a las que se puede conceder acceso, derivadas del catálogo.
    ///
    /// Existe porque la interfaz llevaba esta lista escrita a mano y se quedó sin
    /// `lmstudio` al añadirlo: los modelos locales estaban detectados y en catálogo,
    /// pero era imposible autorizarlos. Derivarla de los datos hace que un proveedor
    /// nuevo aparezca sin tocar la interfaz.
    pub fn grantable_routes(&self) -> Result<Vec<GrantableRoute>> {
        let accounts = self.db.accounts()?;
        let mut seen: Vec<GrantableRoute> = Vec::new();

        for row in self.db.catalog_rows()? {
            let kind = CredentialKind::parse(&row.credential_kind)
                .unwrap_or(CredentialKind::ApiKey);

            if let Some(existing) = seen
                .iter_mut()
                .find(|r| r.provider_id == row.provider_id && r.credential_kind == row.credential_kind)
            {
                existing.models += 1;
                continue;
            }

            let connected = kind == CredentialKind::Mock
                || accounts.iter().any(|a| {
                    a.provider_id == row.provider_id
                        && a.credential_kind == kind
                        && a.status == "active"
                });

            seen.push(GrantableRoute {
                provider_id: row.provider_id.clone(),
                credential_kind: row.credential_kind.clone(),
                connected,
                requires_limit: kind.requires_app_limit(),
                models: 1,
            });
        }

        // Las utilizables primero: son las que el usuario quiere conceder.
        seen.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then_with(|| a.provider_id.cmp(&b.provider_id))
                .then_with(|| a.credential_kind.cmp(&b.credential_kind))
        });
        Ok(seen)
    }

    /// Lo que el usuario tiene conectado, una fila por pareja proveedor+credencial,
    /// ya ordenado para presentarse.
    ///
    /// Se compone aquí y no en la interfaz porque agrupar por eje de credencial y
    /// decidir qué cuenta exige atención es dominio, no presentación. La vista lo
    /// hacía por su cuenta y se equivocaba: filtraba por tipo de credencial sin mirar
    /// el proveedor, así que cualquier proveedor propio con API key aparecía además
    /// dentro de la caja de OpenAI (spec 0003).
    pub fn provider_rows(&self) -> Result<Vec<ProviderRow>> {
        let catalog = self.db.catalog_rows()?;
        let custom = self.db.custom_providers()?;

        let mut rows: Vec<ProviderRow> = self
            .db
            .accounts()?
            .into_iter()
            .filter(|a| a.status != "revoked")
            .map(|a| {
                let models = catalog
                    .iter()
                    .filter(|r| {
                        r.provider_id == a.provider_id
                            && r.credential_kind == a.credential_kind.as_str()
                    })
                    .count();
                let manage = if custom.iter().any(|c| c.id == a.provider_id) {
                    RowManage::CustomProvider
                } else if a.credential_kind == CredentialKind::Local {
                    RowManage::LocalServer
                } else {
                    RowManage::Account
                };

                ProviderRow {
                    note: route_note(&a.provider_id, a.credential_kind),
                    account_id: a.id,
                    provider_id: a.provider_id,
                    credential_kind: a.credential_kind.as_str().to_string(),
                    name: a.label,
                    needs_attention: needs_attention(&a.status),
                    status: a.status,
                    models,
                    address: a.external_id.filter(|v| v.starts_with("http")),
                    manage,
                    expires_at: a.expires_at,
                    created_at: a.created_at,
                }
            })
            .collect();

        // Lo que exige actuar, delante: si el usuario no despliega la fila, el
        // estado de la línea plegada es lo único que va a leer.
        rows.sort_by(|a, b| {
            b.needs_attention
                .cmp(&a.needs_attention)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.credential_kind.cmp(&b.credential_kind))
        });
        Ok(rows)
    }

    /// Las vías que el usuario puede dar de alta, con la forma de formulario que
    /// necesita cada una.
    ///
    /// Lo declara el núcleo y no la interfaz por el mismo motivo que
    /// `grantable_routes()`: la lista escrita a mano en la vista se quedó sin
    /// `lmstudio` al añadirlo y dejó los modelos locales imposibles de autorizar.
    /// Un proveedor nuevo que encaje en una de las formas no debe tocar la vista.
    pub fn connect_options(&self) -> Result<Vec<ConnectOption>> {
        let accounts = self.db.accounts()?;
        let connected = |provider: &str, kind: CredentialKind| {
            accounts
                .iter()
                .any(|a| a.provider_id == provider && a.credential_kind == kind && a.status != "revoked")
        };
        let lmstudio_url = self.db.settings().unwrap_or_default().lmstudio_base_url;

        let mut out = vec![
            ConnectOption {
                id: AdapterId::new("openai", CredentialKind::SubscriptionOauth).slug(),
                name: "ChatGPT por suscripción".into(),
                summary: "Usa el plan que ya pagas, sin API key y sin coste por token.".into(),
                form: ConnectForm::SubscriptionOauth,
                note: None,
                already_connected: connected("openai", CredentialKind::SubscriptionOauth),
                docs_url: None,
            },
            ConnectOption {
                id: AdapterId::new("lmstudio", CredentialKind::Local).slug(),
                name: "LM Studio".into(),
                summary: "Modelos que corren en tu equipo. Nada sale de la máquina y no hay coste por token.".into(),
                form: ConnectForm::LocalServer { default_url: lmstudio_url },
                note: route_note("lmstudio", CredentialKind::Local),
                already_connected: connected("lmstudio", CredentialKind::Local),
                docs_url: None,
            },
            ConnectOption {
                id: AdapterId::new("openai", CredentialKind::ApiKey).slug(),
                name: "OpenAI por API key".into(),
                summary: "Vía estable y documentada. Se factura por token y sirve de respaldo si la suscripción deja de funcionar.".into(),
                form: ConnectForm::ApiKey,
                note: route_note("openai", CredentialKind::ApiKey),
                already_connected: connected("openai", CredentialKind::ApiKey),
                docs_url: None,
            },
        ];

        // Los atajos OpenAI-compatible: nombre y dirección ya puestos.
        for preset in openai_compat::presets() {
            out.push(ConnectOption {
                id: format!("preset:{}", util::slugify(preset.suggested_name)),
                name: preset.suggested_name.to_string(),
                summary: "Atajo con la dirección ya rellena: solo tienes que pegar la clave.".into(),
                form: ConnectForm::CompatEndpoint {
                    suggested_name: preset.suggested_name.to_string(),
                    base_url: preset.base_url.to_string(),
                },
                note: None,
                already_connected: connected(
                    &util::slugify(preset.suggested_name),
                    CredentialKind::ApiKey,
                ),
                docs_url: Some(preset.docs_url.to_string()),
            });
        }

        // Y el caso general, siempre al final: cualquier otro servicio compatible.
        out.push(ConnectOption {
            id: "compat:custom".into(),
            name: "Otro servicio OpenAI-compatible".into(),
            summary: "OpenRouter, un proxy propio, un servidor de tu empresa… Puedes añadir varios, cada uno con su nombre.".into(),
            form: ConnectForm::CompatEndpoint {
                suggested_name: String::new(),
                base_url: String::new(),
            },
            note: Some(
                "El catálogo se cruza con models.dev para saber sus capacidades y su \
                 precio; lo que no aparezca ahí se ofrece solo como texto."
                    .into(),
            ),
            already_connected: false,
            docs_url: None,
        });

        Ok(out)
    }

    /// Los modelos de una vía con si esta aplicación los tiene marcados.
    ///
    /// Incluye al final los marcados que **ya no están en el catálogo**: se conservan
    /// a propósito —son intención declarada del usuario, y un proveedor que falle un
    /// minuto no debe borrar permisos para siempre—, así que hay que poder verlos y
    /// desmarcarlos.
    pub fn app_route_models(
        &self,
        app_id: &str,
        provider_id: &str,
        kind: CredentialKind,
    ) -> Result<RouteModels> {
        let grants = self.db.grants(app_id)?;
        let route_grants: Vec<_> = grants
            .iter()
            .filter(|g| g.provider_id == provider_id && g.credential_kind == kind.as_str())
            .collect();

        // Un permiso heredado con `*` vale para todos los modelos de la vía, incluidos
        // los que el proveedor añada mañana. No es lo mismo que tenerlos marcados uno a
        // uno, y la interfaz tiene que poder decirlo.
        let all = route_grants.iter().any(|g| g.model_pattern == "*");

        let mut models: Vec<RouteModel> = self
            .db
            .catalog_rows()?
            .into_iter()
            .filter(|r| r.provider_id == provider_id && r.credential_kind == kind.as_str())
            .map(|r| RouteModel {
                selected: all || route_grants.iter().any(|g| g.model_pattern == r.public_name),
                missing: false,
                public_name: r.public_name,
                accounting: r.accounting,
                priced: r.price_input.is_some(),
                caps: r.caps,
            })
            .collect();
        models.sort_by(|a, b| a.public_name.cmp(&b.public_name));

        let known: Vec<&str> = models.iter().map(|m| m.public_name.as_str()).collect();
        let mut orphans: Vec<RouteModel> = route_grants
            .iter()
            .filter(|g| g.model_pattern != "*" && !known.contains(&g.model_pattern.as_str()))
            .map(|g| RouteModel {
                public_name: g.model_pattern.clone(),
                selected: true,
                missing: true,
                accounting: "unavailable".into(),
                priced: false,
                caps: Default::default(),
            })
            .collect();
        orphans.sort_by(|a, b| a.public_name.cmp(&b.public_name));
        models.extend(orphans);

        let selected = models.iter().filter(|m| m.selected).count();
        Ok(RouteModels {
            provider_id: provider_id.to_string(),
            credential_kind: kind.as_str().to_string(),
            requires_limit: kind.requires_app_limit(),
            inherited_all: all,
            selected,
            models,
        })
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

        // Rechazo explícito antes de gastar nada. Se hace aquí y no en el
        // adaptador porque el catálogo real vive en la base de datos: los
        // modelos descubiertos no están en ningún manifiesto local.
        crate::provider::check_capabilities(&req, &resolved.descriptor())?;

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
                    provider_id: "mock".into(),
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
            .adapter_for(&prepared.resolved.provider_id, prepared.resolved.credential_kind)
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
        let mut collector = Collector::since(prepared.started);
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
        Self::since(Instant::now())
    }

    /// Mide el tiempo hasta el primer token desde `started`, que es cuando
    /// arrancó la petición.
    ///
    /// No se mide desde el evento `Started`: en `chat/completions` ese evento
    /// y el primer trozo de texto salen del mismo fragmento SSE, así que la
    /// diferencia entre ambos es siempre ~0. Anclarlo ahí hacía que ocho
    /// segundos de espera se registraran como «0 ms al primer token» para Zen,
    /// OpenAI por API key y LM Studio, mientras la vía de suscripción sí daba
    /// una cifra creíble porque su formato emite un evento antes del contenido.
    pub fn since(started: Instant) -> Self {
        Self { started: Some(started), ..Default::default() }
    }

    pub fn observe(&mut self, event: &ChatEvent) {
        match event {
            ChatEvent::Started { provider_request_id } => {
                self.provider_request_id = provider_request_id.clone();
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

/// Forma del formulario de alta. La interfaz tiene una rama por forma, no por
/// proveedor: así Ollama entra como `LocalServer`, Anthropic por clave como `ApiKey`
/// y Gemini por OAuth como `SubscriptionOauth`, sin tocar la vista.
///
/// No es una descripción genérica de campos a propósito: los flujos no se distinguen
/// por qué campos piden, sino por lo que pasa alrededor —el de suscripción exige
/// aceptar un aviso y esperar un callback del navegador, el local se comprueba antes
/// de guardar—, y un constructor genérico de campos no expresa eso.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectForm {
    /// Aviso de riesgo obligatorio (ADR 0001) y login en el navegador.
    SubscriptionOauth,
    /// Un servidor en la máquina del usuario: solo dirección, y se comprueba.
    LocalServer { default_url: String },
    /// Clave y etiqueta opcional, contra un proveedor conocido.
    ApiKey,
    /// Nombre, dirección y clave. Los dos primeros vienen rellenos si es un atajo,
    /// pero siguen siendo editables: prefijar es una comodidad, no un candado, y si
    /// el proveedor cambia su dirección el usuario tiene que poder corregirla.
    CompatEndpoint { suggested_name: String, base_url: String },
}

/// Una vía que el usuario puede dar de alta.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectOption {
    /// Identificador estable para la interfaz, del estilo «openai:subscription_oauth».
    pub id: String,
    pub name: String,
    pub summary: String,
    pub form: ConnectForm,
    /// Lo que el usuario necesita saber antes de darle. Nace de confusiones reales.
    pub note: Option<String>,
    /// Si ya hay una cuenta por esta vía. Se ofrece igual, pero avisando.
    pub already_connected: bool,
    /// Documentación del proveedor, cuando la tiene.
    pub docs_url: Option<String>,
}

/// La nota que el usuario necesita saber de una vía, si tiene alguna.
///
/// Un único origen para las dos veces que se muestra —al dar de alta y en el detalle
/// de la fila—, porque tenerla escrita en los dos sitios es la forma de que uno se
/// quede atrás. Nacen de confusiones reales, no son adorno.
fn route_note(provider_id: &str, kind: CredentialKind) -> Option<String> {
    match (provider_id, kind) {
        ("lmstudio", CredentialKind::Local) => Some(
            "La primera petición a un modelo que no esté cargado puede tardar bastante \
             —unos 14 segundos en las pruebas con un modelo de 12B— porque LM Studio lo \
             carga en ese momento. No es un cuelgue."
                .into(),
        ),
        ("openai", CredentialKind::ApiKey) => Some(
            "Se guarda en el Keychain del sistema, nunca en la base de datos ni en un \
             fichero."
                .into(),
        ),
        _ => None,
    }
}

/// Si el estado de una cuenta exige que el usuario haga algo.
///
/// Lo desconocido cuenta como que exige atención, a propósito: un estado nuevo que
/// nadie enseñe al usuario es peor que un aviso de más.
fn needs_attention(status: &str) -> bool {
    !matches!(status, "active")
}

/// Un modelo de una vía, con si la aplicación lo tiene marcado.
#[derive(Debug, Clone, Serialize)]
pub struct RouteModel {
    pub public_name: String,
    pub selected: bool,
    /// Marcado pero ausente del catálogo de hoy. Se conserva, no se borra.
    pub missing: bool,
    pub accounting: String,
    pub priced: bool,
    pub caps: crate::provider::Capabilities,
}

/// Los modelos de una vía para una aplicación concreta.
#[derive(Debug, Clone, Serialize)]
pub struct RouteModels {
    pub provider_id: String,
    pub credential_kind: String,
    /// Si conceder esta vía obliga a fijar un límite (ADR 0001).
    pub requires_limit: bool,
    /// Permiso heredado con `*`: vale para todos los modelos de la vía, también los
    /// que el proveedor añada en el futuro. No equivale a marcarlos todos.
    pub inherited_all: bool,
    pub selected: usize,
    pub models: Vec<RouteModel>,
}

/// Cómo se gestiona una fila: de qué comandos dispone.
///
/// Lo declara el núcleo porque no es cosmético: desconectar un proveedor añadido por
/// el usuario no es lo mismo que desconectar una cuenta. Si la vista llamara a
/// `disconnect_account` para un proveedor propio, su definición quedaría huérfana en
/// `custom_providers` y volvería a aparecer sin cuenta.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowManage {
    /// Solo se desconecta.
    Account,
    /// Servidor local: dirección editable y comprobación bajo demanda.
    LocalServer,
    /// Proveedor añadido por el usuario: dirección editable, y al quitarlo se borra
    /// también su definición, no solo la cuenta.
    CustomProvider,
}

/// Una vía conectada, tal como se presenta en la pestaña de Proveedores.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderRow {
    pub account_id: String,
    pub provider_id: String,
    pub credential_kind: String,
    /// Etiqueta de la cuenta: «ChatGPT (correo)», «OpenCode Zen», «LM Studio (url)».
    pub name: String,
    pub status: String,
    /// Modelos que ofrece esta pareja proveedor+credencial, del catálogo ya guardado.
    /// No se pregunta al proveedor: abrir una pestaña no debe generar tráfico.
    pub models: usize,
    /// Dirección del servidor, cuando la vía tiene una.
    pub address: Option<String>,
    /// De qué comandos dispone esta fila.
    pub manage: RowManage,
    /// Lo que el usuario necesita saber de esta vía. Mismo origen que en el alta.
    pub note: Option<String>,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    /// Si esta fila exige atención. Decide el orden y el aviso.
    pub needs_attention: bool,
}

/// Una vía a la que se puede conceder acceso.
#[derive(Debug, Clone, Serialize)]
pub struct GrantableRoute {
    pub provider_id: String,
    pub credential_kind: String,
    /// Si hay cuenta activa. Sin ella, conceder acceso no sirve de nada todavía.
    pub connected: bool,
    /// Si conceder esta vía obliga a fijar un límite (ADR 0001).
    pub requires_limit: bool,
    pub models: usize,
}

/// Resultado de refrescar el catálogo de una vía concreta.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogRefresh {
    pub provider_id: String,
    pub credential_kind: String,
    pub discovered: usize,
    pub error: Option<String>,
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
    use crate::provider::ModelDescriptor;

    fn nexo() -> Arc<Nexo> {
        Nexo::new(
            Db::open_in_memory().unwrap(),
            Arc::new(MemorySecretStore::default()),
        )
        .unwrap()
    }

    /// La retención existía como botón en Configuración, pero nadie lo pulsaba: sin
    /// esto, `requests` crecía sin límite mientras la app estuviera instalada
    /// (encontrado al revisar el panel real del usuario, con datos desde el día
    /// anterior sin ninguna poda). Se comprueba reabriendo el mismo `Db` —simulando
    /// un reinicio— y viendo que un evento más viejo que la retención configurada
    /// desaparece sin que nadie pulse nada.
    #[test]
    fn old_requests_are_pruned_automatically_on_startup() {
        let db = Db::open_in_memory().unwrap();
        let secrets = Arc::new(MemorySecretStore::default()) as Arc<dyn crate::secrets::SecretStore>;

        // Retención corta, para no depender del valor por defecto.
        let mut settings = db.settings().unwrap();
        settings.retention_days = 30;
        db.save_settings(&settings).unwrap();

        // Primer arranque: no hay nada que podar todavía.
        Nexo::new(db.clone(), secrets.clone()).unwrap();

        let old_id = "req_viejo";
        db.record_request(&crate::db::stats::RequestEvent {
            id: old_id.into(),
            ts: util::now_ms() - 200 * 86_400_000, // muy anterior a los 30 días
            app_id: "app1".into(),
            provider_id: "openai".into(),
            credential_kind: "api_key".into(),
            account_id: None,
            public_model: "openai/gpt-5.5".into(),
            api_model: "gpt-5.5".into(),
            operation: "chat".into(),
            streamed: false,
            status: crate::db::stats::RequestStatus::Ok,
            error_kind: None,
            http_status: Some(200),
            latency_ms: Some(100),
            ttft_ms: None,
            input_tokens: Some(1),
            output_tokens: Some(1),
            cached_input_tokens: None,
            reasoning_tokens: None,
            usage_source: UsageSource::Reported,
            cost_micros: None,
            cost_basis: CostBasis::Reported,
            fallback_from: None,
            provider_usage_raw: None,
            provider_request_id: None,
        })
        .unwrap();
        assert!(
            db.recent_requests(10).unwrap().iter().any(|r| r.id == old_id),
            "el evento viejo debe existir antes de reiniciar"
        );

        // Segundo arranque, mismo `Db`: simula reabrir la aplicación instalada.
        Nexo::new(db.clone(), secrets).unwrap();

        assert!(
            !db.recent_requests(50).unwrap().iter().any(|r| r.id == old_id),
            "un reinicio debe podar lo que ya supera la retención configurada, sin \
             que el usuario tenga que pulsar nada"
        );
    }

    #[test]
    fn lmstudio_setting_roundtrips_with_a_sane_default() {
        let n = nexo();
        let s = n.db().settings().unwrap();
        assert_eq!(s.lmstudio_base_url, crate::provider::lmstudio::DEFAULT_BASE_URL);

        let mut changed = s.clone();
        changed.lmstudio_base_url = "http://localhost:4321".into();
        n.db().save_settings(&changed).unwrap();
        assert_eq!(
            n.db().settings().unwrap().lmstudio_base_url,
            "http://localhost:4321"
        );
    }

    #[tokio::test]
    async fn detect_lmstudio_rejects_an_address_that_is_not_lm_studio() {
        let n = nexo();
        let mut s = n.db().settings().unwrap();
        // Puerto cerrado: no hay LM Studio ahí.
        s.lmstudio_base_url = "http://127.0.0.1:1".into();
        n.db().save_settings(&s).unwrap();

        let status = n.detect_lmstudio().await.unwrap();
        assert!(!status.reachable);
        assert!(status.detail.is_some(), "hay que decir por qué no se conectó");
        assert!(
            n.db()
                .account_for("lmstudio", CredentialKind::Local)
                .unwrap()
                .is_none(),
            "no se crea cuenta para algo que no es LM Studio"
        );
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
    fn grantable_routes_cover_every_route_in_the_catalog() {
        // Reproduce el fallo del 2026-07-31: la interfaz llevaba la lista de vías
        // escrita a mano y se quedó sin `lmstudio`, así que era imposible conceder
        // acceso a los modelos locales aunque estuvieran detectados y en catálogo.
        // Derivarla del catálogo hace que un proveedor nuevo aparezca solo.
        let n = nexo();
        n.db()
            .replace_models(
                "lmstudio",
                CredentialKind::Local,
                &[crate::provider::ModelDescriptor {
                    api_id: "modelo-local".into(),
                    public_name: "lmstudio/modelo-local".into(),
                    caps: crate::provider::Capabilities {
                        text: true,
                        streaming: true,
                        ..Default::default()
                    },
                    limits: Default::default(),
                    accounting: Accounting::Local,
                    pricing: None,
                }],
                "prueba",
            )
            .unwrap();

        let routes = n.grantable_routes().unwrap();
        let keys: Vec<String> = routes
            .iter()
            .map(|r| format!("{}:{}", r.provider_id, r.credential_kind))
            .collect();

        for expected in [
            "openai:subscription_oauth",
            "openai:api_key",
            "lmstudio:local",
            "mock:mock",
        ] {
            assert!(
                keys.contains(&expected.to_string()),
                "falta la vía {expected} entre las concedibles: {keys:?}"
            );
        }

        let local = routes
            .iter()
            .find(|r| r.provider_id == "lmstudio")
            .expect("lmstudio");
        assert_eq!(local.models, 1);
        assert!(!local.requires_limit, "la vía local no exige límite");
    }

    #[test]
    fn grantable_routes_say_which_ones_have_an_account_connected() {
        let n = nexo();
        let routes = n.grantable_routes().unwrap();

        let sub = routes
            .iter()
            .find(|r| r.credential_kind == "subscription_oauth")
            .unwrap();
        assert!(!sub.connected, "sin cuenta conectada todavía");
        assert!(sub.requires_limit, "la vía de suscripción sí exige límite");

        let mock = routes.iter().find(|r| r.provider_id == "mock").unwrap();
        assert!(mock.connected, "el proveedor de pruebas no necesita cuenta");

        n.connect_openai_api_key("sk-test", None).unwrap();
        let routes = n.grantable_routes().unwrap();
        assert!(
            routes
                .iter()
                .find(|r| r.credential_kind == "api_key")
                .unwrap()
                .connected
        );
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

    // -- Proveedores añadidos por el usuario (spec 0002) --------------------

    #[tokio::test]
    async fn adding_a_custom_provider_creates_its_account_with_the_key_in_the_keychain() {
        let n = nexo();
        let p = n
            .add_custom_provider("OpenCode Zen", "https://opencode.ai/zen/v1", "sk-test-123")
            .await
            .unwrap();
        assert_eq!(p.id, "opencode-zen");

        let account = n.db().account_for("opencode-zen", CredentialKind::ApiKey).unwrap();
        let account = account.expect("debe crear la cuenta");
        assert_eq!(account.external_id.as_deref(), Some("https://opencode.ai/zen/v1"));

        // La clave se busca por el id de la CUENTA, como cualquier otra API key.
        let secret = n.secrets().get(&SecretRef::api_key(&account.id)).unwrap();
        assert_eq!(secret.as_deref(), Some("sk-test-123"));
    }

    #[tokio::test]
    async fn adding_a_provider_without_a_key_is_refused() {
        let n = nexo();
        assert!(n
            .add_custom_provider("Runpod", "https://runpod.example/v1", "  ")
            .await
            .is_err());
        assert!(n.custom_providers().unwrap().is_empty());
    }

    #[tokio::test]
    async fn duplicate_provider_name_does_not_leave_an_orphan_account_or_secret() {
        let n = nexo();
        n.add_custom_provider("Runpod", "https://a.example/v1", "sk-a").await.unwrap();
        assert!(n
            .add_custom_provider("Runpod", "https://b.example/v1", "sk-b")
            .await
            .is_err());
        // Solo una cuenta, solo un proveedor: el segundo intento no dejó restos.
        assert_eq!(n.custom_providers().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn two_custom_providers_coexist_with_separate_accounts() {
        let n = nexo();
        n.add_custom_provider("Runpod", "https://a.example/v1", "sk-a").await.unwrap();
        n.add_custom_provider("Together", "https://b.example/v1", "sk-b").await.unwrap();

        assert_eq!(n.custom_providers().unwrap().len(), 2);
        let a = n.db().account_for("runpod", CredentialKind::ApiKey).unwrap().unwrap();
        let b = n.db().account_for("together", CredentialKind::ApiKey).unwrap().unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(a.external_id.as_deref(), Some("https://a.example/v1"));
        assert_eq!(b.external_id.as_deref(), Some("https://b.example/v1"));
    }

    #[test]
    fn adapter_for_falls_back_to_the_generic_adapter_for_a_custom_provider() {
        let n = nexo();
        n.db().create_custom_provider("Runpod", "https://a.example/v1").unwrap();
        let adapter = n.adapter_for("runpod", CredentialKind::ApiKey);
        assert!(adapter.is_some(), "un proveedor añadido debe resolver a un adaptador");
    }

    #[test]
    fn adapter_for_returns_none_for_a_provider_id_that_does_not_exist_anywhere() {
        let n = nexo();
        assert!(n.adapter_for("fantasma", CredentialKind::ApiKey).is_none());
    }

    #[test]
    fn adapter_for_still_resolves_built_in_providers_first() {
        let n = nexo();
        // "mock" es integrado: no debe ni mirar `custom_providers`.
        assert!(n.adapter_for("mock", CredentialKind::Mock).is_some());
    }

    // -- Modelos permitidos por aplicación (spec 0004) ----------------------

    /// El criterio 4 de la spec 0004, y la razón de que `grant_for` exista: nada
    /// listado puede ser rechazable, y nada rechazable puede estar listado. Antes de
    /// unificar la decisión, el catálogo filtraba solo por vía y el gateway también
    /// por modelo, así que un permiso estrecho listaba 60 modelos y rechazaba 58.
    #[tokio::test]
    async fn catalog_and_gateway_never_disagree_about_what_is_allowed() {
        let n = nexo();
        n.add_custom_provider("Zen", "https://z.example/v1", "sk-z").await.unwrap();
        // Catálogo de tres modelos por esa vía.
        n.db()
            .replace_models(
                "zen",
                CredentialKind::ApiKey,
                &["uno", "dos", "tres"].map(|id| ModelDescriptor {
                    api_id: id.into(),
                    public_name: format!("zen/{id}"),
                    caps: crate::provider::Capabilities {
                        text: true,
                        streaming: true,
                        ..Default::default()
                    },
                    limits: Default::default(),
                    accounting: Accounting::Metered,
                    pricing: None,
                }),
                catalog::MANIFEST_VERSION,
            )
            .unwrap();

        let app = n.db().create_app("cliente", None).unwrap().app;
        // Solo uno marcado, de los tres que existen.
        n.db()
            .set_grant(
                &app.id,
                &crate::apps::Grant {
                    provider_id: "zen".into(),
                    credential_kind: "api_key".into(),
                    model_pattern: "zen/dos".into(),
                    allow_tools: true,
                    allow_multimodal: true,
                    log_content: false,
                },
            )
            .unwrap();

        let listed: Vec<String> = n
            .models_for_app(&app.id)
            .unwrap()
            .into_iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(listed, vec!["zen/dos"], "el catálogo solo anuncia lo marcado");

        // Y la otra mitad del criterio: cada modelo del catálogo completo se comprueba
        // contra la decisión del gateway, y las dos respuestas coinciden.
        let grants = n.db().grants(&app.id).unwrap();
        for row in n.db().catalog_rows().unwrap() {
            let gateway_allows = crate::policy::grant_for(
                &grants,
                &row.provider_id,
                &row.credential_kind,
                &row.public_name,
            )
            .is_some();
            let catalog_lists = listed.contains(&row.public_name);
            assert_eq!(
                catalog_lists, gateway_allows,
                "«{}» se lista {} pero el gateway {}",
                row.public_name,
                if catalog_lists { "sí" } else { "no" },
                if gateway_allows { "lo permite" } else { "lo rechaza" }
            );
        }
    }

    /// Prepara una vía «zen» con tres modelos en catálogo y una cuenta conectada.
    async fn nexo_with_zen_catalog() -> Arc<Nexo> {
        let n = nexo();
        n.add_custom_provider("Zen", "https://z.example/v1", "sk-z").await.unwrap();
        n.db()
            .replace_models(
                "zen",
                CredentialKind::ApiKey,
                &["uno", "dos", "tres"].map(|id| ModelDescriptor {
                    api_id: id.into(),
                    public_name: format!("zen/{id}"),
                    caps: crate::provider::Capabilities {
                        text: true,
                        streaming: true,
                        ..Default::default()
                    },
                    limits: Default::default(),
                    accounting: Accounting::Metered,
                    pricing: None,
                }),
                catalog::MANIFEST_VERSION,
            )
            .unwrap();
        n
    }

    #[test]
    fn a_new_app_is_born_with_no_access() {
        let n = nexo();
        let app = n.create_app("cliente", None).unwrap().app;
        assert!(
            n.db().grants(&app.id).unwrap().is_empty(),
            "el acceso se concede, no se hereda por existir"
        );
        assert!(n.models_for_app(&app.id).unwrap().is_empty());
    }

    #[tokio::test]
    async fn empty_catalog_reason_tells_the_four_cases_apart() {
        let n = nexo_with_zen_catalog().await;
        let app = n.db().create_app("cliente", None).unwrap().app;

        // 1. Nada concedido.
        assert_eq!(n.empty_catalog_reason(&app.id).unwrap(), "no_grants");

        // 2. Modelos marcados que ya no existen en el catálogo.
        n.db()
            .replace_app_models(
                &app.id,
                "zen",
                CredentialKind::ApiKey,
                &[String::from("zen/modelo-que-se-fue")],
                true,
                true,
                None,
                None,
            )
            .unwrap();
        assert_eq!(n.empty_catalog_reason(&app.id).unwrap(), "no_models_match");

        // 3. Marcado y presente: ya no está vacío, así que no hay motivo que dar.
        n.db()
            .replace_app_models(
                &app.id,
                "zen",
                CredentialKind::ApiKey,
                &[String::from("zen/dos")],
                true,
                true,
                None,
                None,
            )
            .unwrap();
        assert_eq!(n.models_for_app(&app.id).unwrap().len(), 1);

        // 4. Sin cuenta activa, aunque el permiso siga ahí.
        let account = n.db().account_for("zen", CredentialKind::ApiKey).unwrap().unwrap();
        n.db().set_account_status(&account.id, "revoked").unwrap();
        assert_eq!(n.empty_catalog_reason(&app.id).unwrap(), "no_account");
    }

    #[tokio::test]
    async fn app_route_models_marks_what_is_selected_and_keeps_the_orphans() {
        let n = nexo_with_zen_catalog().await;
        let app = n.db().create_app("cliente", None).unwrap().app;
        n.db()
            .replace_app_models(
                &app.id,
                "zen",
                CredentialKind::ApiKey,
                &["zen/dos", "zen/ya-no-existe"].map(String::from),
                true,
                true,
                None,
                None,
            )
            .unwrap();

        let route = n.app_route_models(&app.id, "zen", CredentialKind::ApiKey).unwrap();
        assert!(!route.inherited_all);
        assert_eq!(route.selected, 2);
        assert!(!route.requires_limit, "una vía de API key no exige límite");

        let names: Vec<&str> = route.models.iter().map(|m| m.public_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["zen/dos", "zen/tres", "zen/uno", "zen/ya-no-existe"],
            "los del catálogo por nombre, y los huérfanos al final"
        );

        let by = |name: &str| route.models.iter().find(|m| m.public_name == name).unwrap();
        assert!(by("zen/dos").selected && !by("zen/dos").missing);
        assert!(!by("zen/uno").selected);
        assert!(
            by("zen/ya-no-existe").selected && by("zen/ya-no-existe").missing,
            "un marcado que desapareció se conserva y se señala"
        );
    }

    /// Criterio 6: los permisos que ya existen usan `*` y tienen que seguir dando
    /// acceso a todos los modelos de su vía, incluidos los que lleguen después.
    #[tokio::test]
    async fn an_inherited_wildcard_grant_keeps_giving_every_model() {
        let n = nexo_with_zen_catalog().await;
        let app = n.db().create_app("studio", None).unwrap().app;
        n.db()
            .set_grant(
                &app.id,
                &crate::apps::Grant {
                    provider_id: "zen".into(),
                    credential_kind: "api_key".into(),
                    model_pattern: "*".into(),
                    allow_tools: true,
                    allow_multimodal: true,
                    log_content: false,
                },
            )
            .unwrap();

        assert_eq!(n.models_for_app(&app.id).unwrap().len(), 3);

        let route = n.app_route_models(&app.id, "zen", CredentialKind::ApiKey).unwrap();
        assert!(route.inherited_all, "la interfaz tiene que poder decir «todos»");
        assert_eq!(route.selected, 3);
        assert!(route.models.iter().all(|m| m.selected && !m.missing));
    }

    // -- Filas de la pestaña de Proveedores (spec 0003) ---------------------

    /// El fallo que motivó la especificación 0003: la interfaz agrupaba por tipo de
    /// credencial sin mirar el proveedor, así que un proveedor propio con API key
    /// salía además dentro de «OpenAI por API key», duplicado.
    #[tokio::test]
    async fn provider_rows_keep_each_api_key_provider_in_its_own_row() {
        let n = nexo();
        n.add_custom_provider("OpenCode Zen", "https://opencode.ai/zen/v1", "sk-z")
            .await
            .unwrap();
        n.connect_openai_api_key("sk-o", Some("OpenAI personal")).unwrap();

        let rows = n.provider_rows().unwrap();
        let api_key_rows: Vec<_> = rows.iter().filter(|r| r.credential_kind == "api_key").collect();
        assert_eq!(api_key_rows.len(), 2, "dos cuentas de API key, dos filas");

        let zen = rows.iter().filter(|r| r.provider_id == "opencode-zen").count();
        assert_eq!(zen, 1, "el proveedor propio aparece una sola vez");
        let openai = rows.iter().filter(|r| r.provider_id == "openai").count();
        assert_eq!(openai, 1);
    }

    #[tokio::test]
    async fn provider_rows_count_the_models_of_that_exact_route() {
        let n = nexo();
        n.connect_openai_api_key("sk-o", None).unwrap();

        let row = n
            .provider_rows()
            .unwrap()
            .into_iter()
            .find(|r| r.provider_id == "openai")
            .expect("fila de OpenAI");

        let esperado = n
            .db()
            .catalog_rows()
            .unwrap()
            .iter()
            .filter(|r| r.provider_id == "openai" && r.credential_kind == "api_key")
            .count();
        assert_eq!(row.models, esperado);
        assert!(row.models > 0, "el catálogo de OpenAI por API key no está vacío");
    }

    #[test]
    fn provider_rows_only_include_routes_that_have_an_account() {
        let n = nexo();
        // El catálogo trae vías (mock, openai) sin ninguna cuenta conectada.
        assert!(!n.db().catalog_rows().unwrap().is_empty());
        assert!(
            n.provider_rows().unwrap().is_empty(),
            "sin cuentas no hay nada conectado que mostrar"
        );
    }

    #[tokio::test]
    async fn provider_rows_put_what_needs_attention_first() {
        let n = nexo();
        n.add_custom_provider("Alfa", "https://a.example/v1", "sk-a").await.unwrap();
        n.add_custom_provider("Zeta", "https://z.example/v1", "sk-z").await.unwrap();

        // «Zeta» iría última por nombre, pero está rota: tiene que salir primera.
        let zeta = n.db().account_for("zeta", CredentialKind::ApiKey).unwrap().unwrap();
        n.db().set_account_status(&zeta.id, "broken").unwrap();

        let rows = n.provider_rows().unwrap();
        assert_eq!(rows[0].name, "Zeta");
        assert!(rows[0].needs_attention);
        assert_eq!(rows[1].name, "Alfa");
        assert!(!rows[1].needs_attention);
    }

    /// La nota se muestra en dos sitios —al dar de alta y en el detalle de la fila— y
    /// tiene que salir del mismo sitio. Estaba escrita a mano en la vista además de en
    /// el núcleo, que es la forma de que una de las dos se quede atrás.
    #[tokio::test]
    async fn the_note_of_a_route_has_a_single_source() {
        let n = nexo();
        n.connect_openai_api_key("sk-o", None).unwrap();

        let row = n
            .provider_rows()
            .unwrap()
            .into_iter()
            .find(|r| r.provider_id == "openai")
            .unwrap();
        let option = n
            .connect_options()
            .unwrap()
            .into_iter()
            .find(|o| o.name == "OpenAI por API key")
            .unwrap();

        assert!(row.note.is_some(), "la fila trae su nota, no la escribe la vista");
        assert_eq!(row.note, option.note);
    }

    #[test]
    fn an_unknown_account_status_counts_as_needing_attention() {
        // Un estado que nadie enseñe al usuario es peor que un aviso de más.
        assert!(!needs_attention("active"));
        assert!(needs_attention("broken"));
        assert!(needs_attention("expired"));
        assert!(needs_attention("algo_que_todavia_no_existe"));
    }

    /// Un proveedor propio se quita con `remove_custom_provider`, no con
    /// `disconnect_account`: si no, su definición queda huérfana en `custom_providers`
    /// y reaparece sin cuenta. La fila tiene que decirlo, no adivinarlo la vista.
    #[tokio::test]
    async fn each_row_declares_how_it_is_managed() {
        let n = nexo();
        n.add_custom_provider("Runpod", "https://a.example/v1", "sk-a").await.unwrap();
        n.connect_openai_api_key("sk-o", None).unwrap();

        let rows = n.provider_rows().unwrap();

        let runpod = rows.iter().find(|r| r.provider_id == "runpod").unwrap();
        assert!(matches!(runpod.manage, RowManage::CustomProvider));
        assert_eq!(runpod.address.as_deref(), Some("https://a.example/v1"));

        let openai = rows.iter().find(|r| r.provider_id == "openai").unwrap();
        assert!(matches!(openai.manage, RowManage::Account));
        assert!(openai.address.is_none(), "OpenAI no tiene dirección que cambiar");
    }

    // -- Vías que se pueden dar de alta (spec 0003) -------------------------

    #[test]
    fn connect_options_cover_the_four_form_shapes() {
        let options = nexo().connect_options().unwrap();

        let mut formas: Vec<&str> = options
            .iter()
            .map(|o| match o.form {
                ConnectForm::SubscriptionOauth => "subscription_oauth",
                ConnectForm::LocalServer { .. } => "local_server",
                ConnectForm::ApiKey => "api_key",
                ConnectForm::CompatEndpoint { .. } => "compat_endpoint",
            })
            .collect();
        formas.sort_unstable();
        formas.dedup();
        assert_eq!(
            formas,
            vec!["api_key", "compat_endpoint", "local_server", "subscription_oauth"],
            "la vista tiene una rama por forma: si aparece una nueva, hay que añadirla"
        );

        // Y los identificadores no se repiten: la interfaz los usa como clave.
        let mut ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "identificadores repetidos");
    }

    #[test]
    fn the_opencode_zen_shortcut_arrives_with_its_name_and_address_filled() {
        let options = nexo().connect_options().unwrap();
        let zen = options
            .iter()
            .find(|o| o.name == "OpenCode Zen")
            .expect("el atajo de Zen debe ofrecerse");

        match &zen.form {
            ConnectForm::CompatEndpoint { suggested_name, base_url } => {
                assert_eq!(suggested_name, "OpenCode Zen");
                assert_eq!(base_url, "https://opencode.ai/zen/v1");
            }
            other => panic!("Zen no es un endpoint compatible: {other:?}"),
        }
        assert!(zen.docs_url.is_some());
    }

    #[test]
    fn the_generic_compatible_option_leaves_every_field_empty() {
        let options = nexo().connect_options().unwrap();
        let generic = options
            .iter()
            .rfind(|o| matches!(o.form, ConnectForm::CompatEndpoint { .. }))
            .expect("debe haber una opción para un servicio cualquiera");

        match &generic.form {
            ConnectForm::CompatEndpoint { suggested_name, base_url } => {
                assert!(suggested_name.is_empty(), "el caso general no prerrellena nada");
                assert!(base_url.is_empty());
            }
            _ => unreachable!(),
        }
        assert!(
            generic.id == "compat:custom",
            "el caso general va al final de la lista"
        );
    }

    #[test]
    fn the_local_server_option_offers_the_address_the_core_actually_uses() {
        let n = nexo();
        let mut s = n.db().settings().unwrap();
        s.lmstudio_base_url = "http://localhost:4321".into();
        n.db().save_settings(&s).unwrap();

        let options = n.connect_options().unwrap();
        let local = options
            .iter()
            .find(|o| matches!(o.form, ConnectForm::LocalServer { .. }))
            .unwrap();
        match &local.form {
            ConnectForm::LocalServer { default_url } => {
                assert_eq!(default_url, "http://localhost:4321");
            }
            _ => unreachable!(),
        }
    }

    #[tokio::test]
    async fn already_connected_tells_the_truth_for_every_option() {
        let n = nexo();
        assert!(n.connect_options().unwrap().iter().all(|o| !o.already_connected));

        n.connect_openai_api_key("sk-o", None).unwrap();
        n.add_custom_provider("OpenCode Zen", "https://opencode.ai/zen/v1", "sk-z")
            .await
            .unwrap();

        let options = n.connect_options().unwrap();
        let by_name = |name: &str| options.iter().find(|o| o.name == name).unwrap();
        assert!(by_name("OpenAI por API key").already_connected);
        assert!(by_name("OpenCode Zen").already_connected);
        assert!(
            !by_name("ChatGPT por suscripción").already_connected,
            "la suscripción no está conectada en esta prueba"
        );
    }

    /// Esta prueba no comprueba nada en ejecución: su valor es que el `match` es
    /// exhaustivo, así que añadir una forma nueva rompe la compilación hasta que
    /// alguien diga con qué comando se completa. Una forma que la interfaz ofrezca
    /// y no se pueda terminar es un callejón sin salida.
    #[test]
    fn adding_a_form_shape_forces_naming_the_command_that_completes_it() {
        for option in nexo().connect_options().unwrap() {
            let comando = match option.form {
                ConnectForm::SubscriptionOauth => "connect_chatgpt",
                ConnectForm::LocalServer { .. } => "set_lmstudio_url + detect_lmstudio",
                ConnectForm::ApiKey => "connect_openai_api_key",
                ConnectForm::CompatEndpoint { .. } => "add_custom_provider",
            };
            assert!(!comando.is_empty(), "«{}» no tiene comando de alta", option.name);
        }
    }

    #[tokio::test]
    async fn removing_a_custom_provider_deletes_its_account_and_secret() {
        let n = nexo();
        n.add_custom_provider("Runpod", "https://a.example/v1", "sk-a").await.unwrap();
        let account_id = n.db().account_for("runpod", CredentialKind::ApiKey).unwrap().unwrap().id;

        n.remove_custom_provider("runpod").unwrap();

        assert!(n.custom_providers().unwrap().is_empty());
        assert!(n.db().account_for("runpod", CredentialKind::ApiKey).unwrap().is_none());
        assert!(
            n.secrets().get(&SecretRef::api_key(&account_id)).unwrap().is_none(),
            "la clave debe borrarse del Keychain, no quedar huérfana"
        );
        // Y sin cuenta activa, ya no resuelve a ningún adaptador.
        assert!(n.adapter_for("runpod", CredentialKind::ApiKey).is_none());
    }

    #[tokio::test]
    async fn updating_the_url_reaches_the_account_the_adapter_actually_reads() {
        let n = nexo();
        n.add_custom_provider("Runpod", "https://old.example/v1", "sk-a").await.unwrap();
        n.update_custom_provider_url("runpod", "https://new.example/v1/").await.unwrap();

        let account = n.db().account_for("runpod", CredentialKind::ApiKey).unwrap().unwrap();
        assert_eq!(
            account.external_id.as_deref(),
            Some("https://new.example/v1"),
            "el adaptador lee la dirección de la cuenta, no de un caché de arranque"
        );
    }

    #[tokio::test]
    async fn models_dev_refresh_is_reflected_immediately_without_rebuilding_adapters() {
        let n = nexo();
        assert_eq!(n.models_dev.read().await.provider_count(), 0);
        *n.models_dev.write().await = ModelsDevCatalog::parse(&serde_json::json!({
            "opencode": {"models": {"x": {}}}
        }));
        // La misma instancia de adaptador ve el cambio: no hace falta reconstruirlo.
        assert_eq!(n.models_dev.read().await.provider_count(), 1);
    }
}
