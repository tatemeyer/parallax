//! Routes, and the one rule that keeps the probe safe.
//!
//! The probe has no authentication, no authorization, and no TLS. That
//! is not an omission — it is only defensible because nothing off this
//! machine can open a socket to it. `tailscale serve` terminates TLS
//! and forwards to the loopback port, and the tailnet decides who may
//! reach *that*. [`bind_address`] is where the claim is enforced.
//!
//! **Control is off unless it was asked for.** `/state` is a
//! disclosure; `POST /action` is a shell, and the two must not arrive
//! together by default. A probe started without control refuses the
//! write routes at the route table, before a body is read — see
//! [`Serving::control`].

use parallax_baseline::actions::wire::{ActionId, ActionRequest, SubmitReply};
use parallax_baseline::actions::ACTION_PATH;
use parallax_baseline::adapters::factory::AdapterConfig;
use parallax_baseline::registry::Registry;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::time::SystemTime;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::control::Control;
use crate::state::envelope;

/// The default loopback port.
pub const DEFAULT_PORT: u16 = 8737;

/// The most of a submission body the probe will read.
///
/// An action is a few hundred bytes. Reading without a bound would let
/// anything that can reach the socket spend this machine's memory, and
/// on the Pi that is the machine running the television.
const MAX_BODY: u64 = 64 * 1024;

/// What a request asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Everything this machine knows, aggregated.
    State,
    /// Liveness, without a scan.
    Health,
    /// Take an action on this machine.
    Submit,
    /// What became of an action this machine was asked to take.
    Status(String),
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
    if let Some(id) = path.strip_prefix(&format!("{ACTION_PATH}/")) {
        return match (*method == Method::Get, id.is_empty()) {
            (_, true) => Route::NotFound,
            (true, false) => Route::Status(id.to_string()),
            (false, false) => Route::MethodNotAllowed,
        };
    }
    match path {
        "/state" | "/health" if *method != Method::Get => Route::MethodNotAllowed,
        "/state" => Route::State,
        "/health" => Route::Health,
        ACTION_PATH if *method == Method::Post => Route::Submit,
        ACTION_PATH => Route::MethodNotAllowed,
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

/// Everything a probe needs to answer a request.
///
/// A struct rather than four parameters because `control` is the one
/// that must be readable at a glance in every call site: it is the
/// difference between a machine that can be looked at and a machine
/// that can be told what to do.
pub struct Serving<'a> {
    /// The projects this machine holds.
    pub registry: &'a Registry,
    /// How this machine's adapters are built.
    pub config: &'a AdapterConfig,
    /// How this machine names itself.
    pub peer: &'a str,
    /// The control surface — `None` unless control was asked for, and
    /// then no request can cause this machine to act.
    pub control: Option<&'a Control>,
}

/// What a probe says when asked to act and control is off.
///
/// **`403`, not `404`.** A client that got `404` could not tell "control
/// is off here" from "this probe is too old to have control at all", and
/// those call for different things from an operator.
fn control_is_off() -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string("control is not enabled on this probe; start it with --allow-control")
        .with_status_code(403)
}

