//! Interfaz del gateway: recibe peticiones de las aplicaciones y devuelve
//! respuestas compatibles con OpenAI.

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

/// Igual que `serve_on`, pero sirve HTTPS con el certificado autofirmado del
/// acceso desde la red local (`crate::tls_cert`), sobre un listener ya
/// reservado. El puerto se sigue reservando de forma síncrona antes de
/// llamar aquí, igual que en el camino sin TLS: esta función solo cambia
/// cómo se sirve, no cuándo se reserva el puerto.
pub async fn serve_on_tls(
    nexo: Arc<Nexo>,
    listener: tokio::net::TcpListener,
    cert: &crate::tls_cert::LanCertificate,
) -> Result<(), ServeError> {
    let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
        &cert.cert_path,
        &cert.key_path,
    )
    .await?;
    let std_listener = listener.into_std()?;
    let server = axum_server::from_tcp_rustls(std_listener, config)?;
    server.serve(router(nexo).into_make_service()).await?;
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

#[cfg(test)]
mod tls_from_reserved_listener {
    //! No prueba nada de Nexo: solo confirma, contra la librería real, que
    //! `axum-server` puede servir TLS a partir de un `TcpListener` que ya
    //! reservamos nosotros (el mismo patrón que `bind()` + `serve_on()` usan
    //! hoy sin TLS). Es la pieza más incierta del diseño de la spec 0007 —
    //! verificarla aquí, antes de construir `tls_cert` y `serve_on_tls`
    //! encima, evita descubrir a mitad de la implementación que la API
    //! esperada no existe.
    use axum::routing::get;
    use axum::Router;
    use axum_server::tls_rustls::RustlsConfig;
    use rcgen::{generate_simple_self_signed, CertifiedKey};

    #[tokio::test]
    async fn serves_https_from_an_already_bound_listener() {
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["127.0.0.1".to_string()])
                .expect("el certificado autofirmado de prueba debe generarse");
        let cert_pem = cert.pem().into_bytes();
        let key_pem = signing_key.serialize_pem().into_bytes();

        let listener = super::bind("127.0.0.1:0".parse().unwrap())
            .await
            .expect("el puerto de prueba debe reservarse");
        let port = listener.local_addr().expect("dirección local").port();
        let std_listener = listener
            .into_std()
            .expect("el listener de tokio debe poder convertirse a std");

        let config = RustlsConfig::from_pem(cert_pem.clone(), key_pem)
            .await
            .expect("la configuración TLS debe construirse desde el PEM en memoria");

        let router = Router::new().route("/healthz", get(|| async { "ok" }));
        let server = axum_server::from_tcp_rustls(std_listener, config)
            .expect("axum-server debe aceptar un TcpListener ya reservado");

        tokio::spawn(async move {
            let _ = server.serve(router.into_make_service()).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let root_cert =
            reqwest::Certificate::from_pem(&cert_pem).expect("el certificado debe parsearse");
        let client = reqwest::Client::builder()
            .add_root_certificate(root_cert)
            .build()
            .expect("el cliente HTTPS de prueba debe construirse");

        let response = client
            .get(format!("https://127.0.0.1:{port}/healthz"))
            .send()
            .await
            .expect("la petición HTTPS debe llegar y validarse contra el certificado autofirmado");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "ok");
    }
}
