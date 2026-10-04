use anyhow::{bail, ensure, Context, Result};
use std::time::Duration;

/// Follow at most five redirects, validating each target before requesting it.
/// HTTPS can never be downgraded; HTTP requires explicit source opt-in.
#[cfg(test)]
pub fn get(initial: &str, allow_http: bool, timeout: Duration) -> Result<ureq::Response> {
    get_headers(initial, allow_http, timeout, &[])
}
#[cfg(test)]
pub fn get_headers(
    initial: &str,
    allow_http: bool,
    timeout: Duration,
    headers: &[(&str, &str)],
) -> Result<ureq::Response> {
    get_with_proxy(initial, allow_http, timeout, headers, &Default::default())
}
pub fn get_with_proxy(
    initial: &str,
    allow_http: bool,
    timeout: Duration,
    headers: &[(&str, &str)],
    proxy: &crate::model::ProxySettings,
) -> Result<ureq::Response> {
    get_with_proxy_cancel(
        initial,
        allow_http,
        timeout,
        headers,
        proxy,
        &Default::default(),
    )
}
// Blocking DNS, headers and body reads must not keep a cancelled job alive.
// Detached blocking calls are bounded independently from the two logical jobs.
static NETWORK_WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct NetworkPermit;
impl Drop for NetworkPermit {
    fn drop(&mut self) {
        NETWORK_WORKERS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}
pub(crate) fn interruptible<T: Send + 'static>(
    cancel: &crate::jobs::Cancellation,
    timeout: Duration,
    action: impl FnOnce(crate::jobs::Cancellation) -> Result<T> + Send + 'static,
) -> Result<T> {
    use std::sync::{atomic::Ordering, mpsc};
    cancel.check()?;
    let mut workers = NETWORK_WORKERS.load(Ordering::Acquire);
    loop {
        ensure!(workers < 32, "网络连接正在清理，请稍后重试");
        match NETWORK_WORKERS.compare_exchange_weak(
            workers,
            workers + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(actual) => workers = actual,
        }
    }
    let permit = NetworkPermit;
    let token = cancel.child();
    let worker_token = token.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    struct Stop(crate::jobs::Cancellation);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.stop();
        }
    }
    let _stop = Stop(token);
    std::thread::Builder::new()
        .name("framely-network".into())
        .spawn(move || {
            let _permit = permit;
            let _ = tx.send(action(worker_token));
        })?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        cancel.check()?;
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        ensure!(!remaining.is_zero(), "网络请求超时");
        match rx.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => bail!("网络请求任务异常"),
        }
    }
}
pub fn get_with_proxy_cancel(
    initial: &str,
    allow_http: bool,
    timeout: Duration,
    headers: &[(&str, &str)],
    proxy: &crate::model::ProxySettings,
    cancel: &crate::jobs::Cancellation,
) -> Result<ureq::Response> {
    let initial = initial.to_owned();
    let proxy = proxy.clone();
    let headers: Vec<_> = headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    interruptible(cancel, timeout, move |token| {
        let refs: Vec<_> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        get_blocking(&initial, allow_http, timeout, &refs, &proxy, &token)
    })
}
fn get_blocking(
    initial: &str,
    allow_http: bool,
    timeout: Duration,
    headers: &[(&str, &str)],
    proxy: &crate::model::ProxySettings,
    cancel: &crate::jobs::Cancellation,
) -> Result<ureq::Response> {
    cancel.check()?;
    proxy.validate()?;
    crate::model::validate_url(initial, allow_http)?;
    let mut builder = ureq::AgentBuilder::new()
        .timeout(timeout)
        .redirects(0)
        .try_proxy_from_env(false);
    if proxy.http_enabled {
        builder = builder.proxy(ureq::Proxy::new(&proxy.http)?);
    }
    let agent = builder.build();
    let mut current = url::Url::parse(initial)?;
    for hop in 0..=5 {
        cancel.check()?;
        let routed = routed_url(&current, proxy);
        let mut request = agent.get(&routed);
        for (key, value) in headers {
            request = request.set(key, value);
        }
        let response = request.call()?;
        cancel.check()?;
        if !matches!(response.status(), 301 | 302 | 303 | 307 | 308) {
            ensure!(
                (200..300).contains(&response.status()) || response.status() == 304,
                "Download HTTP status {}",
                response.status()
            );
            return Ok(response);
        }
        ensure!(hop < 5, "Too many download redirects");
        let next = url::Url::parse(&routed)?.join(
            response
                .header("Location")
                .context("Redirect missing Location")?,
        )?;
        crate::model::validate_url(next.as_str(), allow_http)?;
        ensure!(
            current.scheme() != "https" || next.scheme() == "https",
            "HTTPS redirect downgrade rejected"
        );
        current = next;
    }
    bail!("Too many download redirects")
}