/// Answers one request.
fn answer(mut request: Request, serving: &Serving) -> std::io::Result<()> {
    match route(request.method(), request.url()) {
        Route::State => {
            let envelope = envelope(
                serving.registry,
                serving.config,
                serving.peer,
                SystemTime::now(),
            );
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
        Route::Submit => {
            // Before the body is touched: a probe that does not do
            // control does not parse actions either.
            let Some(control) = serving.control else {
                return request.respond(control_is_off());
            };
            let mut body = String::new();
            if let Err(e) = request.as_reader().take(MAX_BODY).read_to_string(&mut body) {
                return request.respond(
                    Response::from_string(format!("could not read the request: {e}"))
                        .with_status_code(400),
                );
            }
            match serde_json::from_str::<ActionRequest>(&body) {
                // A body we cannot read is a `400`: this machine read it
                // and declined, so the caller knows nothing ran.
                Err(e) => request.respond(
                    Response::from_string(format!("could not read the action: {e}"))
                        .with_status_code(400),
                ),
                Ok(action) => {
                    let reply = control.submit(action);
                    // `202` for the one that will still happen, `200`
                    // for the one that is already over.
                    let status = match reply {
                        SubmitReply::Accepted { .. } => 202,
                        SubmitReply::Refused { .. } => 200,
                    };
                    match serde_json::to_string(&reply) {
                        Ok(body) => request.respond(
                            Response::from_string(body)
                                .with_header(json_header())
                                .with_status_code(status),
                        ),
                        Err(e) => request.respond(
                            Response::from_string(format!("could not serialize the reply: {e}"))
                                .with_status_code(500),
                        ),
                    }
                }
            }
        }
        Route::Status(id) => {
            let Some(control) = serving.control else {
                return request.respond(control_is_off());
            };
            let reply = control.status(&ActionId::new(id));
            match serde_json::to_string(&reply) {
                Ok(body) => request.respond(Response::from_string(body).with_header(json_header())),
                Err(e) => request.respond(
                    Response::from_string(format!("could not serialize the reply: {e}"))
                        .with_status_code(500),
                ),
            }
        }
        Route::NotFound => {
            request.respond(Response::from_string("not found").with_status_code(404))
        }
        Route::MethodNotAllowed => {
            request.respond(Response::from_string("method not allowed").with_status_code(405))
        }
    }
}

/// A bound probe.
///
/// Owns the listener so `tiny_http` does not appear in this crate's
/// public API — which keeps the HTTP library a private choice, and lets
/// an integration test bind a real port without taking a dependency on
/// whichever library happens to be behind it.
pub struct Probe {
    server: Server,
    addr: SocketAddr,
}

impl Probe {
    /// Binds loopback on `port`.
    ///
    /// Port `0` asks the operating system for a free one and is what a
    /// test wants — and what a second probe on one machine needs, which
    /// is how two peers get exercised without two machines.
    pub fn bind(port: u16) -> Result<Self, String> {
        let requested = bind_address("127.0.0.1", port)?;
        let server =
            Server::http(requested).map_err(|e| format!("could not bind {requested}: {e}"))?;
        // Re-read it rather than trusting `requested`: with port 0 the
        // interesting number is the one the OS chose.
        let addr = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| "listener has no IP address".to_string())?;
        Ok(Self { server, addr })
    }

    /// The address actually bound.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The base URL a client would fetch from.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Serves until the listener dies.
    pub fn serve(&self, serving: &Serving) {
        for request in self.server.incoming_requests() {
            // One bad connection is not a reason to stop serving the
            // other two machines.
            let _ = answer(request, serving);
        }
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

    /// The read routes stay read-only now that a write route exists
    /// beside them. A POST that fell through to `/state` would be a
    /// second control surface, and one nobody specified.
    #[test]
    fn a_known_path_asked_for_the_wrong_way_is_refused_rather_than_served() {
        assert_eq!(route(&Method::Post, "/state"), Route::MethodNotAllowed);
        assert_eq!(route(&Method::Delete, "/health"), Route::MethodNotAllowed);
        assert_eq!(route(&Method::Get, "/action"), Route::MethodNotAllowed);
        assert_eq!(
            route(&Method::Post, "/action/desktop-1-1"),
            Route::MethodNotAllowed
        );
    }

    #[test]
    fn the_control_routes_resolve() {
        assert_eq!(route(&Method::Post, "/action"), Route::Submit);
        assert_eq!(
            route(&Method::Get, "/action/desktop-1-1"),
            Route::Status("desktop-1-1".into())
        );
    }

    /// `/action/` names no action, and answering it as a status query
    /// for the empty id would invent one.
    #[test]
    fn an_action_path_with_no_id_is_not_a_status_query() {
        // Normalizes to `/action`, which a GET may not have.
        assert_eq!(route(&Method::Get, "/action/"), Route::MethodNotAllowed);
        assert_eq!(route(&Method::Post, "/action/"), Route::Submit);
        // An empty id is not an id.
        assert_eq!(route(&Method::Get, "/action//"), Route::NotFound);
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
    ///
    /// `100.64.0.1` stands for it — Tailscale allocates from
    /// `100.64.0.0/10`, so any address in that range is the case this
    /// row is about. A real one was used when this was verified on
    /// hardware; it is not checked in, because the repository is public.
    #[test]
    fn every_routable_address_is_refused_including_a_tailnet_one() {
        for host in ["0.0.0.0", "192.168.1.10", "100.64.0.1", "::"] {
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
