//! Icono permanente en la barra de estado.
//!
//! Es punto de acceso rápido e indicador de salud, no sustituto del panel.
//!
//! Usa deliberadamente los iconos de `tray/`, no el icono de aplicación: en
//! macOS el sufijo `Template` permite que el sistema adapte el color al modo
//! claro u oscuro.

use crate::state::AppState;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Abrir panel de Nexo", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pausar gateway", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume", "Reanudar gateway", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Salir de Nexo", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(app, &[&open, &sep, &pause, &resume, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("nexo-tray")
        .tooltip("Nexo")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event);

    if let Some(icon) = tray_icon(app) {
        builder = builder.icon(icon);
        // En macOS el icono monocromo debe declararse como plantilla.
        #[cfg(target_os = "macos")]
        {
            builder = builder.icon_as_template(true);
        }
    }

    builder.build(app)?;
    Ok(())
}

fn tray_icon<R: Runtime>(app: &AppHandle<R>) -> Option<tauri::image::Image<'static>> {
    // En macOS, `nexoTemplate` se adapta al tema del sistema. En el resto se
    // usa la variante monocroma correspondiente.
    #[cfg(target_os = "macos")]
    let candidates = ["icons/tray/nexoTemplate@2x.png", "icons/tray/nexoTemplate.png"];
    #[cfg(target_os = "windows")]
    let candidates = ["icons/tray/nexo-tray-dark.ico", "icons/tray/nexo-tray-dark-32.png"];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let candidates = ["icons/tray/nexo-tray-dark-32.png"];

    let resolver = app.path();
    for relative in candidates {
        if let Ok(path) = resolver.resolve(relative, tauri::path::BaseDirectory::Resource) {
            if let Ok(image) = tauri::image::Image::from_path(&path) {
                return Some(image);
            }
        }
    }
    // Sin icono de bandeja no se instala uno de aplicación: el manual de
    // identidad lo prohíbe explícitamente por legibilidad.
    tracing::warn!("no se encontró el icono de bandeja en los recursos");
    None
}

fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        "open" => show_panel(app),
        "pause" => set_paused(app, true),
        "resume" => set_paused(app, false),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn show_panel<R: Runtime>(app: &AppHandle<R>) {
    // El icono del Dock, primero. Al cerrar el panel Nexo pasa a accesoria, y
    // una aplicación accesoria puede no recibir el foco: pedirlo antes de
    // volver a ser normal es justo el orden que falla.
    #[cfg(target_os = "macos")]
    {
        if let Err(e) = app.set_dock_visibility(true) {
            tracing::warn!(error = %e, "no se pudo recuperar el icono del Dock");
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn set_paused<R: Runtime>(app: &AppHandle<R>, paused: bool) {
    if let Some(state) = app.try_state::<AppState>() {
        state.nexo.set_paused(paused);
        tracing::info!(paused, "estado del gateway cambiado desde la bandeja");
    }
}
