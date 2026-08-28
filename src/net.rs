//! Where this can be reached, and saying so.

use std::net::IpAddr;
use std::path::Path;

use crate::files;

/// The addresses worth telling the operator about.
///
/// `0.0.0.0` is not something anyone can type into a browser, so a bind to every
/// interface also reports this machine's address on the network.
///
/// The one on the network comes first: it is the address worth giving somebody
/// else, and loopback only ever works for whoever is sitting at the machine. The
/// order is the whole of what says which is which, since the mod hands players
/// the first of them.
#[must_use]
pub fn reachable_at(bind: &str) -> Vec<String> {
    let Some((host, port)) = bind.rsplit_once(':') else {
        return vec![format!("http://{bind}")];
    };

    if !matches!(host, "0.0.0.0" | "[::]" | "*") {
        return vec![format!("http://{bind}")];
    }

    let mut addresses = Vec::new();
    if let Some(local) = local_address() {
        addresses.push(format!("http://{local}:{port}"));
    }
    addresses.push(format!("http://127.0.0.1:{port}"));
    addresses
}

/// Whether an address only works for whoever is sitting at this machine.
#[must_use]
pub fn only_here(address: &str) -> bool {
    ["//127.0.0.1:", "//[::1]:", "//localhost:"].iter().any(|only| address.contains(only))
}

/// Publishes where this can be reached, for the half that can tell people.
///
/// Written rather than answered over the socket because the mod is not the only
/// thing that wants it and it is not always the one that started this: a file
/// beside the map is readable by whoever is looking, and is how the two halves
/// already talk about everything else.
pub fn publish_addresses(data: &Path, bind: &str, addresses: &[String]) {
    let body = serde_json::json!({
        "Urls": addresses,
        "Bind": bind,
        "Version": env!("CARGO_PKG_VERSION"),
    });

    let path = data.join("service.json");
    if files::replace(&path, body.to_string().as_bytes()).is_err() {
        eprintln!("witchlight: could not write {}", path.display());
    }
}

/// This machine's address on the network it routes through.
///
/// A connected UDP socket sends nothing — it only asks the routing table which
/// local address would be used — and the address it asks about is the reserved
/// documentation range, which goes nowhere.
fn local_address() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:80").ok()?;
    Some(socket.local_addr().ok()?.ip())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_bound_to_one_interface_is_offered_as_it_stands() {
        assert_eq!(reachable_at("10.0.0.4:8080"), vec!["http://10.0.0.4:8080"]);
        assert_eq!(reachable_at("127.0.0.1:8080"), vec!["http://127.0.0.1:8080"]);
    }

    #[test]
    fn a_bind_to_everything_offers_loopback_last() {
        // Whatever this machine's address turns out to be, the one worth giving
        // somebody else comes first and loopback is the fallback behind it.
        for bind in ["0.0.0.0:8080", "[::]:8080", "*:8080"] {
            let offered = reachable_at(bind);
            assert_eq!(
                offered.last().map(String::as_str),
                Some("http://127.0.0.1:8080"),
                "{bind} should end with loopback"
            );
            assert!(only_here(offered.last().unwrap()));
            if let Some(first) = offered.first().filter(|_| offered.len() > 1) {
                assert!(!only_here(first), "{first} is the one to hand out");
            }
        }
    }

    #[test]
    fn only_the_addresses_nobody_else_can_reach_are_marked() {
        assert!(only_here("http://127.0.0.1:8080"));
        assert!(only_here("http://[::1]:8080"));
        assert!(only_here("http://localhost:8080"));
        assert!(!only_here("http://10.0.0.4:8080"));
        assert!(!only_here("http://192.168.1.20:8080"));
    }
}
