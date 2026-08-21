//! Recepción e ingestión segura de cuerpos HTTP para el gateway de Nexo.
//!
//! Implementa la ingestión protegida por disco (ADR 0007, Spec 0017):
//! - Comprueba el límite configurado de tamaño en streaming.
//! - Almacena cuerpos pequeños (hasta 4 MiB) en memoria RAM.
//! - Deriva cuerpos mayores a 4 MiB hacia un archivo temporal seguro (0600 en Unix)
//!   que se elimina automáticamente al destruirse (RAII).
//! - Devuelve respuestas de error estructuradas compatibles con OpenAI (HTTP 413 con JSON).

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Umbral en memoria a partir del cual se deriva la recepción a disco (4 MiB).
pub const IN_MEMORY_THRESHOLD_BYTES: usize = 4 * 1024 * 1024;

/// Archivo temporal seguro para el cuerpo de una petición grande.
/// Se elimina automáticamente del disco al salir de ámbito (RAII).
pub struct TempPayloadFile {
    path: PathBuf,
    file: File,
}

impl std::fmt::Debug for TempPayloadFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TempPayloadFile")
            .field("path", &self.path)
            .finish()
    }
}

impl TempPayloadFile {
    pub fn new(dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let filename = format!("req_{}.tmp", Uuid::new_v4());
        let path = dir.join(filename);

        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;

        let mut opts = OpenOptions::new();
        opts.read(true).write(true).create_new(true);
        #[cfg(unix)]
        opts.mode(0o600);

        let file = opts.open(&path)?;
        Ok(Self { path, file })
    }

    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    pub fn file(&self) -> &File {
        &self.file
    }

    pub fn rewind(&mut self) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

impl Drop for TempPayloadFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Contenedor del cuerpo recibido (en RAM o respaldado por archivo temporal).
#[derive(Debug)]
pub enum BodyPayload {
    Memory(Vec<u8>),
    File(TempPayloadFile),
}

impl BodyPayload {
    pub fn reader(&mut self) -> Box<dyn Read + '_> {
        match self {
            BodyPayload::Memory(b) => Box::new(b.as_slice()),
            BodyPayload::File(f) => {
                let _ = f.rewind();
                Box::new(&mut f.file)
            }
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            BodyPayload::Memory(b) => Some(b.as_slice()),
            BodyPayload::File(_) => None,
        }
    }
}

/// Errores durante la recepción del cuerpo.
#[derive(Debug)]
pub enum ReceiveError {
    TooLarge { max_bytes: u64 },
    Io(std::io::Error),
    Network(String),
}

impl IntoResponse for ReceiveError {
    fn into_response(self) -> Response {
        match self {
            ReceiveError::TooLarge { max_bytes } => {
                let mib = max_bytes as f64 / (1024.0 * 1024.0);
                let message = if mib >= 1024.0 {
                    let gib = mib / 1024.0;
                    format!("La petición supera el tamaño máximo permitido por Nexo ({gib:.1} GiB).")
                } else {
                    format!("La petición supera el tamaño máximo permitido por Nexo ({mib:.0} MiB).")
                };

                (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(json!({
                        "error": {
                            "message": message,
                            "type": "invalid_request_error",
                            "code": "request_too_large",
                            "nexo": {
                                "kind": "request_too_large",
                                "max_bytes": max_bytes
                            }
                        }
                    })),
                )
                    .into_response()
            }
            ReceiveError::Io(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {
                        "message": format!("error de almacenamiento temporal al recibir la petición: {e}"),
                        "type": "invalid_request_error",
                        "code": "internal_io_error",
                    }
                })),
            )
                .into_response(),
            ReceiveError::Network(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": {
                        "message": format!("error de red al recibir el cuerpo de la petición: {e}"),
                        "type": "invalid_request_error",
                        "code": "network_error",
                    }
                })),
            )
                .into_response(),
        }
    }
}

/// Recibe el cuerpo de una petición en streaming con control de tamaño y desborde a disco.
pub async fn receive_body(
    body: Body,
    max_bytes: Option<u64>,
    temp_dir: &Path,
) -> Result<BodyPayload, ReceiveError> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut temp_file: Option<TempPayloadFile> = None;
    let mut total_received: u64 = 0;

    let mut stream = body.into_data_stream();

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| ReceiveError::Network(e.to_string()))?;
        let chunk_len = chunk.len() as u64;

        if let Some(limit) = max_bytes {
            if total_received + chunk_len > limit {
                return Err(ReceiveError::TooLarge { max_bytes: limit });
            }
        }

        total_received += chunk_len;

        if let Some(ref mut tf) = temp_file {
            tf.file_mut()
                .write_all(&chunk)
                .map_err(ReceiveError::Io)?;
        } else if buffer.len() + chunk.len() > IN_MEMORY_THRESHOLD_BYTES {
            // Supera el umbral de memoria: pasar a archivo temporal en disco
            let mut tf = TempPayloadFile::new(temp_dir).map_err(ReceiveError::Io)?;
            tf.file_mut()
                .write_all(&buffer)
                .map_err(ReceiveError::Io)?;
            tf.file_mut()
                .write_all(&chunk)
                .map_err(ReceiveError::Io)?;
            buffer.clear();
            buffer.shrink_to_fit();
            temp_file = Some(tf);
        } else {
            buffer.extend_from_slice(&chunk);
        }
    }

    if let Some(mut tf) = temp_file {
        tf.rewind().map_err(ReceiveError::Io)?;
        Ok(BodyPayload::File(tf))
    } else {
        Ok(BodyPayload::Memory(buffer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn receive_body_small_payload_stays_in_memory() {
        let dir = std::env::temp_dir().join(format!("nexo_test_{}", Uuid::new_v4()));
        let data = b"hello small world";
        let body = Body::from(data.to_vec());

        let payload = receive_body(body, Some(1024 * 1024), &dir).await.unwrap();
        match payload {
            BodyPayload::Memory(b) => assert_eq!(b, data),
            BodyPayload::File(_) => panic!("cuerpo pequeño no debe ir a disco"),
        }
    }

    #[tokio::test]
    async fn receive_body_exceeding_memory_threshold_spools_to_disk_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!("nexo_test_{}", Uuid::new_v4()));
        let total_size = 5 * 1024 * 1024; // 5 MiB > 4 MiB threshold
        let data = vec![b'A'; total_size];
        let body = Body::from(data.clone());

        let mut payload = receive_body(body, Some(10 * 1024 * 1024), &dir).await.unwrap();
        let path = match &payload {
            BodyPayload::File(tf) => tf.path.clone(),
            BodyPayload::Memory(_) => panic!("cuerpo de 5 MiB debe derivarse a disco"),
        };

        assert!(path.exists(), "el archivo temporal debe existir en disco mientras payload esté vivo");

        let mut read_buf = Vec::new();
        payload.reader().read_to_end(&mut read_buf).unwrap();
        assert_eq!(read_buf.len(), total_size);
        assert_eq!(read_buf, data);

        // Al destruir payload, el archivo temporal debe borrarse automáticamente
        drop(payload);
        assert!(!path.exists(), "el archivo temporal debe haberse borrado tras drop");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn receive_body_exceeding_max_limit_returns_too_large() {
        let dir = std::env::temp_dir().join(format!("nexo_test_{}", Uuid::new_v4()));
        let data = vec![b'X'; 2000];
        let body = Body::from(data);

        let res = receive_body(body, Some(1000), &dir).await;
        match res {
            Err(ReceiveError::TooLarge { max_bytes }) => assert_eq!(max_bytes, 1000),
            other => panic!("esperaba TooLarge, llegó {other:?}"),
        }
    }
}
