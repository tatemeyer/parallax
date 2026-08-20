//! Routes, and the one rule that keeps the probe safe.
//!
//! The probe has no authentication, no authorization, and no TLS. That
//! is not an omission — it is only defensible because nothing off this
//! machine can open a socket to it. `tailscale serve` terminates TLS
//! and forwards to the loopback port, and the tailnet decides who may
//! reach *that*. [`bind_address`] is where the claim is enforced.

use parallax_baseline::adapters::factory::AdapterConfig;
use parallax_baseline::registry::Registry;
use std::net::{IpAddr, SocketAddr};
use std::time::SystemTime;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::state::envelope;

/// The default loopback port.
pub const DEFAULT_PORT: u16 = 8737;

/// What a request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Everything this machine knows, aggregated.
    State,
    /// Liveness, without a scan.
    Health,
    /// No such path.
    NotFound,
    /// A known path, asked for the wrong way.
    MethodNotAllowed,
}

/// Resolves a method and URL to a route.
///
/// Separated from serving so the table can be tested without a socket.
pub fn route(method: &Method, url: &str) -> Route {
    let path = url.split('?').next().unwrap_or(url);
    let path = path.strip_suffix('/').unwrap_or(path);
    match path {
        "/state" | "/health" if *method != Method::Get => Route::MethodNotAllowed,
        "/state" => Route::State,
        "/health" => Route::Health,
        _ => Route::NotFound,
    }
}

/// The address to bind, or a refusal.
///
/// **The probe binds loopback and nothing else.** A routable address is
/// refused rather than served, because every other security property
/// this binary claims follows from being unreachable. The address that
/// matters most here is a `100.x` tailnet address: it looks safe, and
/// binding it would publish the probe to the whole tailnet directly,
/// bypassing `tailscale serve` and the ACLs that make it a boundary.
pub fn bind_address(host: &str, port: u16) -> Result<SocketAddr, String> {
    let ip: IpAddr = host
        .parse()
        .map_err(|_| format!("`{host}` is not an IP address"))?;
    if !ip.is_loopback() {
        return Err(format!(
            "refusing to bind {ip}: the probe serves loopback only. \
             Publish it with `tailscale serve --bg --https=443 http://127.0.0.1:{port}`."
        ));
    }
    Ok(SocketAddr::new(ip, port))
}

/// `Content-Type: application/json`.
fn json_header() -> Header {
    // The bytes are constant and valid; a malformed constant header is a
    // bug in this line, not a runtime condition worth propagating.
    Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("constant header is well-formed")
}

/// Answers one request.
fn answer(
    request: Request,
    registry: &Registry,
    config: &AdapterConfig,
    peer: &str,
) -> std::io::Result<()> {
    match route(request.method(), request.url()) {
        Route::State => {
            let envelope = envelope(registry, config, peer, SystemTime::now());
            match serde_json::to_string(&envelope) {
                Ok(body) => request.respond(Response::from_string(body).with_header(json_header())),
                // Serialization failing is this crate's bug, and a 500
                // that says so is better than a panic that takes the
                // whole probe down and leaves the cockpit guessing.
                Err(e) => request.respond(
                    Response::from_string(format!("could not serialize state: {e}"))
                        .with_status_code(500),
                ),
            }
        }
        Route::Health => request.respond(Response::from_string("ok")),
        Route::NotFound => {
            request.respond(Response::from_string("not found").with_status_code(404))
        }
        Route::MethodNotAllowed => {
            request.respond(Response::from_string("method not allowed").with_status_code(405))
        }
    }
}

/// Serves until the listener dies.
pub fn serve(server: &Server, registry: &Registry, config: &AdapterConfig, peer: &str) {
    for request in server.incoming_requests() {
        // One bad connection is not a reason to stop serving the other
        // two machines.
        let _ = answer(request, registry, config, peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_routes_resolve() {
        assert_eq!(route(&Method::Get, "/state"), Route::State);
        assert_eq!(route(&Method::Get, "/health"), Route::Health);
    }

    #[test]
    fn a_trailing_slash_or_query_string_does_not_change_the_route() {
        assert_eq!(route(&Method::Get, "/state/"), Route::State);
        assert_eq!(route(&Method::Get, "/state?pretty=1"), Route::State);
    }

    #[test]
    fn an_unknown_path_is_not_found() {
        assert_eq!(route(&Method::Get, "/"), Route::NotFound);
        assert_eq!(route(&Method::Get, "/admin"), Route::NotFound);
    }

    /// Arc 1 is read-only. A POST that fell through to `/state` would be
    /// a control surface nobody specified.
    #[test]
    fn a_known_path_asked_for_the_wrong_way_is_refused_rather_than_served() {
        assert_eq!(route(&Method::Post, "/state"), Route::MethodNotAllowed);
        assert_eq!(route(&Method::Delete, "/health"), Route::MethodNotAllowed);
    }

    #[test]
    fn loopback_is_accepted() {
        assert!(bind_address("127.0.0.1", DEFAULT_PORT).is_ok());
        assert!(bind_address("::1", DEFAULT_PORT).is_ok());
    }

    /// The security argument, asserted rather than documented. The
    /// tailnet address is the important row: it is the one a reasonable
    /// person would assume is safe, and it is the one that would quietly
    /// bypass `tailscale serve`.
    #[test]
    fn every_routable_address_is_refused_including_a_tailnet_one() {
        for host in ["0.0.0.0", "192.168.1.10", "100.67.55.58", "::"] {
            let err = bind_address(host, DEFAULT_PORT)
                .expect_err("bound a routable address: the probe is now exposed");
            assert!(err.contains("loopback only"), "got {err}");
        }
    }

    #[test]
    fn a_host_that_is_not_an_address_is_refused_by_name() {
        let err = bind_address("localhost", DEFAULT_PORT).unwrap_err();
        assert!(err.contains("localhost"), "got {err}");
    }
}
