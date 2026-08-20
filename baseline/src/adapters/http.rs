//! The HTTP seam. Everything above this file works against
//! `HttpTransport`, which means the GitHub adapter is exercised entirely
//! against recorded fixtures — no network in any test.
//!
//! Reads and writes share one seam deliberately. A control action is
//! only testable if a test can state exactly what would have been sent
//! without sending it, and that is what `FixtureTransport` records.

use super::AdapterError;
use std::collections::HashMap;
use std::path::Path;

/// What a request does. Reads are conditional; writes never are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Method {
    /// Read.
    #[default]
    Get,
    /// Create, or act on, a subresource.
    Post,
    /// Replace, or perform, an operation named by the URL.
    Put,
    /// Amend part of a resource.
    Patch,
    /// Remove.
    Delete,
}

impl Method {
    /// The verb as it goes on the wire.
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }

    /// Whether this method changes anything at the far end.
    pub fn is_write(&self) -> bool {
        !matches!(self, Method::Get)
    }
}

/// One request: a conditional read, or a write carrying a body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpRequest {
    /// What to do.
    pub method: Method,
    /// The absolute URL.
    pub url: String,
    /// The ETag from the last successful response, when there was one.
    /// Meaningless on a write, and never sent on one.
    pub etag: Option<String>,
    /// The request body, on a write.
    pub body: Option<String>,
}

impl HttpRequest {
    /// An unconditional read.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            url: url.into(),
            etag: None,
            body: None,
        }
    }

    /// A read that will settle for a `304`.
    pub fn conditional(url: impl Into<String>, etag: Option<String>) -> Self {
        Self {
            etag,
            ..Self::get(url)
        }
    }

    /// A write.
    pub fn write(method: Method, url: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            etag: None,
            body: Some(body.into()),
        }
    }
}

/// What a conditional GET returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpResponse {
    /// A body, plus the ETag to send next time.
    Ok {
        /// The response body.
        body: String,
        /// The response's ETag, when it carried one.
        etag: Option<String>,
    },
    /// `304`: the value the caller already holds is still current.
    NotModified,
}

/// Something that can perform an HTTP request.
pub trait HttpTransport {
    /// Performs the request.
    fn send(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError>;
}

/// A boxed transport is a transport.
///
/// Needed because a caller holding several sources of different concrete
/// types — the live one in a running cockpit, a recorded one in fixture
/// mode — has to put them in one collection, and the generic parameter
/// cannot be two things at once.
impl<T: HttpTransport + ?Sized> HttpTransport for Box<T> {
    fn send(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError> {
        (**self).send(request)
    }
}

/// The live transport. **The only type in this crate that touches the
/// network**, and therefore the only one exempt from automated testing
/// under the real-external-service precedent. It holds no logic beyond
/// mapping a status code onto an `AdapterError` — keep it that way.
pub struct UreqTransport {
    agent: ureq::Agent,
    token: Option<String>,
}

impl UreqTransport {
    /// An unauthenticated transport.
    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new().build(),
            token: None,
        }
    }

