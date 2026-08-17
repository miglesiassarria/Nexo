// En Windows, sin esto la app abriría una consola detrás de la ventana.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;
mod tray;

use nexo_core::secrets::SystemSecretStore;
use nexo_core::service::Nexo;
use state::AppState;
use std::sync::Arc;
use tauri::WindowEvent;

fn main() {
    init_tracing();

    let nexo = match Nexo::open_default(Arc::new(SystemSecretStore)) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("Nexo no pudo arrancar: {e}");
            std::process::exit(1);
        }
    };

    let settings = match nexo.db().settings() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no se pudo leer la configuración: {e}");
            std::process::exit(1);
        }
    };

    let state = AppState::new(nexo.clone());

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::gateway_status,
            commands::list_accounts,
            commands::connect_chatgpt,
            commands::connect_api_key,
            commands::disconnect_account,
            commands::detect_lmstudio,
            commands::lmstudio_status,
            commands::set_lmstudio_url,
            commands::lmstudio_models,
            commands::provider_rows,
            commands::connect_options,
            commands::list_custom_providers,
            commands::add_custom_provider,
            commands::update_custom_provider_url,
            commands::remove_custom_provider,
            commands::list_apps,
            commands::create_app,
            commands::revoke_app,
            commands::app_token_secret,
            commands::delete_app,
            commands::app_detail,
            commands::grantable_routes,
            commands::app_route_models,
            commands::set_app_models,
            commands::catalog,
            commands::refresh_catalog,
            commands::usage_summary,
            commands::recent_requests,
            commands::load_settings,
            commands::save_settings,
            commands::lan_risk_notice,
            commands::purge_stats,
            commands::apply_retention,
            commands::set_paused,
            commands::risk_notice,
        ])
        .setup(move |app| {
            tray::install(app.handle())?;

            // El gateway vive en el mismo proceso y sigue sirviendo con la
            // ventana cerrada.
            let gateway_nexo = nexo.clone();

            // Decide la dirección y, si hace falta, el certificado —
            // probado en `Nexo::prepare_gateway_bind` sin depender de
            // Tauri. Si el modo red está pedido pero el certificado no se
            // pudo preparar, el plan ya cae a 127.0.0.1 y deja el motivo en
            // `bind_error`, así que aquí solo hace falta seguir el plan.
            let data_dir = nexo_core::service::default_db_path()
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let plan = nexo.prepare_gateway_bind(&settings, &data_dir);
            let addr = plan.addr;

            // El puerto se reserva de forma síncrona: si está ocupado, el
            // panel debe decirlo en lugar de mostrarse como activo.
            match tauri::async_runtime::block_on(nexo_core::gateway::bind(addr)) {
                Ok(listener) => {
                    tracing::info!(%addr, tls = plan.tls.is_some(), "gateway escuchando");
                    let reporting = nexo.clone();
                    tauri::async_runtime::spawn(async move {
                        let result = match plan.tls {
                            Some(cert) => {
                                nexo_core::gateway::serve_on_tls(gateway_nexo, listener, &cert)
                                    .await
                            }
                            None => nexo_core::gateway::serve_on(gateway_nexo, listener).await,
                        };
                        if let Err(e) = result {
                            tracing::error!(error = %e, "el gateway se detuvo");
                            reporting.set_bind_error(Some(format!(
                                "el gateway se detuvo: {e}"
                            )));
                        }
                    });
                }
                Err(e) => {
                    let detail = format!(
                        "no se pudo reservar {addr}: {e}. Puede que ya haya otra \
                         instancia de Nexo en marcha, o que otro programa ocupe el \
                         puerto. Cámbialo en Configuración y reinicia."
                    );
                    tracing::error!("{detail}");
                    nexo.set_bind_error(Some(detail));
                }
            }

            // El catálogo del proveedor se pide al arrancar, sin bloquear: si
            // falla, queda el manifiesto local y el usuario puede reintentarlo.
            // LM Studio se busca al arrancar: si está abierto, aparece solo.
            let local_nexo = nexo.clone();
            tauri::async_runtime::spawn(async move {
                match local_nexo.detect_lmstudio().await {
                    Ok(status) if status.reachable => tracing::info!(
                        models = status.models,
                        "LM Studio disponible en {}",
                        status.base_url
                    ),
                    Ok(status) => tracing::debug!(
                        detail = ?status.detail,
                        "LM Studio no disponible en {}",
                        status.base_url
                    ),
                    Err(e) => tracing::warn!(error = %e, "fallo detectando LM Studio"),
                }
            });

            // `models.dev` se descarga (o se cachea) y SOLO ENTONCES se
            // descubre el catálogo de cada proveedor — en una sola tarea de
            // fondo, no en dos sin orden entre ellas. Antes eran dos spawns
            // independientes: si el descubrimiento terminaba antes que la
            // carga de `models.dev` (plausible con caché fría), el proveedor
            // se quedaba con el catálogo sin precio ni capacidades hasta el
            // próximo refresco manual. Si falla, los proveedores añadidos por
            // el usuario siguen funcionando, solo que sus modelos aparecen
            // como texto en lugar de con capacidades y precio reales.
            let catalog_nexo = nexo.clone();
            tauri::async_runtime::spawn(async move {
                for result in catalog_nexo.refresh_models_dev_then_catalogs().await {
                    if let Some(error) = &result.error {
                        tracing::warn!(
                            provider = %result.provider_id,
                            %error,
                            "no se pudo descubrir el catálogo al arrancar"
                        );
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            // Cerrar la ventana oculta el panel; no termina Nexo.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("no se pudo construir la aplicación")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                // Sin código de salida explícito, la app sigue en segundo
                // plano aunque no queden ventanas abiertas.
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

fn init_tracing() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let filter = EnvFilter::try_from_env("NEXO_LOG")
        .unwrap_or_else(|_| EnvFilter::new("nexo=info,nexo_core=info,warn"));
    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true))
        .with(filter)
        .init();
}
