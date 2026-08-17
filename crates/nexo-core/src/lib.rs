//! Núcleo de Nexo.
//!
//! Todo lo que el gateway necesita para funcionar vive aquí: adaptadores de
//! proveedores, flujos OAuth, políticas, persistencia y estadísticas. La capa
//! de escritorio (`src-tauri`) solo orquesta y presenta.
//!
//! Ver `docs/contrato-proveedor.md` y `docs/modelo-datos.md`.

pub mod apps;
pub mod auth;
pub mod catalog;
pub mod config;
pub mod db;
pub mod error;
pub mod gateway;
pub mod net;
pub mod policy;
pub mod provider;
pub mod secrets;
pub mod service;
pub mod translate;
pub mod util;

pub use error::{CoreError, Result};
pub use provider::{AdapterId, CredentialKind};
pub use service::{GatewayStatus, Nexo};
