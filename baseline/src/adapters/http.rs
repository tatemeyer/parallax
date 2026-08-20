//! The HTTP seam. Everything above this file works against
//! `HttpTransport`, which means the GitHub adapter is exercised entirely
//! against recorded fixtures — no network in any test.

use super::AdapterError;
use std::collections::HashMap;
use std::path::Path;

/// A conditional GET.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// The absolute URL.
    pub url: String,
    /// The ETag from the last successful response, when there was one.
    pub etag: Option<String>,
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

/// Something that can perform a conditional GET.
pub trait HttpTransport {
    /// Performs the request.
    fn get(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError>;
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
    fn get(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError> {
        let mut req = self
            .agent
            .get(&request.url)
            .set("Accept", "application/vnd.github+json");
        if let Some(token) = &self.token {
            req = req.set("Authorization", &format!("Bearer {token}"));
        }
        if let Some(etag) = &request.etag {
            req = req.set("If-None-Match", etag);
        }
        match req.call() {
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

    /// Makes the next request fail with `error`, once.
    pub fn fail_next(&mut self, error: AdapterError) {
        self.next_error = Some(error);
    }
}

impl HttpTransport for FixtureTransport {
    fn get(&mut self, request: &HttpRequest) -> Result<HttpResponse, AdapterError> {
        self.requests.push(request.clone());
        if let Some(e) = self.next_error.take() {
            return Err(e);
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
            .get(&HttpRequest {
                url: "https://api.github.com/repos/a/b/issues".into(),
                etag: None,
            })
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
            .get(&HttpRequest {
                url: "https://api.github.com/repos/a/b/issues".into(),
                etag: Some("W/\"abc\"".into()),
            })
            .unwrap();
        assert_eq!(r, HttpResponse::NotModified);
    }

    #[test]
    fn a_request_carrying_a_stale_etag_gets_the_new_body() {
        let mut t = transport();
        let r = t
            .get(&HttpRequest {
                url: "https://api.github.com/repos/a/b/issues".into(),
                etag: Some("W/\"old\"".into()),
            })
            .unwrap();
        assert!(matches!(r, HttpResponse::Ok { .. }));
    }

    #[test]
    fn an_unknown_url_is_a_404_rather_than_a_panic() {
        let mut t = transport();
        let e = t
            .get(&HttpRequest {
                url: "https://api.github.com/nope".into(),
                etag: None,
            })
            .unwrap_err();
        assert!(matches!(e, AdapterError::Http { status: 404, .. }));
    }

    #[test]
    fn every_request_is_recorded_so_a_test_can_assert_conditionality() {
        let mut t = transport();
        let url = "https://api.github.com/repos/a/b/issues";
        let _ = t.get(&HttpRequest {
            url: url.into(),
            etag: None,
        });
        let _ = t.get(&HttpRequest {
            url: url.into(),
            etag: Some("W/\"abc\"".into()),
        });
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
        assert!(t
            .get(&HttpRequest {
                url: url.into(),
                etag: None
            })
            .is_err());
        assert!(t
            .get(&HttpRequest {
                url: url.into(),
                etag: None
            })
            .is_ok());
    }

    #[test]
    fn a_fixture_transport_is_usable_as_a_trait_object() {
        let mut boxed: Box<dyn HttpTransport> = Box::new(transport());
        assert!(boxed
            .get(&HttpRequest {
                url: "https://api.github.com/repos/a/b/issues".into(),
                etag: None,
            })
            .is_ok());
    }
}
