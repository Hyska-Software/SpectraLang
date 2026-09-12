//! Injected HTTP transport for providers (plan adaptation 12).
//!
//! `spectra-api` depends on `spectra-agent` (aggregation), so this crate must
//! not depend on the API client. The transport is a process-global slot
//! installed once by whoever owns a real HTTP stack: `spectra-api` installs
//! its client adapter at registration time, and tests install a fake.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

/// The transport's view of one HTTP response: a status code and a UTF-8 body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportResponse {
    pub status: i64,
    pub body: String,
}

/// Minimal JSON-over-HTTP capability the providers need.
///
/// Implementations must apply their own TLS, pooling and SSRF policy; the
/// agent layer never bypasses them.
pub trait HttpTransport: Send + Sync + 'static {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<TransportResponse, String>;
}

fn slot() -> &'static Mutex<Option<Arc<dyn HttpTransport>>> {
    static SLOT: LazyLock<Mutex<Option<Arc<dyn HttpTransport>>>> =
        LazyLock::new(|| Mutex::new(None));
    &SLOT
}

fn lock() -> MutexGuard<'static, Option<Arc<dyn HttpTransport>>> {
    slot().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Installs the process-global transport.
///
/// Registration is idempotent: the first installation wins, so a later
/// `register()` call cannot silently swap the HTTP stack under a live process.
/// Returns `true` when this call installed the transport.
pub fn set_http_transport<T: HttpTransport>(transport: T) -> bool {
    let mut guard = lock();
    if guard.is_some() {
        return false;
    }
    *guard = Some(Arc::new(transport));
    true
}

/// The installed transport, if any.
pub(crate) fn http_transport() -> Option<Arc<dyn HttpTransport>> {
    lock().clone()
}

/// Removes the installed transport (test support and explicit teardown).
pub fn clear_http_transport() -> bool {
    lock().take().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoTransport;

    impl HttpTransport for EchoTransport {
        fn post_json(
            &self,
            _url: &str,
            _headers: &[(String, String)],
            _body: &str,
        ) -> Result<TransportResponse, String> {
            Ok(TransportResponse {
                status: 200,
                body: "{}".to_string(),
            })
        }
    }

    #[test]
    fn installation_is_idempotent_and_clearable() {
        clear_http_transport();
        assert!(set_http_transport(EchoTransport));
        assert!(!set_http_transport(EchoTransport));
        assert!(http_transport().is_some());
        assert!(clear_http_transport());
        assert!(http_transport().is_none());
        assert!(!clear_http_transport());
    }
}
