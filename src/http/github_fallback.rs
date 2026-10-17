//! Shared public-GitHub fallback policy for the core and desktop installer.
use anyhow::Result;
use std::io;

pub const PREFIX: &str = "https://gh-proxy.com";
#[cfg(test)]
static TEST_ROUTES: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
#[cfg(test)]
pub(crate) fn test_url(url: &str) -> String {
    TEST_ROUTES
        .lock()
        .unwrap()
        .iter()
        .find(|(source, _)| source == url)
        .map(|(_, target)| target.clone())
        .unwrap_or_else(|| url.into())
}
#[cfg(test)]
pub(crate) struct TestRoutes(pub Vec<(String, String)>);
#[cfg(test)]
impl TestRoutes {
    pub fn new(routes: Vec<(String, String)>) -> Self {
        TEST_ROUTES.lock().unwrap().extend(routes.clone());
        Self(routes)
    }
}
#[cfg(test)]
impl Drop for TestRoutes {
    fn drop(&mut self) {
        TEST_ROUTES
            .lock()
            .unwrap()
            .retain(|(url, _)| !self.0.iter().any(|(source, _)| source == url));
    }
}
#[cfg(test)]
pub(crate) fn local_truncated_server() -> (String, std::thread::JoinHandle<()>) {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::{Duration, Instant},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let worker = std::thread::spawn(move || {
        for body in [b"old".as_slice(), b"new-data".as_slice()] {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("local fallback server accept failed: {error}"),
                }
                assert!(Instant::now() < deadline, "local fallback server timed out");
                std::thread::sleep(Duration::from_millis(5));
            };
            // Accepted sockets can inherit O_NONBLOCK on macOS. Wait for
            // the complete request even when its headers arrive separately.
            stream.set_nonblocking(false).unwrap();
            let request_deadline = Instant::now() + Duration::from_secs(2);
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let remaining = request_deadline
                    .checked_duration_since(Instant::now())
                    .expect("local fallback server request timed out");
                stream.set_read_timeout(Some(remaining)).unwrap();
                let received = match stream.read(&mut chunk) {
                    Ok(received) => received,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => panic!("local fallback server read failed: {error}"),
                };
                assert!(received > 0, "local fallback server request ended before headers");
                request.extend_from_slice(&chunk[..received]);
                assert!(request.len() <= 8192, "local fallback server request too large");
            }
            stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n")
                .unwrap();
            stream.write_all(body).unwrap();
        }
    });
    (address, worker)
}
pub fn github_host(host: &str) -> bool {
    matches!(
        host,
        "github.com"
            | "api.github.com"
            | "raw.githubusercontent.com"
            | "objects.githubusercontent.com"
            | "release-assets.githubusercontent.com"
    )
}
pub fn eligible(initial: &str, headers: &[(&str, &str)]) -> bool {
    let Ok(url) = url::Url::parse(initial) else {
        return false;
    };
    url.scheme() == "https" && url.host_str().is_some_and(github_host)
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty() && url.password().is_none()
        && url.fragment().is_none()
        // Only pagination and Framely's fixed public topic search, never arbitrary queries or signed asset addresses.
        && url.query_pairs().all(|(key, value)| {
            if matches!(key.as_ref(), "page" | "per_page") { return !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()); }
            url.host_str() == Some("api.github.com") && url.path() == "/search/repositories"
                && matches!((key.as_ref(), value.as_ref()), ("q", "topic:framely-plugin archived:false fork:false") | ("sort", "stars") | ("order", "desc"))
        })
        && headers.iter().all(|(key, _)| matches!(key.to_ascii_lowercase().as_str(), "accept" | "accept-encoding" | "user-agent" | "if-none-match" | "if-modified-since" | "cache-control"))
}
fn certificate_error(error: &(dyn std::error::Error + 'static)) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("certificate")
        || message.contains("certvalid")
        || message.contains("unknownissuer")
        || message.contains("invalid peer")
}
pub fn retryable(error: &anyhow::Error) -> bool {
    if error.chain().any(certificate_error) {
        return false;
    }
    if error.to_string() == "网络请求超时" {
        return true;
    }
    error.chain().any(|cause| {
        if let Some(error) = cause.downcast_ref::<ureq::Error>() {
            return match error {
                ureq::Error::Status(code, response) => {
                    matches!(code, 408 | 429 | 500..=599)
                        || *code == 403
                            && (response.header("X-RateLimit-Remaining") == Some("0")
                                || response.header("Retry-After").is_some())
                }
                ureq::Error::Transport(error) => matches!(
                    error.kind(),
                    ureq::ErrorKind::Dns
                        | ureq::ErrorKind::ConnectionFailed
                        | ureq::ErrorKind::Io
                        | ureq::ErrorKind::ProxyConnect
                ),
            };
        }
        cause.downcast_ref::<io::Error>().is_some_and(|e| {
            matches!(
                e.kind(),
                io::ErrorKind::TimedOut
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::UnexpectedEof
            )
        })
    })
}
// Error display from HTTP clients can contain signed redirect URLs. Do not
// surface those URLs when reporting both routes' errors.
pub fn diagnostic(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(|cause| {
            if let Some(ureq::Error::Status(code, _)) = cause.downcast_ref::<ureq::Error>() {
                return format!("HTTP {code}");
            }
            if let Some(ureq::Error::Transport(t)) = cause.downcast_ref::<ureq::Error>() {
                return format!("network {:?}", t.kind());
            }
            cause
                .to_string()
                .split_whitespace()
                .map(|word| {
                    if word.contains("://") {
                        "[URL hidden]"
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(": ")
}
pub fn run<T>(
    enabled: bool,
    initial: &str,
    headers: &[(&str, &str)],
    mut attempt: impl FnMut(bool) -> Result<T>,
) -> Result<T> {
    match attempt(false) {
        Err(first) if enabled && eligible(initial, headers) && retryable(&first) => {
            eprintln!("GitHub connection failed; trying gh-proxy.com once");
            attempt(true).map_err(|second| {
                anyhow::anyhow!(
                    "GitHub primary route failed: {}; gh-proxy.com fallback failed: {}",
                    diagnostic(&first),
                    diagnostic(&second)
                )
            })
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_fallback_server_waits_for_complete_fragmented_headers() {
        use std::{io::{Read, Write}, net::TcpStream, time::Duration};

        let (address, server) = local_truncated_server();
        for expected in [b"old".as_slice(), b"new-data".as_slice()] {
            let mut stream = TcpStream::connect(address.trim_start_matches("http://")).unwrap();
            // Leave time for accept before sending a partial request. The
            // server must wait for the final header delimiter before replying.
            std::thread::sleep(Duration::from_millis(25));
            stream.write_all(b"GET /asset HTTP/1.1\r\nHost: localhost\r\n").unwrap();
            stream.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
            let mut byte = [0u8; 1];
            let error = stream.read(&mut byte).expect_err("server replied before headers completed");
            assert!(matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut));
            stream.write_all(b"\r\n").unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).unwrap();
            let header_end = response.windows(4).position(|bytes| bytes == b"\r\n\r\n").unwrap() + 4;
            assert_eq!(&response[header_end..], expected);
        }
        server.join().unwrap();
    }
    #[test]
    fn github_fallback_only_public_exact_hosts_without_credentials() {
        assert!(eligible(
            "https://api.github.com/repos/a/b/releases?page=1&per_page=100",
            &[]
        ));
        assert!(eligible("https://api.github.com/search/repositories?q=topic%3Aframely-plugin+archived%3Afalse+fork%3Afalse&sort=stars&order=desc&per_page=100&page=1", &[]));
        assert!(!eligible("https://api.github.com/search/repositories?q=secret", &[]));
        assert!(!eligible("https://github.com/search/repositories?q=topic%3Aframely-plugin+archived%3Afalse+fork%3Afalse", &[]));
        for u in [
            "https://github.com.evil/a",
            "https://foo.github.com/a",
            "https://user@github.com/a",
            "https://github.com/a?token=secret",
            "https://release-assets.githubusercontent.com/a?X-Amz-Signature=secret",
            "https://gh-proxy.com/https://github.com/a",
            "http://github.com/a",
            "https://github.com:8443/a",
        ] {
            assert!(!eligible(u, &[]), "{u}");
        }
        for h in ["Authorization", "cookie", "X-API-KEY", "X-GitHub-Token"] {
            assert!(!eligible("https://github.com/a", &[(h, "secret")]));
        }
    }
    #[test]
    fn github_fallback_runs_once_and_does_not_retry_validation_or_certificates() {
        let mut calls = vec![];
        let result = run(true, "https://github.com/a", &[], |fallback| {
            calls.push(fallback);
            if fallback {
                Ok(7)
            } else {
                Err(io::Error::from(io::ErrorKind::ConnectionReset).into())
            }
        })
        .unwrap();
        assert_eq!(result, 7);
        assert_eq!(calls, vec![false, true]);
        for enabled in [false, true] {
            for message in [
                "Download SHA256 mismatch",
                "Operation cancelled",
                "invalid peer certificate",
            ] {
                let mut calls = 0;
                assert!(run::<()>(enabled, "https://github.com/a", &[], |_| {
                    calls += 1;
                    anyhow::bail!("{message}")
                })
                .is_err());
                assert_eq!(calls, 1);
            }
        }
        for (code, remaining, retry) in [
            (404, false, false),
            (403, false, false),
            (403, true, true),
            (429, false, true),
            (503, false, true),
        ] {
            let response: ureq::Response = format!(
                "HTTP/1.1 {code} Test\r\n{}\r\n",
                if remaining {
                    "X-RateLimit-Remaining: 0\r\n"
                } else {
                    ""
                }
            )
            .parse()
            .unwrap();
            assert_eq!(
                retryable(&ureq::Error::Status(code, response).into()),
                retry
            );
        }
    }
}
