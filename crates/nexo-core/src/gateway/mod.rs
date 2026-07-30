//! Interfaz del gateway: recibe peticiones de las aplicaciones y devuelve
//! respuestas compatibles con OpenAI.

pub mod routes;
pub mod wire;

pub use routes::router;
