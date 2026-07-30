//! Servidor local de un solo uso para recibir el callback OAuth.
//!
//! Escucha en 127.0.0.1 y se apaga en cuanto recibe el código o expira. El
//! `state` se verifica antes de aceptar nada.

use crate::error::{CoreError, Result};
use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// Espera un único callback en `port` y devuelve el `code` si el `state` coincide.
pub async fn wait_for_code(
    port: u16,
    path: &str,
    expected_state: &str,
    timeout: Duration,
) -> Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
        CoreError::Auth(format!(
            "no se pudo escuchar en 127.0.0.1:{port} para el callback OAuth ({e}). \
             Ese puerto es parte del redirect_uri registrado y no es configurable: \
             cierra el proceso que lo esté ocupando."
        ))
    })?;

    let accept = async {
        loop {
            let (mut socket, _) = listener.accept().await?;

            let mut buf = vec![0u8; 8192];
            let n = socket.read(&mut buf).await?;
            let request = String::from_utf8_lossy(&buf[..n]).to_string();

            let target = request_target(&request);
            let Some(target) = target else {
                respond(&mut socket, 400, &page_error("Petición no válida")).await?;
                continue;
            };

            let (req_path, query) = split_target(&target);
            if req_path != path {
                respond(&mut socket, 404, "not found").await?;
                continue;
            }

            let params = parse_query(query);

            if let Some(err) = params.get("error") {
                let detail = params
                    .get("error_description")
                    .cloned()
                    .unwrap_or_else(|| err.clone());
                respond(&mut socket, 200, &page_error(&detail)).await?;
                return Ok(Err(CoreError::Auth(format!(
                    "el proveedor rechazó la autorización: {detail}"
                ))));
            }

            match (params.get("code"), params.get("state")) {
                (Some(code), Some(state)) if state == expected_state => {
                    respond(&mut socket, 200, &page_ok()).await?;
                    return Ok(Ok(code.clone()));
                }
                (Some(_), Some(_)) => {
                    respond(&mut socket, 400, &page_error("State no coincide")).await?;
                    return Ok(Err(CoreError::Auth(
                        "el parámetro state no coincide: posible CSRF, autorización descartada"
                            .into(),
                    )));
                }
                _ => {
                    respond(&mut socket, 400, &page_error("Falta el código")).await?;
                    return Ok(Err(CoreError::Auth(
                        "el callback llegó sin código de autorización".into(),
                    )));
                }
            }
        }
    };

    match tokio::time::timeout(timeout, accept).await {
        Err(_) => Err(CoreError::Auth(
            "el callback OAuth no llegó a tiempo; la autorización se ha cancelado".into(),
        )),
        Ok(Err(e)) => Err(CoreError::Io(e)),
        Ok(Ok(inner)) => inner,
    }
}

fn request_target(request: &str) -> Option<String> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    Some(parts.next()?.to_string())
}

fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((
                urlencoding::decode(k).ok()?.into_owned(),
                urlencoding::decode(v).ok()?.into_owned(),
            ))
        })
        .collect()
}

async fn respond(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "OK",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.flush().await
}

fn page(title: &str, message: &str, accent: &str) -> String {
    format!(
        r#"<!doctype html><html lang="es"><head><meta charset="utf-8">
<title>Nexo</title><style>
:root{{color-scheme:light dark}}
body{{font:15px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;
display:grid;place-items:center;min-height:100vh;margin:0;
background:Canvas;color:CanvasText}}
main{{text-align:center;max-width:32rem;padding:2rem}}
h1{{font-size:1.25rem;margin:0 0 .5rem;color:{accent}}}
p{{margin:0;opacity:.75}}
</style></head><body><main><h1>{title}</h1><p>{message}</p></main></body></html>"#
    )
}

fn page_ok() -> String {
    page(
        "Cuenta conectada",
        "Ya puedes cerrar esta pestaña y volver a Nexo.",
        "#16a34a",
    )
}

fn page_error(detail: &str) -> String {
    page("No se pudo conectar la cuenta", &html_escape(detail), "#dc2626")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_target() {
        let req = "GET /auth/callback?code=abc&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(
            request_target(req).as_deref(),
            Some("/auth/callback?code=abc&state=xyz")
        );
    }

    #[test]
    fn rejects_non_get() {
        let req = "POST /auth/callback HTTP/1.1\r\n\r\n";
        assert!(request_target(req).is_none());
    }

    #[test]
    fn parses_url_encoded_query() {
        let params = parse_query("code=a%2Fb&state=s+1&error_description=algo%20mal");
        assert_eq!(params.get("code").unwrap(), "a/b");
        assert_eq!(params.get("error_description").unwrap(), "algo mal");
    }

    #[test]
    fn splits_target_without_query() {
        assert_eq!(split_target("/auth/callback"), ("/auth/callback", ""));
    }

    #[test]
    fn escapes_html_in_error_page() {
        let html = page_error("<script>alert(1)</script>");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[tokio::test]
    async fn times_out_when_no_callback_arrives() {
        let err = wait_for_code(0, "/auth/callback", "s", Duration::from_millis(50))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no llegó a tiempo"));
    }
}
