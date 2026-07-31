//! Comandos que la interfaz invoca. Aquí no hay lógica de negocio: todo se
//! delega al núcleo.

use crate::state::{map_err, AppState, CmdResult};
use nexo_core::apps::{App, Grant, IssuedApp, Limit};
use nexo_core::config::Settings;
use nexo_core::db::{Account, CatalogRow};
use nexo_core::db::stats::{GroupBy, RequestRow, UsageBucket};
use nexo_core::provider::lmstudio::{LmStudioStatus, LocalModelDetail};
use nexo_core::provider::CredentialKind;
use nexo_core::service::{CatalogRefresh, GatewayStatus, GrantableRoute};
use nexo_core::util;
use serde::Serialize;
use serde_json::Value;
use tauri::State;

const DAY_MS: i64 = 86_400_000;

#[tauri::command]
pub fn gateway_status(state: State<'_, AppState>) -> CmdResult<GatewayStatus> {
    let settings = state.nexo.db().settings().map_err(map_err)?;
    state.nexo.status(&settings).map_err(map_err)
}

#[tauri::command]
pub fn set_paused(state: State<'_, AppState>, paused: bool) -> CmdResult<()> {
    state.nexo.set_paused(paused);
    Ok(())
}

// -- Cuentas ---------------------------------------------------------------

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> CmdResult<Vec<Account>> {
    state.nexo.db().accounts().map_err(map_err)
}

/// Texto que la interfaz debe mostrar y el usuario aceptar ANTES de conectar
/// una suscripción. Vive en el núcleo para que no pueda quedar desalineado con
/// lo que el código hace realmente.
#[tauri::command]
pub fn risk_notice() -> RiskNotice {
    RiskNotice {
        title: "Vas a conectar tu suscripción de ChatGPT por una vía no soportada"
            .into(),
        points: vec![
            "OpenAI no ofrece un mecanismo oficial para que aplicaciones de terceros \
             usen la cuota de una suscripción. Nexo reutiliza el flujo OAuth de su \
             cliente oficial."
                .into(),
            "Puede dejar de funcionar en cualquier momento y sin aviso. Configura una \
             API key como respaldo si necesitas continuidad."
                .into(),
            "Usar la suscripción desde una aplicación no autorizada puede incumplir las \
             condiciones del servicio, con consecuencias sobre tu cuenta."
                .into(),
            "Nexo reparte una única cuota personal entre todas las aplicaciones que \
             conectes. Por eso los límites por aplicación son obligatorios en esta vía."
                .into(),
            "Nexo se identifica ante OpenAI como Nexo. No suplanta a otro cliente."
                .into(),
        ],
        confirm_label: "Entiendo el riesgo y quiero continuar".into(),
    }
}

#[derive(Serialize)]
pub struct RiskNotice {
    pub title: String,
    pub points: Vec<String>,
    pub confirm_label: String,
}

/// Conecta la suscripción de ChatGPT. `risk_acknowledged` debe venir de la
/// aceptación explícita del aviso anterior.
#[tauri::command]
pub async fn connect_chatgpt(
    state: State<'_, AppState>,
    risk_acknowledged: bool,
) -> CmdResult<Account> {
    if !risk_acknowledged {
        return Err(
            "hay que aceptar el aviso de riesgo antes de conectar la suscripción".into(),
        );
    }
    let nexo = state.nexo.clone();
    nexo.connect_chatgpt_subscription(util::now_ms(), |url| {
        open::that(url).map_err(nexo_core::CoreError::Io)
    })
    .await
    .map_err(map_err)
}

#[tauri::command]
pub fn connect_api_key(
    state: State<'_, AppState>,
    api_key: String,
    label: Option<String>,
) -> CmdResult<Account> {
    state
        .nexo
        .connect_openai_api_key(&api_key, label.as_deref())
        .map_err(map_err)
}

#[tauri::command]
pub fn disconnect_account(state: State<'_, AppState>, account_id: String) -> CmdResult<()> {
    state.nexo.disconnect_account(&account_id).map_err(map_err)
}

// -- Proveedores locales ---------------------------------------------------

/// Busca LM Studio y lo conecta si responde como tal.
#[tauri::command]
pub async fn detect_lmstudio(state: State<'_, AppState>) -> CmdResult<LmStudioStatus> {
    let nexo = state.nexo.clone();
    nexo.detect_lmstudio().await.map_err(map_err)
}

/// Estado actual de LM Studio, sin cambiar nada.
#[tauri::command]
pub async fn lmstudio_status(state: State<'_, AppState>) -> CmdResult<LmStudioStatus> {
    let nexo = state.nexo.clone();
    Ok(nexo.lmstudio_status().await)
}

/// Cambia la dirección del servidor local y vuelve a detectarlo.
#[tauri::command]
pub async fn set_lmstudio_url(
    state: State<'_, AppState>,
    base_url: String,
) -> CmdResult<LmStudioStatus> {
    let nexo = state.nexo.clone();
    nexo.set_lmstudio_url(&base_url).await.map_err(map_err)
}

/// Cuantización, arquitectura y estado de carga de los modelos locales.
#[tauri::command]
pub async fn lmstudio_models(state: State<'_, AppState>) -> CmdResult<Vec<LocalModelDetail>> {
    let nexo = state.nexo.clone();
    Ok(nexo.lmstudio_model_details().await)
}

