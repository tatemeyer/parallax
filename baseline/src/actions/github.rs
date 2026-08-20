//! `WorkControl` against the GitHub API.
//!
//! Everything here is request construction: which verb, which URL, what
//! body. It performs no decision — `authorize` has already run by the
//! time anything in this file is called, and re-checking here would put
//! the confirmation contract in two places.

use crate::actions::executor::WorkControl;
use crate::adapters::http::{HttpRequest, HttpTransport, Method};
use crate::adapters::AdapterError;

/// Performs work-item actions against `api.github.com`.
///
/// Holds the same transport family the work adapter reads through, so
/// one token configures both and a test asserts the write without
/// performing it.
pub struct GithubWorkControl<T: HttpTransport> {
    transport: T,
    api_base: String,
}

impl<T: HttpTransport> GithubWorkControl<T> {
    /// Control against the public API.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            api_base: "https://api.github.com".to_string(),
        }
    }

    /// Control against a different host — GitHub Enterprise, or a test.
    pub fn with_api_base(transport: T, api_base: impl Into<String>) -> Self {
        Self {
            transport,
            api_base: api_base.into(),
        }
    }

    /// The transport, for asserting what was sent.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    fn send(&mut self, request: HttpRequest) -> Result<(), AdapterError> {
        self.transport.send(&request).map(|_| ())
    }
}

impl<T: HttpTransport> WorkControl for GithubWorkControl<T> {
    fn set_label(&mut self, repo: &str, item: u64, label: &str) -> Result<(), AdapterError> {
        // Adding to the labels subresource rather than PATCHing the
        // issue: a PATCH replaces the whole set, so it would silently
        // strip every label the cockpit did not know about.
        let url = format!("{}/repos/{repo}/issues/{item}/labels", self.api_base);
        let body = format!(r#"{{"labels":[{}]}}"#, json_string(label));
        self.send(HttpRequest::write(Method::Post, url, body))
    }

    fn request_review(&mut self, repo: &str, item: u64) -> Result<(), AdapterError> {
        // No reviewers named: the platform does not know who reviews
        // this project, and inventing a name would be worse than asking
        // GitHub to apply the repository's own rules.
        let url = format!(
            "{}/repos/{repo}/pulls/{item}/requested_reviewers",
            self.api_base
        );
        self.send(HttpRequest::write(Method::Post, url, "{}"))
    }

    fn merge(&mut self, repo: &str, number: u64) -> Result<(), AdapterError> {
        // Squash, matching every merge this platform's projects perform
        // by hand — see `git-github-standards.md`, which requires it so
        // `main` keeps one commit per Arc.
        let url = format!("{}/repos/{repo}/pulls/{number}/merge", self.api_base);
        self.send(HttpRequest::write(
            Method::Put,
            url,
            r#"{"merge_method":"squash"}"#,
        ))
    }
}

/// One JSON string literal, escaped by the same serialiser the rest of
/// the crate reads with. Hand-rolling this was tried and was a mistake:
/// an escaping bug in a write path is invisible until it reaches
/// GitHub, which is the one place there is no test.
fn json_string(s: &str) -> String {
    serde_json::to_string(s).expect("a string always serialises")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::http::FixtureTransport;

    fn control() -> GithubWorkControl<FixtureTransport> {
        GithubWorkControl::new(FixtureTransport::new())
    }

    #[test]
    fn setting_a_label_posts_to_the_labels_subresource() {
        let mut c = control();
        c.set_label("tatemeyer/ttui", 170, "semver:minor").unwrap();

        let writes = c.transport().writes();
        assert_eq!(writes.len(), 1, "one action, one request");
        assert_eq!(writes[0].method, Method::Post);
        assert_eq!(
            writes[0].url,
            "https://api.github.com/repos/tatemeyer/ttui/issues/170/labels"
        );
        assert_eq!(
            writes[0].body.as_deref(),
            Some(r#"{"labels":["semver:minor"]}"#)
        );
    }

    /// A PATCH on the issue would replace the label set, dropping every
    /// label the cockpit never saw. The subresource adds.
    #[test]
    fn setting_a_label_never_replaces_the_label_set() {
        let mut c = control();
        c.set_label("a/b", 1, "x").unwrap();
        assert_ne!(c.transport().writes()[0].method, Method::Patch);
    }

    #[test]
    fn requesting_a_review_posts_to_requested_reviewers() {
        let mut c = control();
        c.request_review("tatemeyer/parallax", 36).unwrap();

        let writes = c.transport().writes();
        assert_eq!(writes[0].method, Method::Post);
        assert_eq!(
            writes[0].url,
            "https://api.github.com/repos/tatemeyer/parallax/pulls/36/requested_reviewers"
        );
    }

    #[test]
    fn merging_puts_a_squash_to_the_merge_endpoint() {
        let mut c = control();
        c.merge("tatemeyer/parallax", 36).unwrap();

        let writes = c.transport().writes();
        assert_eq!(writes[0].method, Method::Put);
        assert_eq!(
            writes[0].url,
            "https://api.github.com/repos/tatemeyer/parallax/pulls/36/merge"
        );
        assert!(writes[0].body.as_deref().unwrap().contains("squash"));
    }

    /// A write is not a read: sending an ETag would invite a `304` on a
    /// request whose whole purpose is to change something.
    #[test]
    fn no_write_carries_an_etag() {
        let mut c = control();
        c.set_label("a/b", 1, "x").unwrap();
        c.request_review("a/b", 2).unwrap();
        c.merge("a/b", 3).unwrap();
        assert!(c.transport().writes().iter().all(|w| w.etag.is_none()));
    }

    #[test]
    fn a_label_with_a_quote_in_it_does_not_break_the_body() {
        let mut c = control();
        c.set_label("a/b", 1, r#"needs "intent""#).unwrap();
        assert_eq!(
            c.transport().writes()[0].body.as_deref(),
            Some(r#"{"labels":["needs \"intent\""]}"#)
        );
    }

    /// The transport's failures are the caller's failures. An action
    /// that could not be performed must not report that it was.
    #[test]
    fn a_rejected_write_is_an_error() {
        let mut transport = FixtureTransport::new();
        transport.fail_next(AdapterError::Http {
            status: 403,
            message: "resource not accessible by integration".into(),
        });
        let mut c = GithubWorkControl::new(transport);
        assert!(c.merge("a/b", 1).is_err());
    }
}
