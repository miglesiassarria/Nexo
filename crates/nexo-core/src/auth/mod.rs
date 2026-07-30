//! Gestor de identidad: flujos OAuth, callbacks y renovación.

pub mod callback;
pub mod chatgpt;

use crate::util;

/// Valor `state` de un solo uso para el flujo de autorización.
pub fn new_state() -> String {
    util::b64url(&util::random_bytes(32))
}