    /// A transport sending `Authorization: Bearer <token>`.
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            ..Self::new()
        }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpTransport for UreqTransport {
    fn send(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError> {
        let mut req = self
            .agent
            .request(request.method.as_str(), &request.url)
            .set("Accept", "application/vnd.github+json");
        if let Some(token) = &self.token {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
        if let Some(etag) = &request.etag {
            req = req.set("If-None-Match", etag);
        }
        let sent = match &request.body {
            Some(body) => req.send_string(body),
            None => req.call(),
        };
        match sent {
            Ok(resp) => {
                let etag = resp.header("ETag").map(str::to_string);
                let body = resp
                    .into_string()
                    .map_err(|e| AdapterError::Parse(e.to_string()))?;
                Ok(HttpResponse::Ok { body, etag })
            }
            Err(ureq::Error::Status(304, _)) => Ok(HttpResponse::NotModified),
            Err(ureq::Error::Status(status, resp)) => Err(AdapterError::Http {
                status,
                message: resp.into_string().unwrap_or_default().trim().to_string(),
            }),
            Err(ureq::Error::Transport(t)) => Err(AdapterError::Timeout(t.to_string())),
        }
    }
}

/// A transport that replays recorded responses. Public because
/// integration tests reach only the public API — and because a frontend
/// demoing the cockpit wants one too.
#[derive(Debug, Default)]
pub struct FixtureTransport {
    responses: HashMap<String, (String, Option<String>)>,
    requests: Vec<HttpRequest>,
    next_error: Option<AdapterError>,
}

impl FixtureTransport {
    /// An empty transport; every URL 404s until inserted.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the response for a URL.
    pub fn insert(&mut self, url: impl Into<String>, body: impl Into<String>, etag: Option<&str>) {
        self.responses
            .insert(url.into(), (body.into(), etag.map(str::to_string)));
    }

    /// Records the response for a URL from a fixture file on disk.
    pub fn insert_from_file(
        &mut self,
        url: impl Into<String>,
        path: &Path,
        etag: Option<&str>,
    ) -> std::io::Result<()> {
        let body = std::fs::read_to_string(path)?;
        self.insert(url, body, etag);
        Ok(())
    }

    /// Every request this transport was asked to perform, in order.
    pub fn requests(&self) -> &[HttpRequest] {
        &self.requests
    }

    /// Every write this transport was asked to perform, in order —
    /// method, URL and body, which is exactly what a test needs to
    /// assert about a control action it must not actually perform.
    pub fn writes(&self) -> Vec<&HttpRequest> {
        self.requests
            .iter()
            .filter(|r| r.method.is_write())
            .collect()
    }

    /// Records what a write returns. Only needed when the caller reads
    /// the response; an unregistered write succeeds with `{}`.
    pub fn insert_write(
        &mut self,
        method: Method,
        url: impl Into<String>,
        body: impl Into<String>,
    ) {
        self.responses
            .insert(write_key(method, &url.into()), (body.into(), None));
    }

    /// Makes the next request fail with `error`, once.
    pub fn fail_next(&mut self, error: AdapterError) {
        self.next_error = Some(error);
    }
}

/// Writes are keyed by method as well as URL: `POST /pulls/12/merge`
/// and `PUT /pulls/12/merge` are different acts on the same path.
fn write_key(method: Method, url: &str) -> String {
    format!("{} {url}", method.as_str())
}

impl HttpTransport for FixtureTransport {
    fn send(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError> {
        self.requests.push(request.clone());
        if let Some(e) = self.next_error.take() {
            return Err(e);
        }
        // A write is recorded and acknowledged. Its point in a test is
        // what was sent, not what came back — and requiring every
        // control test to register a canned response would make the
        // assertion about the fixture rather than about the caller.
        if request.method.is_write() {
            let body = self
                .responses
                .get(&write_key(request.method, &request.url))
                .map(|(b, _)| b.clone())
                .unwrap_or_else(|| "{}".to_string());
            return Ok(HttpResponse::Ok { body, etag: None });
        }
        match self.responses.get(&request.url) {
            None => Err(AdapterError::Http {
                status: 404,
                message: request.url.clone(),
            }),
            Some((body, etag)) => {
                if request.etag.is_some() && request.etag == *etag {
                    Ok(HttpResponse::NotModified)
                } else {
                    Ok(HttpResponse::Ok {
                        body: body.clone(),
                        etag: etag.clone(),
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport() -> FixtureTransport {
        let mut t = FixtureTransport::new();
        t.insert(
            "https://api.github.com/repos/a/b/issues",
            r#"[{"number":1}]"#,
            Some("W/\"abc\""),
        );
        t
    }

    #[test]
    fn a_request_without_an_etag_gets_the_body_and_the_current_etag() {
        let mut t = transport();
        let r = t
            .send(&HttpRequest::get("https://api.github.com/repos/a/b/issues"))
            .unwrap();
        match r {
            HttpResponse::Ok { body, etag } => {
                assert!(body.contains("\"number\":1"));
                assert_eq!(etag.as_deref(), Some("W/\"abc\""));
            }
            HttpResponse::NotModified => panic!("a first request cannot be NotModified"),
        }
    }

    #[test]
    fn a_request_carrying_the_matching_etag_gets_not_modified() {
        let mut t = transport();
        let r = t
            .send(&HttpRequest::conditional(
                "https://api.github.com/repos/a/b/issues",
                Some("W/\"abc\"".into()),
            ))
            .unwrap();
        assert_eq!(r, HttpResponse::NotModified);
    }

    #[test]
    fn a_request_carrying_a_stale_etag_gets_the_new_body() {
        let mut t = transport();
        let r = t
            .send(&HttpRequest::conditional(
                "https://api.github.com/repos/a/b/issues",
                Some("W/\"old\"".into()),
            ))
            .unwrap();
        assert!(matches!(r, HttpResponse::Ok { .. }));
    }

    #[test]
    fn an_unknown_url_is_a_404_rather_than_a_panic() {
        let mut t = transport();
        let e = t
            .send(&HttpRequest::get("https://api.github.com/nope"))
            .unwrap_err();
        assert!(matches!(e, AdapterError::Http { status: 404, .. }));
    }

    #[test]
    fn every_request_is_recorded_so_a_test_can_assert_conditionality() {
        let mut t = transport();
        let url = "https://api.github.com/repos/a/b/issues";
        let _ = t.send(&HttpRequest::get(url));
        let _ = t.send(&HttpRequest::conditional(url, Some("W/\"abc\"".into())));
        assert_eq!(t.requests().len(), 2);
        assert_eq!(t.requests()[0].etag, None);
        assert_eq!(t.requests()[1].etag.as_deref(), Some("W/\"abc\""));
    }

    #[test]
    fn fail_next_injects_one_error_and_then_behaves_normally() {
        let mut t = transport();
        t.fail_next(AdapterError::Http {
            status: 403,
            message: "rate limit exceeded".into(),
        });
        let url = "https://api.github.com/repos/a/b/issues";
        assert!(t.send(&HttpRequest::get(url)).is_err());
        assert!(t.send(&HttpRequest::get(url)).is_ok());
    }

    #[test]
    fn a_fixture_transport_is_usable_as_a_trait_object() {
        let mut boxed: Box<dyn HttpTransport> = Box::new(transport());
        assert!(boxed
            .send(&HttpRequest::get("https://api.github.com/repos/a/b/issues"))
            .is_ok());
    }
}
