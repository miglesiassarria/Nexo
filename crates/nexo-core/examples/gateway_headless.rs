//! Arranca solo el gateway, sin aplicación de escritorio.
//!
//! Sirve para probar el núcleo sin compilar Tauri ni el frontend: emite un
//! token, concede acceso al proveedor mock y queda escuchando.
//!
//! ```sh
//! cargo run -p nexo-core --example gateway_headless
//! ```
//!
//! Variables opcionales:
//! - `NEXO_PORT`: puerto de escucha (por defecto 8787).
//! - `NEXO_DATA_DIR`: si se define, usa una base de datos en disco en esa
//!   carpeta en lugar de una en memoria.
//!
//! No conecta ninguna cuenta real: el proveedor mock no sale de la máquina.

use nexo_core::db::Db;
use nexo_core::gateway;
use nexo_core::provider::CredentialKind;
use nexo_core::secrets::MemorySecretStore;
use nexo_core::service::Nexo;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port: u16 = std::env::var("NEXO_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8787);

    let db = match std::env::var_os("NEXO_DATA_DIR") {
        Some(dir) => {
            let path = std::path::PathBuf::from(dir).join("nexo.sqlite");
            println!("base de datos: {}", path.display());
            Db::open(&path)?
        }
        None => {
            println!("base de datos: en memoria (se pierde al salir)");
            Db::open_in_memory()?
        }
    };

    // Almacén de secretos en memoria: este arranque no toca el Keychain.
    let nexo = Nexo::new(db, Arc::new(MemorySecretStore::default()))?;

    let issued = nexo.db().create_app("gateway_headless", None)?;
    nexo.db().grant_with_mandatory_limit(
        &issued.app.id,
        "mock",
        CredentialKind::Mock,
        false,
        false,
        None,
        None,
    )?;

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = gateway::bind(addr).await.map_err(|e| {
        format!("no se pudo reservar {addr}: {e}. Prueba con NEXO_PORT=9787.")
    })?;

    println!();
    println!("  Gateway escuchando en http://127.0.0.1:{port}/v1");
    println!("  Token de la aplicación: {}", issued.token);
    println!();
    println!("  Pruébalo:");
    println!(
        "    curl -s http://127.0.0.1:{port}/v1/models \\\n       -H 'Authorization: Bearer {}'",
        issued.token
    );
    println!();
    println!(
        "    curl -N http://127.0.0.1:{port}/v1/chat/completions \\\n       \
         -H 'Authorization: Bearer {}' \\\n       \
         -H 'content-type: application/json' \\\n       \
         -d '{{\"model\":\"mock/mock-echo\",\"messages\":[{{\"role\":\"user\",\"content\":\"hola\"}}],\"stream\":true}}'",
        issued.token
    );
    println!();
    println!("  Ctrl-C para salir.");
    println!();

    gateway::serve_on(nexo, listener).await?;
    Ok(())
}