// -- Aplicaciones ----------------------------------------------------------

#[tauri::command]
pub fn list_apps(state: State<'_, AppState>) -> CmdResult<Vec<App>> {
    state.nexo.db().apps().map_err(map_err)
}

/// Crea la aplicación y devuelve el token. Es la única vez que se muestra.
///
/// Nace ya con acceso a las vías que tengan cuenta conectada: si naciera sin
/// permisos, el cliente vería un catálogo vacío sin saber por qué.
#[tauri::command]
pub fn create_app(
    state: State<'_, AppState>,
    name: String,
    notes: Option<String>,
) -> CmdResult<IssuedApp> {
    state
        .nexo
        .create_app_with_access(&name, notes.as_deref())
        .map_err(map_err)
}

#[tauri::command]
pub fn revoke_app(state: State<'_, AppState>, app_id: String) -> CmdResult<()> {
    state.nexo.db().revoke_app(&app_id).map_err(map_err)
}

#[tauri::command]
pub fn delete_app(state: State<'_, AppState>, app_id: String) -> CmdResult<()> {
    state.nexo.db().delete_app(&app_id).map_err(map_err)
}

/// Vías a las que se puede conceder acceso, derivadas del catálogo.
#[tauri::command]
pub fn grantable_routes(state: State<'_, AppState>) -> CmdResult<Vec<GrantableRoute>> {
    state.nexo.grantable_routes().map_err(map_err)
}

#[derive(Serialize)]
pub struct AppDetail {
    pub grants: Vec<Grant>,
    pub limits: Vec<Limit>,
}

#[tauri::command]
pub fn app_detail(state: State<'_, AppState>, app_id: String) -> CmdResult<AppDetail> {
    let db = state.nexo.db();
    Ok(AppDetail {
        grants: db.grants(&app_id).map_err(map_err)?,
        limits: db.limits(&app_id).map_err(map_err)?,
    })
}

/// Concede o revoca el acceso de una aplicación a una vía concreta.
///
/// En la vía de suscripción el límite se crea siempre: no existe forma de
/// conceder el acceso sin él.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn set_app_access(
    state: State<'_, AppState>,
    app_id: String,
    provider_id: String,
    credential_kind: String,
    enabled: bool,
    allow_tools: bool,
    allow_multimodal: bool,
    max_requests: Option<i64>,
    window_seconds: Option<i64>,
) -> CmdResult<()> {
    let kind = CredentialKind::parse(&credential_kind)
        .ok_or_else(|| format!("vía de credencial desconocida: {credential_kind}"))?;
    let db = state.nexo.db();

    if !enabled {
        return db
            .remove_grant(&app_id, &provider_id, &credential_kind, "*")
            .map_err(map_err);
    }

    db.grant_with_mandatory_limit(
        &app_id,
        &provider_id,
        kind,
        allow_tools,
        allow_multimodal,
        max_requests,
        window_seconds,
    )
    .map_err(map_err)
}

// -- Catálogo y estadísticas -----------------------------------------------

#[tauri::command]
pub fn catalog(state: State<'_, AppState>) -> CmdResult<Vec<CatalogRow>> {
    state.nexo.db().catalog_rows().map_err(map_err)
}

/// Pregunta a los proveedores conectados qué modelos ofrecen de verdad.
#[tauri::command]
pub async fn refresh_catalog(state: State<'_, AppState>) -> CmdResult<Vec<CatalogRefresh>> {
    let nexo = state.nexo.clone();
    Ok(nexo.refresh_catalog_from_providers().await)
}

#[tauri::command]
pub fn usage_summary(
    state: State<'_, AppState>,
    days: i64,
    group: String,
    operation: Option<String>,
) -> CmdResult<Vec<UsageBucket>> {
    let since = util::now_ms() - days.max(1) * DAY_MS;
    state
        .nexo
        .db()
        .usage_summary(since, GroupBy::parse(&group), operation.as_deref())
        .map_err(map_err)
}

#[tauri::command]
pub fn recent_requests(state: State<'_, AppState>, limit: i64) -> CmdResult<Vec<RequestRow>> {
    state
        .nexo
        .db()
        .recent_requests(limit.clamp(1, 500))
        .map_err(map_err)
}

// -- Configuración ---------------------------------------------------------

#[tauri::command]
pub fn load_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    state.nexo.db().settings().map_err(map_err)
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<Value> {
    state.nexo.db().save_settings(&settings).map_err(map_err)?;
    // El puerto solo cambia al reiniciar: el gateway ya está escuchando.
    Ok(serde_json::json!({
        "saved": true,
        "restart_required": true,
    }))
}

#[tauri::command]
pub fn apply_retention(state: State<'_, AppState>) -> CmdResult<Value> {
    let s = state.nexo.db().settings().map_err(map_err)?;
    let (requests, content) = state
        .nexo
        .db()
        .apply_retention(s.retention_days, s.content_retention_days)
        .map_err(map_err)?;
    Ok(serde_json::json!({
        "deleted_requests": requests,
        "deleted_content": content,
    }))
}

#[tauri::command]
pub fn purge_stats(state: State<'_, AppState>) -> CmdResult<()> {
    state.nexo.db().purge_all_stats().map_err(map_err)
}