pub fn read_cancel(
    response: ureq::Response,
    limit: usize,
    cancel: &crate::jobs::Cancellation,
) -> Result<Vec<u8>> {
    interruptible(cancel, Duration::from_secs(30), move |token| {
        use std::io::Read;
        let mut reader = response.into_reader();
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 65536];
        loop {
            token.check()?;
            let n = reader.read(&mut buffer)?;
            token.check()?;
            if n == 0 {
                break;
            }
            ensure!(bytes.len() + n <= limit, "Download exceeds size limit");
            bytes.extend_from_slice(&buffer[..n]);
        }
        Ok(bytes)
    })
}

fn routed_url(url: &url::Url, proxy: &crate::model::ProxySettings) -> String {
    let host = url.host_str().unwrap_or("");
    let github = host == "github.com"
        || host.ends_with(".github.com")
        || host == "githubusercontent.com"
        || host.ends_with(".githubusercontent.com");
    if github
        && proxy.github_enabled
        && !url
            .as_str()
            .starts_with(&format!("{}/", proxy.github.trim_end_matches('/')))
    {
        format!("{}/{}", proxy.github.trim_end_matches('/'), url)
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redirect_download_retains_integrity_and_bounds() {
        use tiny_http::{Header, Response, Server};
        let server = Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.server_addr());
        let worker = std::thread::spawn(move || {
            for _ in 0..10 {
                let request = server.recv().unwrap();
                let response = match request.url() {
                    "/start" => Response::from_string("")
                        .with_status_code(302)
                        .with_header(Header::from_bytes("Location", "/package").unwrap()),
                    "/package" => Response::from_string("payload"),
                    "/invalid" => Response::from_string("")
                        .with_status_code(302)
                        .with_header(Header::from_bytes("Location", "file:///etc/passwd").unwrap()),
                    _ => Response::from_string("")
                        .with_status_code(302)
                        .with_header(Header::from_bytes("Location", "/loop").unwrap()),
                };
                request.respond(response).unwrap();
            }
        });
        assert!(get(&format!("{base}/start"), false, Duration::from_secs(2)).is_err());
        let response = get(&format!("{base}/start"), true, Duration::from_secs(2)).unwrap();
        assert_eq!(response.into_string().unwrap(), "payload");
        let params = serde_json::json!({"url":format!("{base}/package"),"allowHttp":true,"sha256":"0".repeat(64)});
        assert!(crate::package::from_request(&params).is_err());
        assert!(get(&format!("{base}/invalid"), true, Duration::from_secs(2)).is_err());
        assert!(get(&format!("{base}/loop"), true, Duration::from_secs(2)).is_err());
        worker.join().unwrap();
    }
}
#[cfg(test)]
mod conditional_tests {
    use super::*;
    #[test]
    fn conditional_headers_and_not_modified_are_preserved() {
        use tiny_http::{Response, Server};
        let server = Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}/subscription.json", server.server_addr());
        let worker = std::thread::spawn(move || {
            let request = server.recv().unwrap();
            assert!(request
                .headers()
                .iter()
                .any(|h| h.field.equiv("If-None-Match") && h.value.as_str() == "fixture"));
            request.respond(Response::empty(304)).unwrap();
        });
        assert_eq!(
            get_headers(
                &url,
                true,
                Duration::from_secs(2),
                &[("If-None-Match", "fixture")]
            )
            .unwrap()
            .status(),
            304
        );
        worker.join().unwrap();
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;
    use crate::model::ProxySettings;
    #[test]
    fn proxy_routing_and_validation() {
        let proxy = ProxySettings {
            http_enabled: false,
            github_enabled: true,
            http: String::new(),
            github: "https://gh-proxy.com".into(),
        };
        for host in [
            "github.com",
            "api.github.com",
            "raw.githubusercontent.com",
            "release-assets.githubusercontent.com",
        ] {
            let url = url::Url::parse(&format!("https://{host}/path?q=1")).unwrap();
            assert_eq!(
                routed_url(&url, &proxy),
                format!("https://gh-proxy.com/{url}")
            );
        }
        for value in [
            "https://github.com.evil.test/path",
            "https://example.org/path",
            "https://gh-proxy.com/https://github.com/path",
        ] {
            assert_eq!(routed_url(&url::Url::parse(value).unwrap(), &proxy), value);
        }
        let disabled = ProxySettings {
            github_enabled: false,
            ..proxy.clone()
        };
        let original = url::Url::parse("https://github.com/user/repo").unwrap();
        assert_eq!(routed_url(&original, &disabled), original.as_str());
        for value in [
            "socks5://localhost:1080",
            "http://user:password@localhost:8080",
            "http://localhost:8080/path",
        ] {
            assert!(ProxySettings {
                http: value.into(),
                github: String::new(),
                ..Default::default()
            }
            .validate()
            .is_err());
        }
    }
    #[test]
    fn http_proxy_receives_download_request() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let proxy = ProxySettings {
            http_enabled: true,
            github_enabled: false,
            http: format!("http://{}", server.server_addr()),
            github: String::new(),
        };
        let worker = std::thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
            assert_eq!(request.url(), "http://unresolvable.example/plugin");
            request
                .respond(tiny_http::Response::from_string("via proxy"))
                .unwrap();
        });
        let result = get_with_proxy(
            "http://unresolvable.example/plugin",
            true,
            Duration::from_secs(2),
            &[],
            &proxy,
        )
        .unwrap();
        assert_eq!(result.into_string().unwrap(), "via proxy");
        worker.join().unwrap();
    }
    #[test]
    fn disabled_http_proxy_keeps_address_but_connects_directly() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let target = format!("http://{}/direct", server.server_addr());
        let proxy = ProxySettings {
            http: "http://127.0.0.1:1".into(),
            ..Default::default()
        };
        let worker = std::thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
            assert_eq!(request.url(), "/direct");
            request
                .respond(tiny_http::Response::from_string("direct"))
                .unwrap();
        });
        assert_eq!(
            get_with_proxy(&target, true, Duration::from_secs(2), &[], &proxy)
                .unwrap()
                .into_string()
                .unwrap(),
            "direct"
        );
        worker.join().unwrap();
    }
    #[test]
    fn github_gateway_receives_original_url_and_preserves_headers() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let proxy = ProxySettings {
            http_enabled: false,
            github_enabled: true,
            http: String::new(),
            github: format!("http://{}", server.server_addr()),
        };
        let worker = std::thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
            assert_eq!(
                request.url(),
                "/https://raw.githubusercontent.com/user/repo/main/catalog.json"
            );
            assert!(request
                .headers()
                .iter()
                .any(|h| h.field.equiv("If-None-Match") && h.value.as_str() == "fixture"));
            request.respond(tiny_http::Response::empty(304)).unwrap();
        });
        assert_eq!(
            get_with_proxy(
                "https://raw.githubusercontent.com/user/repo/main/catalog.json",
                false,
                Duration::from_secs(2),
                &[("If-None-Match", "fixture")],
                &proxy
            )
            .unwrap()
            .status(),
            304
        );
        worker.join().unwrap();
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use std::sync::mpsc;
    #[test]
    fn stalled_headers_cancel_promptly_and_release_job_slots() {
        use tiny_http::{Response, Server};
        let server = Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}/wait", server.server_addr());
        let (seen_tx, seen_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server_thread = std::thread::spawn(move || {
            let req = server.recv().unwrap();
            seen_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            let _ = req.respond(Response::from_string("done"));
        });
        let jobs = crate::jobs::Jobs::default();
        let task = jobs
            .task("inspect", move |cancel| {
                get_with_proxy_cancel(
                    &url,
                    true,
                    Duration::from_secs(2),
                    &[],
                    &Default::default(),
                    &cancel,
                )?;
                Ok(serde_json::json!(true))
            })
            .unwrap();
        let _other = jobs
            .task("inspect", |_| {
                std::thread::sleep(Duration::from_millis(600));
                Ok(serde_json::json!(true))
            })
            .unwrap();
        seen_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let start = std::time::Instant::now();
        jobs.cancel(task["job"].as_str().unwrap()).unwrap();
        let replacement = loop {
            if let Ok(job) = jobs.task("catalog:after-cancel", |_| Ok(serde_json::json!(true))) {
                break job;
            }
            assert!(start.elapsed() < Duration::from_millis(500));
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(replacement["job"].is_string());
        assert_eq!(
            jobs.status(task["job"].as_str().unwrap()).unwrap()["phase"],
            "cancelled"
        );
        release_tx.send(()).unwrap();
        server_thread.join().unwrap();
    }
    #[test]
    fn stalled_body_cancel_and_deadline_are_bounded() {
        use std::{io::Write, net::TcpListener};
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/body", server.local_addr().unwrap());
        let (release_tx, release_rx) = mpsc::channel();
        let server_thread = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n")
                .unwrap();
            stream.flush().unwrap();
            release_rx.recv().unwrap();
            let _ = stream.write_all(b"hello");
        });
        let response = get(&url, true, Duration::from_secs(2)).unwrap();
        let token = crate::jobs::Cancellation::default();
        let worker_token = token.clone();
        let worker = std::thread::spawn(move || read_cancel(response, 100, &worker_token));
        std::thread::sleep(Duration::from_millis(30));
        let start = std::time::Instant::now();
        token.stop();
        assert!(worker.join().unwrap().is_err());
        assert!(start.elapsed() < Duration::from_millis(500));
        release_tx.send(()).unwrap();
        server_thread.join().unwrap();
        let start = std::time::Instant::now();
        assert!(
            interruptible(&Default::default(), Duration::from_millis(30), |_| {
                std::thread::sleep(Duration::from_millis(200));
                Ok(())
            })
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_millis(500));
    }
}
