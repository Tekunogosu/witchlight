//! Works out and publishes the addresses this service can be reached at.

use std::net::IpAddr;
use std::path::Path;

use crate::util::files;

/// Returns the addresses to report to the operator for a given bind address.
///
/// A bind to every interface also reports this machine's address on the network,
/// because `0.0.0.0` cannot be typed into a browser.
///
/// The network address comes first and loopback last. Order is the only thing
/// that distinguishes them, and the mod hands players the first one.
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

/// Returns true when an address only works on this machine.
#[must_use]
pub fn only_here(address: &str) -> bool {
    ["//127.0.0.1:", "//[::1]:", "//localhost:"].iter().any(|only| address.contains(only))
}

/// Writes the reachable addresses to `service.json` for the mod to read.
///
/// This goes to a file rather than an endpoint because the mod is not the only
/// reader and does not always start the service, and because a file beside the
/// map is how the two halves exchange everything else.
pub fn publish_addresses(data: &Path, bind: &str, addresses: &[String]) {
    let body = serde_json::json!({
        "Urls": addresses,
        "Bind": bind,
        "Version": env!("CARGO_PKG_VERSION"),
    });

    files::publish(&data.join("service.json"), body.to_string().as_bytes());
}

/// Returns this machine's address on the network it routes through.
///
/// Connecting a UDP socket sends no packets. It only asks the routing table
/// which local address would be used. The address it asks about is in the
/// reserved documentation range, which goes nowhere.
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
        // Whatever this machine's network address turns out to be, it comes
        // first and loopback comes last.
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
