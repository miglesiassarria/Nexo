use nexo_core::service::Nexo;
use std::sync::Arc;

pub struct AppState {
    pub nexo: Arc<Nexo>,
}

impl AppState {
    pub fn new(nexo: Arc<Nexo>) -> Self {
        Self { nexo }
    }
}

/// Los comandos devuelven el error como texto: la interfaz solo necesita
/// mostrarlo, y así no se filtran tipos del núcleo al frontend.
pub type CmdResult<T> = Result<T, String>;

pub fn map_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
