//! Direcciones de red de esta máquina.
//!
//! Dos preguntas distintas que se confunden con facilidad: «¿por qué IP sale
//! esta máquina a la red?» (una, la de la ruta por defecto) y «¿por qué
//! direcciones queda escuchando el gateway?» (todas las de todas las
//! interfaces, porque escucha en `0.0.0.0`). La primera es la que el usuario
//! querrá usar; la segunda es su exposición real, y desde el
//! [ADR 0005](../../../docs/adr/0005-red-local-sin-cifrado.md) —que retiró el
//! cifrado del acceso desde la red local— enseñarla entera es la mitigación
//! principal que queda.

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

/// Una dirección por la que el gateway queda alcanzable al escuchar en
/// `0.0.0.0`, con el nombre de su interfaz para que el usuario reconozca de
/// qué red se trata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ListeningAddress {
    pub interface: String,
    pub address: String,
    /// La de la ruta por defecto: la que el usuario querrá usar casi siempre.
    pub preferred: bool,
}

/// Todas las direcciones IPv4 no-loopback de la máquina, la preferida
/// primero. Lista vacía si no hay ninguna (sin red).
///
/// Solo IPv4: ningún cliente de los que se usan contra Nexo pide una URL
/// IPv6, y añadirlas alargaría la lista sin utilidad. Las link-local
/// (`169.254.x.x`, `fe80::`) quedan fuera porque `if-addrs` no las incluye sin
/// activar su característica opcional, que no se activa.
pub fn listening_addresses() -> Vec<ListeningAddress> {
    let preferred = detect_lan_ip();
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        // Sin poder enumerar, la ruta por defecto es mejor que nada: es
        // exactamente lo que el panel mostraba antes de saber enumerar.
        return preferred
            .filter(IpAddr::is_ipv4)
            .map(|ip| ListeningAddress {
                interface: "?".into(),
                address: ip.to_string(),
                preferred: true,
            })
            .into_iter()
            .collect();
    };

    let mut found: Vec<ListeningAddress> = interfaces
        .into_iter()
        .filter(|i| !i.is_loopback() && i.addr.ip().is_ipv4())
        .map(|i| ListeningAddress {
            interface: i.name,
            preferred: Some(i.addr.ip()) == preferred,
            address: i.addr.ip().to_string(),
        })
        .collect();

    found.sort_by_key(|a| (!a.preferred, a.interface.clone()));
    found
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

    #[test]
    fn listening_addresses_never_include_loopback_and_are_all_ipv4() {
        for a in listening_addresses() {
            assert!(
                !a.address.starts_with("127."),
                "loopback no es una dirección de red: {}",
                a.address
            );
            assert!(
                a.address.parse::<std::net::Ipv4Addr>().is_ok(),
                "solo se anuncian IPv4: {}",
                a.address
            );
            assert!(!a.interface.is_empty(), "cada dirección dice su interfaz");
        }
    }

    /// En una máquina con red, la preferida —la de la ruta por defecto— tiene
    /// que estar en la lista y ser la primera. Sin red no hay nada que
    /// comprobar, y la prueba no debe fallar por eso.
    #[test]
    fn the_default_route_address_comes_first_when_there_is_one() {
        let Some(expected) = detect_lan_ip().filter(IpAddr::is_ipv4) else {
            return;
        };
        let found = listening_addresses();
        let first = found.first().expect("con ruta por defecto hay al menos una");
        assert_eq!(first.address, expected.to_string());
        assert!(first.preferred);
        assert_eq!(
            found.iter().filter(|a| a.preferred).count(),
            1,
            "solo una puede ser la de la ruta por defecto"
        );
    }
}
