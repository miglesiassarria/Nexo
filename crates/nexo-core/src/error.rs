use crate::provider::AdapterError;

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("base de datos: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("almacén seguro del sistema: {0}")]
    Keyring(String),

    #[error("red: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("entrada/salida: {0}")]
    Io(#[from] std::io::Error),

    #[error("configuración: {0}")]
    Config(String),

    #[error("no encontrado: {0}")]
    NotFound(String),

    #[error("no permitido: {0}")]
    Forbidden(String),

    #[error("flujo de autorización: {0}")]
    Auth(String),

    #[error("proveedor: {0}")]
    Adapter(#[from] AdapterError),

    #[error("{0}")]
    Other(String),
}

impl From<keyring::Error> for CoreError {
    fn from(e: keyring::Error) -> Self {
        CoreError::Keyring(e.to_string())
    }
}
