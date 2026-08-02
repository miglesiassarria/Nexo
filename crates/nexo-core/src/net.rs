//! Detección de la IP de red local de esta máquina.
//!
//! Se usa para dos cosas: el `Subject Alternative Name` del certificado que
//! genera `tls_cert`, y la dirección que el panel muestra para conectar
//! desde otro equipo. Las dos comparten la misma noción de "la IP de esta
//! máquina", así que viven detrás de una única función.

use std::net::{IpAddr, UdpSocket};

/// La IP no-loopback que el sistema operativo usaría para salir a la red, o
/// `None` si no hay ninguna ruta disponible.
///
/// No manda ningún paquete: `UdpSocket::connect` en UDP solo resuelve la
/// interfaz de salida contra la tabla de rutas del sistema y fija esa IP
/// local, sin negociar nada con el destino. Por eso funciona sin necesidad
/// de que `8.8.8.8` responda ni de que haya Internet — solo hace falta una
/// ruta por defecto, que en cualquier red local con un router existe.
pub fn detect_lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    let ip = addr.ip();
    if ip.is_loopback() {
        None
    } else {
        Some(ip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn does_not_panic_and_never_returns_loopback() {
        if let Some(ip) = detect_lan_ip() {
            assert!(!ip.is_loopback(), "no debe devolver una IP de loopback");
        }
    }
}
