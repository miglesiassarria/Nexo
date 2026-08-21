//! Interfaz del gateway: recibe peticiones de las aplicaciones y devuelve
//! respuestas compatibles con OpenAI.

pub mod body;
pub mod routes;
pub mod wire;

pub use routes::router;

use crate::service::Nexo;
use std::net::SocketAddr;
use std::sync::Arc;

pub type ServeError = Box<dyn std::error::Error + Send + Sync>;

/// Reserva el puerto. Se expone aparte de `serve` para poder conocer el puerto
/// efectivo cuando se pide el 0, y para fallar temprano si está ocupado.
pub async fn bind(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(addr).await
}

/// Sirve sobre un listener ya reservado.
pub async fn serve_on(nexo: Arc<Nexo>, listener: tokio::net::TcpListener) -> Result<(), ServeError> {
    axum::serve(listener, router(nexo)).await?;
    Ok(())
}

/// Arranca el gateway. Vive en el mismo proceso que la aplicación de
/// escritorio y sigue sirviendo con la ventana cerrada.
///
/// La capa de escritorio no necesita conocer axum: solo llama aquí.
pub async fn serve(nexo: Arc<Nexo>, addr: SocketAddr) -> Result<(), ServeError> {
    let listener = bind(addr).await?;
    tracing::info!(%addr, "gateway escuchando");
    serve_on(nexo, listener).await
}
