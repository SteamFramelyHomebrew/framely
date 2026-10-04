use crate::{ipc, model::*};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

struct FrameAccess {
    plugin: String,
    version: String,
    network_key: String,
    expires: std::time::Instant,
}
struct Agent {
    socket: PathBuf,
    state: PathBuf,
    assets: PathBuf,
    origin: String,
    web_key: String,
    network_key: Mutex<String>,
    network_status: Mutex<Value>,
    frame_access: Mutex<std::collections::BTreeMap<String, FrameAccess>>,
    native_key: String,
    commands: Mutex<VecDeque<Value>>,
    events: Mutex<EventLog>,
    jobs: crate::jobs::Jobs,
    catalog_cache: Arc<Mutex<std::collections::BTreeMap<String, Value>>>,
}
#[derive(Default)]
struct EventLog {
    sequence: u64,
    entries: VecDeque<Value>,
}
impl EventLog {
    fn append(&mut self, mut event: Value) {
        self.sequence += 1;
        event["sequence"] = json!(self.sequence);
        if self.entries.len() >= 256 {
            self.entries.pop_front();
        }
        self.entries.push_back(event);
    }
    fn after(&self, cursor: u64) -> Value {
        json!({"events": self.entries.iter().filter(|e| e["sequence"].as_u64().unwrap_or(0) > cursor).collect::<Vec<_>>(), "cursor":self.sequence})
    }
}
fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).unwrap()
}
fn random_key() -> String {
    use rand::RngCore;
    let mut b = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}
fn push(q: &Mutex<VecDeque<Value>>, v: Value) {
    let mut q = q.lock().unwrap();
    if q.len() >= 256 {
        q.pop_front();
    }
    q.push_back(v);
}
impl Agent {
    fn core(&self, m: &str, p: Value) -> Result<Value> {
        ipc::call(&self.socket, m, p)
    }
    fn authorized(&self, r: &Request) -> bool {
        let cookie = r
            .headers()
            .iter()
            .find(|h| h.field.equiv("Cookie"))
            .map(|h| h.value.as_str())
            .unwrap_or("");
        cookie
            .split(';')
            .any(|c| c.trim() == format!("framely={}", self.web_key))
    }
    fn network_authorized(&self, r: &Request) -> bool {
        let key = self.network_key.lock().unwrap();
        r.headers()
            .iter()
            .find(|h| h.field.equiv("Cookie"))
            .is_some_and(|h| {
                h.value
                    .as_str()
                    .split(';')
                    .any(|c| c.trim() == format!("framely-network={key}"))
            })
    }
    fn native(&self, r: &Request) -> bool {
        r.headers()
            .iter()
            .any(|h| h.field.equiv("X-Framely-Native") && h.value.as_str() == self.native_key)
    }
    fn handle(&self, r: Request) -> Result<()> {
        self.handle_http(r, false)
    }
    fn handle_http(&self, mut r: Request, remote: bool) -> Result<()> {
        let path = r.url().split('?').next().unwrap_or("/").to_owned();
        let expected_origin = if remote {
            let host = r
                .headers()
                .iter()
                .find(|h| h.field.equiv("Host"))
                .context("Missing Host")?
                .value
                .as_str();
            let url = url::Url::parse(&format!("http://{host}"))?;
            ensure!(
                url.host_str()
                    .is_some_and(|h| h.parse::<std::net::IpAddr>().is_ok() || h == "localhost")
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.path() == "/"
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "Use the device IP address"
            );
            url.origin().ascii_serialization()
        } else {
            self.origin.clone()
        };
        if remote && (path.starts_with("/boot/") || path.starts_with("/host/")) {
            r.respond(Response::empty(StatusCode(403)))?;
            return Ok(());
        }
        if remote {
            if let Some(resource) = path.strip_prefix("/plugin-resources/") {
                let (token, file) = resource.split_once('/').context("Invalid frame resource")?;
                let (id, version) = {
                    let access = self.frame_access.lock().unwrap();
                    let grant = access.get(token).context("Frame resource expired")?;
                    ensure!(
                        grant.expires > std::time::Instant::now()
                            && grant.network_key == *self.network_key.lock().unwrap(),
                        "Frame resource expired"
                    );
                    (grant.plugin.clone(), grant.version.clone())
                };
                let status = self.core("status", json!({}))?;
                let plugin = &status["database"]["plugins"][&id];
                ensure!(
                    status["agreement"]["accepted"] == true
                        && plugin["enabled"] == true
                        && status["database"]["safeMode"] == false
                        && plugin["manifest"]["version"] == version,
                    "Plugin disabled or changed"
                );
                if file == "sdk.js" {
                    return send_file(r, &self.assets.join("assets/plugin-bootstrap.js"), true);
                }
                let file = file
                    .strip_prefix("plugin/")
                    .context("Invalid frame resource")?;
                safe_path(file)?;
                ensure!(
                    plugin["manifest"]["files"].get(file).is_some(),
                    "Unlisted asset"
                );
                return send_file(
                    r,
                    &self
                        .state
                        .join("plugins")
                        .join(id)
                        .join("versions")
                        .join(version)
                        .join(file),
                    true,
                );
            }
        }
        // Public, compile-time branding only. No other assets bypass network login.
        let branding: Option<(&[u8], &str)> = match path.as_str() {
            "/assets/branding/framely-mark-light.svg" => Some((
                include_bytes!("../assets/branding/framely-mark-light.svg"),
                "image/svg+xml",
            )),
            "/assets/branding/framely-logo-light.svg" => Some((
                include_bytes!("../assets/branding/framely-logo-light.svg"),
                "image/svg+xml",
            )),
            "/assets/branding/framely-app-icon.svg" => Some((
                include_bytes!("../assets/branding/framely-app-icon.svg"),
                "image/svg+xml",
            )),
            "/assets/branding/framely-app-icon.png" => Some((
                include_bytes!("../assets/branding/framely-app-icon.png"),
                "image/png",
            )),
            "/assets/branding/framely.ico" | "/favicon.ico" => Some((
                include_bytes!("../assets/branding/framely.ico"),
                "image/x-icon",
            )),
            _ => None,
        };
        if r.method() == &Method::Get {
            if let Some((data, mime)) = branding {
                r.respond(
                    Response::from_data(data)
                        .with_header(header("Content-Type", mime))
                        .with_header(header("X-Content-Type-Options", "nosniff"))
                        .with_header(header("Cache-Control", "no-cache")),
                )?;
                return Ok(());
            }
        }
        let configured = remote && self.core("network.password.configured", json!({}))? == true;
        if remote && (!configured || !self.network_authorized(&r)) {
            let status = self.core("status", json!({}))?;
            let protected = status["database"]["networkPanel"]["passwordEnabled"]
                .as_bool()
                .unwrap_or(false);
            if configured
                && !protected
                && r.method() == &Method::Get
                && (path == "/" || path == "/manager")
            {
                r.respond(
                    Response::empty(StatusCode(302))
                        .with_header(header("Location", "/manager"))
                        .with_header(header(
                            "Set-Cookie",
                            &format!(
                                "framely-network={}; HttpOnly; SameSite=Strict; Path=/",
                                self.network_key.lock().unwrap()
                            ),
                        )),
                )?;
                return Ok(());
            }
            if path == "/setup" && r.method() == &Method::Post {
                ensure!(
                    r.headers()
                        .iter()
                        .any(|h| h.field.equiv("Origin") && h.value.as_str() == expected_origin),
                    "Cross-origin setup denied"
                );
                let mut body = String::new();
                r.as_reader().take(8193).read_to_string(&mut body)?;
                ensure!(body.len() <= 8192, "Setup request too large");
                let fields: std::collections::BTreeMap<_, _> =
                    url::form_urlencoded::parse(body.as_bytes())
                        .into_owned()
                        .collect();
                match self.core("network.password.setup", json!({"password":fields.get("password"),"confirmPassword":fields.get("confirmPassword")})) {
                    Ok(_) => {
                        *self.network_key.lock().unwrap() = random_key();
                        r.respond(Response::empty(StatusCode(303)).with_header(header("Location", "/manager")))?;
                    }
                    Err(error) => {
                        r.respond(Response::from_string(format!("{error}。请返回重试。"))
                            .with_status_code(409)
                            .with_header(header("Content-Type", "text/plain; charset=utf-8")))?;
                    }
                }
                return Ok(());
            }
            if path == "/login" && r.method() == &Method::Post {
                if !configured {
                    r.respond(
                        Response::from_string("请先设置访问密码。")
                            .with_status_code(409)
                            .with_header(header("Content-Type", "text/plain; charset=utf-8")),
                    )?;
                    return Ok(());
                }
                ensure!(
                    r.headers()
                        .iter()
                        .any(|h| h.field.equiv("Origin") && h.value.as_str() == expected_origin),
                    "Cross-origin login denied"
                );
                let mut body = String::new();
                r.as_reader().take(8193).read_to_string(&mut body)?;
                ensure!(body.len() <= 8192, "Login request too large");
                let code = url::form_urlencoded::parse(body.as_bytes())
                    .find(|(k, _)| k == "password")
                    .map(|(_, v)| v.into_owned());
                let peer = r
                    .remote_addr()
                    .map(|a| a.ip().to_string())
                    .unwrap_or_else(|| "unknown".into());
                let key_before = self.network_key.lock().unwrap().clone();
                let verified = match self.core(
                    "network.password.verify",
                    json!({"password":code.unwrap_or_default(),"peer":peer}),
                ) {
                    Ok(value) => value.as_bool() == Some(true),
                    Err(error) if error.to_string().contains("Login rate limit exceeded") => {
                        r.respond(
                            Response::from_string("登录尝试过多，请稍后重试。")
                                .with_status_code(429)
                                .with_header(header("Retry-After", "60")),
                        )?;
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                };
                if verified && key_before == *self.network_key.lock().unwrap() {
                    r.respond(
                        Response::empty(StatusCode(302))
                            .with_header(header("Location", "/manager"))
                            .with_header(header(
                                "Set-Cookie",
                                &format!(
                                    "framely-network={}; HttpOnly; SameSite=Strict; Path=/",
                                    self.network_key.lock().unwrap()
                                ),
                            )),
                    )?;
                } else {
                    r.respond(
                        Response::from_string("访问密码不正确，请返回重试。")
                            .with_header(header("Content-Type", "text/plain; charset=utf-8"))
                            .with_status_code(401),
                    )?;
                }
                return Ok(());
            }
            if r.method() == &Method::Get && (path == "/" || path == "/manager") {
                r.respond(Response::from_string(if configured { NETWORK_LOGIN } else { NETWORK_SETUP }).with_header(header("Content-Type","text/html; charset=utf-8")).with_header(header("Content-Security-Policy","default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'")))?;
            } else {
                r.respond(
                    Response::empty(StatusCode(401))
                        .with_header(header("X-Framely-Reauthenticate", "1")),
                )?;
            }
            return Ok(());
        }
        if path == "/host/poll" {
            ensure!(self.native(&r), "Native authentication failed");
            let mut queue = self.commands.lock().unwrap();
            let mut commands = Vec::new();
            let mut size = 0;
            while let Some(command) = queue.front() {
                let n = serde_json::to_vec(command)?.len();
                if !commands.is_empty() && size + n > 3 * 1024 * 1024 {
                    break;
                }
                size += n;
                commands.push(queue.pop_front().unwrap());
            }
            drop(queue);
            return send_json(r, json!({"commands":commands}));
        }
        if let Some(rel) = path.strip_prefix("/plugin-assets/") {
            let (id, file) = rel.split_once('/').context("Invalid asset path")?;
            valid_id(id)?;
            safe_path(file)?;
            let status = self.core("status", json!({}))?;
            ensure!(
                status["agreement"]["accepted"] == true,
                "请先同意用户协议和隐私声明"
            );
            let p = status["database"]["plugins"][id].clone();
            ensure!(!p.is_null(), "Unknown plugin");
            let m: Manifest = serde_json::from_value(p["manifest"].clone())?;
            ensure!(m.files.contains_key(file), "Unlisted asset");
            let root = self
                .state
                .join("plugins")
                .join(id)
                .join("versions")
                .join(&m.version);
            return send_file(r, &root.join(file), true);
        }
        if let Some(rel) = path.strip_prefix("/plugin-frame/") {
            let parts: Vec<_> = rel.split('/').collect();
            ensure!(parts.len() == 2, "Invalid plugin frame");
            let id = parts[0];
            valid_id(id)?;
            let status = self.core("status", json!({}))?;
            ensure!(
                status["agreement"]["accepted"] == true,
                "请先同意用户协议和隐私声明"
            );
            let p = &status["database"]["plugins"][id];
            ensure!(
                p["enabled"] == true && status["database"]["safeMode"] == false,
                "Plugin disabled"
            );
            let m: Manifest = serde_json::from_value(p["manifest"].clone())?;
            let entry = if parts[1] == "quick" {
                m.ui.quick_page.clone().context("No quick page")?
            } else {
                m.ui.windows
                    .get(parts[1])
                    .context("Unknown window")?
                    .entry
                    .clone()
            };
            let resource_origin = &expected_origin;
            let (sdk, script, base) = if remote {
                let token = random_key();
                let mut grants = self.frame_access.lock().unwrap();
                grants.retain(|_, grant| grant.expires > std::time::Instant::now());
                if grants.len() >= 256 {
                    let oldest = grants
                        .iter()
                        .min_by_key(|(_, g)| g.expires)
                        .map(|(k, _)| k.clone())
                        .unwrap();
                    grants.remove(&oldest);
                }
                grants.insert(
                    token.clone(),
                    FrameAccess {
                        plugin: id.into(),
                        version: m.version.clone(),
                        network_key: self.network_key.lock().unwrap().clone(),
                        expires: std::time::Instant::now() + Duration::from_secs(24 * 60 * 60),
                    },
                );
                let prefix = format!("{resource_origin}/plugin-resources/{token}");
                (
                    format!("{prefix}/sdk.js"),
                    format!("{prefix}/plugin/{entry}"),
                    format!("{prefix}/plugin/"),
                )
            } else {
                (
                    format!("{resource_origin}/assets/plugin-bootstrap.js"),
                    format!("{resource_origin}/plugin-assets/{id}/{entry}"),
                    format!("{resource_origin}/plugin-assets/{id}/"),
                )
            };
            let html=format!("<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><base href='{base}'><div id=root></div><script src='{sdk}'></script><script src='{script}'></script>");
            let csp=format!("default-src 'none'; script-src {}; style-src 'unsafe-inline' {}; img-src {} data: blob: https: http:; connect-src https: http: ws: wss:; font-src {} data:; base-uri {}; form-action 'none'; frame-src 'none'",resource_origin,resource_origin,resource_origin,resource_origin,resource_origin);
            let response = Response::from_string(html)
                .with_header(header("Content-Type", "text/html; charset=utf-8"))
                .with_header(header("Content-Security-Policy", &csp))
                .with_header(header("Access-Control-Allow-Origin", "*"))
                .with_header(header("Cache-Control", "no-store"))
                .with_header(header("Referrer-Policy", "no-referrer"));
            r.respond(response)?;
            return Ok(());
        }
        if let Some(rel) = path.strip_prefix("/assets/") {
            safe_path(rel)?;
            return send_file(r, &self.assets.join(&path[1..]), true);
        }
        if let Some(key) = path.strip_prefix("/boot/") {
            ensure!(key == self.web_key, "Invalid boot token");
            let response = Response::empty(StatusCode(302))
                .with_header(header(
                    "Set-Cookie",
                    &format!(
                        "framely={}; HttpOnly; SameSite=Strict; Path=/",
                        self.web_key
                    ),
                ))
                .with_header(header(
                    "Location",
                    if r.url().ends_with("?view=manager") {
                        "/manager"
                    } else if r.url().ends_with("?view=notifications") {
                        "/notifications"
                    } else {
                        "/"
                    },
                ));
            r.respond(response)?;
            return Ok(());
        }
        ensure!(
            if remote {
                self.network_authorized(&r)
            } else {
                self.authorized(&r)
            },
            "Session authentication failed"
        );
        if let Some(path) = path.strip_prefix("/api/upload/") {
            ensure!(r.method() == &Method::Post, "Invalid upload method");
            let origin = r
                .headers()
                .iter()
                .find(|h| h.field.equiv("Origin"))
                .map(|h| h.value.as_str());
            ensure!(
                origin == Some(expected_origin.as_str()),
                "Cross-origin API request denied"
            );
            ensure!(
                self.core("agreement.status", json!({}))?["accepted"] == true,
                "请先同意用户协议和隐私声明"
            );
            let (id, offset) = path.split_once('/').context("Invalid upload path")?;
            let offset: u64 = offset.parse()?;
            let mut bytes = Vec::new();
            r.as_reader()
                .take((crate::uploads::CHUNK + 1) as u64)
                .read_to_end(&mut bytes)?;
            let result = self.jobs.uploads.append(id, offset, &bytes)?;
            return send_json(r, json!({"result":result}));
        }
        if path == "/api" && r.method() == &Method::Post {
            let origin = r
                .headers()
                .iter()
                .find(|h| h.field.equiv("Origin"))
                .map(|h| h.value.as_str());
            ensure!(
                origin == Some(expected_origin.as_str()),
                "Cross-origin API request denied"
            );
            let mut bytes = Vec::new();
            r.as_reader().read_to_end(&mut bytes)?;
            let request: Value = serde_json::from_slice(&bytes)?;
            let result = if remote && request["method"] == "agreement.decline" {
                Ok(json!({"closed":false}))
            } else {
                self.api(request)
            };
            let v = match result {
                Ok(v) => json!({"result":v}),
                Err(e) => json!({"error":e.to_string()}),
            };
            return send_json(r, v);
        }
        if path == "/"
            || path == "/manager"
            || path == "/notifications"
            || path.starts_with("/window/")
        {
            let data = fs::read(self.assets.join("index.html"))?;
            let response=Response::from_data(data).with_header(header("Content-Type","text/html; charset=utf-8")).with_header(header("Content-Security-Policy","default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https: http:; frame-src 'self' http://localhost:*; connect-src 'self'; base-uri 'none'; form-action 'none'"));
            r.respond(response)?;
            return Ok(());
        }
        r.respond(Response::from_string("Not found").with_status_code(404))?;
        Ok(())
    }
    fn api(&self, v: Value) -> Result<Value> {
        let method = v["method"].as_str().context("Missing method")?;
        let p = v["params"].clone();
        if !matches!(
            method,
            "status"
                | "language.list"
                | "language.save"
                | "agreement.accept"
                | "agreement.status"
                | "agreement.decline"
                | "agreement.revoke"
                | "ui.events"
                | "host.manager.open"
                | "host.manager.close"
                | "host.menu.close"
                | "host.haptic"
                | "host.keyboard"
        ) {
            ensure!(
                self.core("agreement.status", json!({}))?["accepted"] == true,
                "请先同意用户协议和隐私声明"
            );
        }
        match method {
            "agreement.decline" => {
                let view = p["view"].as_str().context("Missing view")?;
                ensure!(
                    view == "menu"
                        || view == "framely.manager"
                        || view.starts_with("framely.window."),
                    "Invalid view"
                );
                push(
                    &self.commands,
                    json!({"kind":"agreement.decline","view":view}),
                );
                Ok(json!({"closed":true}))
            }
            "catalog" => self.catalog(),
            "catalog.start" => {
                let status = self.core("status", json!({}))?;
                let sources: Vec<Source> =
                    serde_json::from_value(status["database"]["sources"].clone())?;
                let cache = self.catalog_cache.clone();
                let proxy = serde_json::from_value(status["database"]["proxy"].clone())?;
                let kind = format!(
                    "catalog:{}",
                    crate::package::digest(&serde_json::to_vec(
                        &json!({"sources":sources,"proxy":proxy})
                    )?)
                );
                self.jobs.task_progress(&kind, move |cancel, progress| {
                    fetch_catalogs_progress(sources, proxy, cache, &cancel, progress.as_ref())
                })
            }
            "catalog.versions.start" => {
                let plugin_id = p["pluginId"]
                    .as_str()
                    .context("Missing plugin ID")?
                    .to_owned();
                valid_id(&plugin_id)?;
                let source_id = p["source"].as_str().context("Missing source ID")?;
                let status = self.core("status", json!({}))?;
                let sources: Vec<Source> =
                    serde_json::from_value(status["database"]["sources"].clone())?;
                let source = sources
                    .into_iter()
                    .find(|s| s.id == source_id && s.enabled)
                    .context("插件源不存在或已停用")?;
                let proxy = self.proxy()?;
                self.jobs.task(
                    &format!("versions:{}:{}", source.id, plugin_id),
                    move |cancel| {
                        Ok(json!(fetch_plugin_versions_cancel(
                            &source, &plugin_id, &proxy, &cancel
                        )?))
                    },
                )
            }
            "subscriptions.preview.start" => {
                let url = p["url"].as_str().context("缺少订阅 URL")?.to_owned();
                let proxy = self.proxy()?;
                self.jobs.task("subscription", move |cancel| {
                    let (document, _, _) =
                        crate::subscriptions::fetch_cancel(&url, None, None, &proxy, &cancel)?;
                    Ok(json!({"url":url,"document":document.context("订阅未返回内容")?}))
                })
            }
            "subscriptions.refresh.start" => {
                let id = p["id"].as_str().context("缺少订阅 ID")?.to_owned();
                let socket = self.socket.clone();
                let cache = self.catalog_cache.clone();
                self.jobs
                    .task(&format!("subscription:{id}"), move |cancel| {
                        refresh_subscription(&socket, &id, cache, &cancel)
                    })
            }
            "source.test" => {
                let source: Source = serde_json::from_value(p["source"].clone())?;
                source.validate()?;
                let proxy = self.proxy()?;
                self.jobs.task("source", move |cancel| {
                    let catalog = fetch_catalog_proxy_cancel(&source, &proxy, &cancel)?;
                    Ok(json!({"name":catalog.name,"count":catalog.plugins.iter().map(|p|&p.id).collect::<std::collections::BTreeSet<_>>().len()}))
                })
            }
            "upload.start" => self
                .jobs
                .uploads
                .start(p["size"].as_u64().context("Missing upload size")?),
            "upload.abort" => self
                .jobs
                .uploads
                .abort(p["upload"].as_str().context("Missing upload")?),
            "inspect.start" => self.jobs.inspect(self.socket.clone(), p),
            "inspect.resolve.start" => self.jobs.resolve(self.socket.clone(), p),
            "install.start" => self.jobs.install(self.socket.clone(), p),
            "job.status" => self.jobs.status(p["job"].as_str().context("Missing job")?),
            "job.cancel" => self.jobs.cancel(p["job"].as_str().context("Missing job")?),
            "ui.events" => {
                let result = self
                    .events
                    .lock()
                    .unwrap()
                    .after(p["cursor"].as_u64().unwrap_or(0));
                Ok(result)
            }
            "host.manager.open" | "host.manager.close" | "host.menu.close" => {
                let kind = match method {
                    "host.manager.open" => "manager.open",
                    "host.manager.close" => "manager.close",
                    _ => "menu.close",
                };
                push(&self.commands, json!({"kind":kind}));
                Ok(json!(true))
            }
            "network.status" => Ok(
                json!({"passwordConfigured":self.core("network.password.configured",json!({}))?,"localOrigin":self.origin,"ips":network_ipv4_addresses(),"listener":self.network_status.lock().unwrap().clone()}),
            ),
            "network.save" => {
                let mut config_params = p.clone();
                config_params
                    .as_object_mut()
                    .context("Invalid settings")?
                    .remove("password");
                let config: NetworkPanel = serde_json::from_value(config_params)?;
                config.validate()?;
                let current = self.network_status.lock().unwrap().clone();
                if config.enabled && current["port"].as_u64() != Some(config.port as u64) {
                    let probe = Server::http(("0.0.0.0", config.port))
                        .map_err(|e| anyhow::anyhow!("端口不可用：{e}"))?;
                    drop(probe);
                }
                let previous: NetworkPanel = serde_json::from_value(
                    self.core("status", json!({}))?["database"]["networkPanel"].clone(),
                )?;
                let password_changed = p["password"].as_str().is_some_and(|v| !v.is_empty());
                self.core("network.save", p)?;
                let changed = config != previous || password_changed;
                if changed {
                    *self.network_key.lock().unwrap() = random_key();
                }
                Ok(json!({"saved":true,"reauthenticate":changed}))
            }
            "host.external.open.start" => {
                let url = p["url"].as_str().context("Missing web link")?.to_owned();
                crate::model::validate_web_link(&url)?;
                self.jobs.task("external-link", move |cancel| {
                    cancel.commit(|| crate::process::open_web_link(&url))?;
                    Ok(json!(true))
                })
            }
            "host.haptic" => {
                let view = p["view"].as_str().context("Missing haptic view")?;
                ensure!(
                    view.len() <= 160
                        && (matches!(view, "menu" | "framely.manager" | "notifications")
                            || view.starts_with("framely.window.")),
                    "Invalid haptic view"
                );
                let mut commands = self.commands.lock().unwrap();
                if !commands
                    .iter()
                    .any(|c| c["kind"] == "haptic" && c["view"] == view)
                {
                    ensure!(commands.len() < 256, "Native command queue full");
                    commands.push_back(json!({"kind":"haptic","view":view}));
                }
                Ok(json!(true))
            }
            "host.keyboard" => {
                push(
                    &self.commands,
                    json!({"kind":"keyboard","view":p["view"],"existing":p["existing"],"password":p["password"],"multiline":p["multiline"]}),
                );
                Ok(json!(true))
            }
            "host.notifications.empty" => {
                let s = self.core("status", json!({}))?;
                if s["notifications"].as_array().is_some_and(Vec::is_empty) {
                    push(&self.commands, json!({"kind":"notifications.empty"}));
                }
                Ok(json!(true))
            }
            "status"
            | "inspect"
            | "install"
            | "plugin.restart"
            | "plugin.enable"
            | "plugin.enable.preview"
            | "plugin.disable.preview"
            | "plugin.dependents"
            | "plugin.dependencies"
            | "plugin.favorite"
            | "plugin.order"
            | "plugin.uninstall"
            | "plugin.open"
            | "plugin.call"
            | "window.open"
            | "window.close"
            | "notification.send"
            | "notification.remove"
            | "notification.action"
            | "sources.save"
            | "subscriptions.add"
            | "subscriptions.change"
            | "agreement.revoke"
            | "agreement.accept"
            | "agreement.status"
            | "language.list"
            | "language.save"
            | "language.install"
            | "proxy.save"
            | "safeMode"
            | "system.source.save"
            | "system.channel.save"
            | "system.check.start"
            | "system.download.start"
            | "system.job.status"
            | "system.apply"
            | "system.rollback"
            | "logs" => self.core(method, p),
            _ => anyhow::bail!("Unknown UI method"),
        }
    }
    fn proxy(&self) -> Result<crate::model::ProxySettings> {
        Ok(serde_json::from_value(
            self.core("status", json!({}))?["database"]["proxy"].clone(),
        )?)
    }
    fn catalog(&self) -> Result<Value> {
        let status = self.core("status", json!({}))?;
        let sources: Vec<Source> = serde_json::from_value(status["database"]["sources"].clone())?;
        Ok(fetch_catalogs_proxy(
            sources,
            serde_json::from_value(status["database"]["proxy"].clone())?,
            self.catalog_cache.clone(),
        ))
    }
}
pub(crate) fn fetch_catalog_proxy_cancel(
    source: &Source,
    proxy: &crate::model::ProxySettings,
    cancel: &crate::jobs::Cancellation,
) -> Result<Catalog> {
    cancel.check()?;
    source.validate()?;
    let response = crate::http::get_with_proxy_cancel(
        &source.url,
        source.allow_http,
        Duration::from_secs(12),
        &[],
        proxy,
        cancel,
    )
    .context("插件源连接失败")?;
    let bytes = crate::http::read_cancel(response, 2 * 1024 * 1024, cancel)?;
    ensure!(bytes.len() <= 2 * 1024 * 1024, "插件目录超过 2 MiB");
    let catalog: Catalog = serde_json::from_slice(&bytes).context("目录格式无效")?;
    ensure!(
        catalog.schema_version == 1 && catalog.plugins.len() <= 1000,
        "不支持的插件目录"
    );
    let mut ids = std::collections::BTreeSet::new();
    for p in &catalog.plugins {
        p.validate(source.allow_http)?;
        ensure!(ids.insert(&p.id), "主目录中有重复插件 ID");
    }
    Ok(catalog)
}
pub(crate) fn fetch_plugin_versions_cancel(
    source: &Source,
    plugin_id: &str,
    proxy: &crate::model::ProxySettings,
    cancel: &crate::jobs::Cancellation,
) -> Result<Vec<CatalogEntry>> {
    source.validate()?;
    valid_id(plugin_id)?;
    let url = url::Url::parse(&source.url)?.join(&format!("plugins/{plugin_id}/versions.json"))?;
    let response = crate::http::get_with_proxy_cancel(
        url.as_str(),
        source.allow_http,
        Duration::from_secs(12),
        &[],
        proxy,
        cancel,
    )?;
    let bytes = crate::http::read_cancel(response, 2 * 1024 * 1024, cancel)?;
    let document: PluginVersions = serde_json::from_slice(&bytes).context("历史版本格式无效")?;
    ensure!(
        document.schema_version == 1 && document.id == plugin_id && document.versions.len() <= 1000,
        "不支持的插件历史版本目录"
    );
    let mut versions = std::collections::BTreeSet::new();
    for entry in &document.versions {
        entry.validate(source.allow_http)?;
        ensure!(
            entry.id == plugin_id && versions.insert(&entry.version),
            "历史版本 ID 不匹配或版本重复"
        );
    }
    Ok(document.versions)
}

fn refresh_subscription(
    socket: &Path,
    id: &str,
    cache: Arc<Mutex<std::collections::BTreeMap<String, Value>>>,
    cancel: &crate::jobs::Cancellation,
) -> Result<Value> {
    cancel.check()?;
    let status = ipc::call(socket, "status", json!({}))?;
    let database: crate::model::Database = serde_json::from_value(status["database"].clone())?;
    let sub = database
        .subscriptions
        .iter()
        .find(|s| s.id == id)
        .context("订阅不存在")?;
    ensure!(sub.enabled, "订阅已停用");
    let snapshot = crate::package::digest(&serde_json::to_vec(sub)?);
    let result = crate::subscriptions::fetch_cancel(
        &sub.url,
        sub.etag.as_deref(),
        sub.last_modified.as_deref(),
        &database.proxy,
        cancel,
    );
    let request = match &result {
        Ok((doc, etag, modified)) => {
            json!({"id":id,"snapshot":snapshot,"document":doc,"etag":etag,"lastModified":modified,"error":null})
        }
        Err(e) => {
            json!({"id":id,"snapshot":snapshot,"document":null,"etag":null,"lastModified":null,"error":e.to_string()})
        }
    };
    cancel.commit(|| ipc::call(socket, "subscriptions.apply", request))?;
    result?;
    let status = ipc::call(socket, "status", json!({}))?;
    let sources = serde_json::from_value(status["database"]["sources"].clone())?;
    Ok(
        json!({"catalogs":fetch_catalogs_cancel(sources,serde_json::from_value(status["database"]["proxy"].clone())?,cache,cancel)?}),
    )
}
#[cfg(test)]
fn fetch_catalogs(
    sources: Vec<Source>,
    cache: Arc<Mutex<std::collections::BTreeMap<String, Value>>>,
) -> Value {
    fetch_catalogs_proxy(sources, Default::default(), cache)
}
fn fetch_catalogs_proxy(
    sources: Vec<Source>,
    proxy: crate::model::ProxySettings,
    cache: Arc<Mutex<std::collections::BTreeMap<String, Value>>>,
) -> Value {
    fetch_catalogs_cancel(sources, proxy, cache, &Default::default())
        .unwrap_or_else(|e| json!({"error":e.to_string()}))
}
fn fetch_catalogs_cancel(
    sources: Vec<Source>,
    proxy: crate::model::ProxySettings,
    cache: Arc<Mutex<std::collections::BTreeMap<String, Value>>>,
    cancel: &crate::jobs::Cancellation,
) -> Result<Value> {
    fetch_catalogs_progress(sources, proxy, cache, cancel, &|_| {})
}
fn fetch_catalogs_progress(
    sources: Vec<Source>,
    proxy: crate::model::ProxySettings,
    cache: Arc<Mutex<std::collections::BTreeMap<String, Value>>>,
    cancel: &crate::jobs::Cancellation,
    progress: &(dyn Fn(Value) + Send + Sync),
) -> Result<Value> {
    cancel.check()?;
    let sources: Vec<_> = sources.into_iter().filter(|s| s.enabled).collect();
    let mut results = vec![];
    let completed = Mutex::new(Vec::<Value>::new());
    let total_sources = sources.len();
    progress(json!({"completedSources":0,"totalSources":total_sources,"partial":[]}));
    for chunk in sources.chunks(8) {
        cancel.check()?;
        let batch = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk.iter().cloned().map(|source| {
                let cache=cache.clone();let proxy=proxy.clone();let completed=&completed;scope.spawn(move || {
                    let key=format!("{}:{}",source.id,source.url);
                    let value=match fetch_catalog_proxy_cancel(&source,&proxy,cancel) {
                        Ok(catalog) => {
                            let value=json!({"source":source,"catalog":catalog,"fetchedAt":crate::service::now_ms(),"cached":false});
                            let _=cancel.commit(|| {
                                let mut map=cache.lock().unwrap();
                                if map.len()>=200 && !map.contains_key(&key) {
                                    if let Some(old)=map.keys().next().cloned(){map.remove(&old);}
                                }
                                map.insert(key,value.clone());Ok(())
                            });
                            value
                        }
                        Err(error) => {
                            let mut value=cache.lock().unwrap().get(&key).cloned().unwrap_or_else(||json!({"source":source}));
                            value["cached"]=json!(value.get("catalog").is_some());value["error"]=json!(format!("{error:#}"));value
                        }
                    };
                    if cancel.check().is_ok(){let mut partial=completed.lock().unwrap();partial.push(value.clone());progress(json!({"completedSources":partial.len(),"totalSources":total_sources,"partial":*partial}));}
                    value
                })
            }).collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or_else(|_| json!({"error":"插件源任务异常"}))
                })
                .collect::<Vec<_>>()
        });
        results.extend(batch);
    }
    cancel.check()?;
    Ok(json!(results))
}
fn send_json(r: Request, v: Value) -> Result<()> {
    r.respond(
        Response::from_string(serde_json::to_string(&v)?)
            .with_header(header("Content-Type", "application/json"))
            .with_header(header("Cache-Control", "no-store")),
    )?;
    Ok(())
}
fn send_file(r: Request, p: &Path, cors: bool) -> Result<()> {
    let data = fs::read(p)?;
    let mime = match p.extension().and_then(|e| e.to_str()) {
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    };
    let mut response = Response::from_data(data)
        .with_header(header("Content-Type", mime))
        .with_header(header("X-Content-Type-Options", "nosniff"));
    if cors {
        response = response.with_header(header("Access-Control-Allow-Origin", "*"));
    }
    r.respond(response)?;
    Ok(())
}

pub fn serve(
    socket: PathBuf,
    state: PathBuf,
    assets: PathBuf,
    native: PathBuf,
    http_only: bool,
) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "UI session must not run as root"
    );
    let server = Server::http("127.0.0.1:0").map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let port = server
        .server_addr()
        .to_ip()
        .context("Missing HTTP address")?
        .port();
    let origin = format!("http://127.0.0.1:{port}");
    let web_key = random_key();
    let native_key = random_key();
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let runtime = runtime.join(format!("framely-session-{}", std::process::id()));
    fs::create_dir(&runtime)?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))?;
    let agent = Arc::new(Agent {
        socket,
        state,
        assets,
        origin: origin.clone(),
        web_key: web_key.clone(),
        network_key: Mutex::new(random_key()),
        network_status: Mutex::new(json!({"enabled":false})),
        frame_access: Mutex::default(),
        native_key: native_key.clone(),
        commands: Mutex::default(),
        events: Mutex::default(),
        jobs: crate::jobs::Jobs::default(),
        catalog_cache: Arc::default(),
    });
    let network_agent = agent.clone();
    std::thread::spawn(move || network_listener(network_agent, || true));
    let subscriptions_agent = agent.clone();
    std::thread::spawn(move || loop {
        if let Ok(status) = subscriptions_agent.core("status", json!({})) {
            if status["agreement"]["accepted"] != true {
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
            if let Ok(db) =
                serde_json::from_value::<crate::model::Database>(status["database"].clone())
            {
                for sub in db.subscriptions.into_iter().filter(|s| {
                    s.enabled && s.auto_refresh && s.next_refresh <= crate::service::now_ms()
                }) {
                    let socket = subscriptions_agent.socket.clone();
                    let cache = subscriptions_agent.catalog_cache.clone();
                    let id = sub.id;
                    let _ = subscriptions_agent
                        .jobs
                        .task(&format!("subscription:{id}"), move |cancel| {
                            refresh_subscription(&socket, &id, cache, &cancel)
                        });
                }
            }
        }
        std::thread::sleep(Duration::from_secs(30));
    });
    let event_agent = agent.clone();
    std::thread::spawn(move || loop {
        if let Ok(events) = event_agent.core("events", json!({})) {
            if let Some(events) = events.as_array() {
                for e in events {
                    event_agent.events.lock().unwrap().append(e.clone());
                    if [
                        "window.open",
                        "window.close",
                        "plugin.disabled",
                        "notification.changed",
                    ]
                    .contains(&e["kind"].as_str().unwrap_or(""))
                    {
                        let mut command = e.clone();
                        if let Some(path) = command["spec"]["iconPath"].as_str() {
                            if let Ok(bytes) = fs::read(path) {
                                if bytes.len() <= 1024 * 1024 {
                                    use base64::Engine;
                                    command["spec"]["iconData"] = json!(
                                        base64::engine::general_purpose::STANDARD.encode(bytes)
                                    );
                                }
                            }
                            command["spec"].as_object_mut().unwrap().remove("iconPath");
                        }
                        push(&event_agent.commands, command);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    });
    if !http_only {
        let run = runtime.clone();
        let url = format!("{origin}/boot/{web_key}");
        std::thread::spawn(move || loop {
            wait_for_steam_session();
            let result = Command::new(&native)
                .arg("--url")
                .arg(&url)
                .arg("--port")
                .arg(port.to_string())
                .arg("--runtime")
                .arg(&run)
                .args([
                    "--disable-background-networking",
                    "--disable-component-update",
                    "--no-default-browser-check",
                ])
                .env("FRAMELY_NATIVE_TOKEN", &native_key)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status();
            eprintln!("Native host stopped: {:?}; retrying in 3 seconds", result);
            std::thread::sleep(Duration::from_secs(3));
        });
    }
    eprintln!("Framely UI session listening on 127.0.0.1:{port}");
    for r in server.incoming_requests() {
        let a = agent.clone();
        std::thread::spawn(move || {
            let _ = a.handle(r);
        });
    }
    Ok(())
}

fn network_ipv4_addresses() -> Vec<String> {
    let mut addresses = std::collections::BTreeSet::new();
    unsafe {
        let mut first: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut first) != 0 {
            return Vec::new();
        }
        let mut current = first;
        while !current.is_null() {
            let interface = &*current;
            if !interface.ifa_addr.is_null()
                && interface.ifa_flags & libc::IFF_UP as u32 != 0
                && interface.ifa_flags & libc::IFF_LOOPBACK as u32 == 0
                && (*interface.ifa_addr).sa_family as i32 == libc::AF_INET
            {
                let socket = &*(interface.ifa_addr as *const libc::sockaddr_in);
                let ip = std::net::Ipv4Addr::from(socket.sin_addr.s_addr.to_ne_bytes());
                if !ip.is_loopback() && !ip.is_unspecified() {
                    addresses.insert(ip.to_string());
                }
            }
            current = interface.ifa_next;
        }
        libc::freeifaddrs(first);
    }
    addresses.into_iter().collect()
}

const NETWORK_SETUP: &str = r#"<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Framely · 设置访问密码</title><link rel="icon" href="/assets/branding/framely-app-icon.svg" type="image/svg+xml"><link rel="alternate icon" href="/assets/branding/framely.ico"><link rel="apple-touch-icon" href="/assets/branding/framely-app-icon.png"><style>body{background:#17191c;color:#f0f1f3;font:18px system-ui;margin:0;display:grid;min-height:100vh;place-items:center}main{padding:32px;max-width:360px}input,button{box-sizing:border-box;width:100%;padding:14px;margin:12px 0;border:1px solid #59616d;border-radius:5px;background:#292c31;color:inherit;font:inherit}p{color:#a4a8b0}.brand{display:block;width:200px;height:48px;margin-bottom:24px;image-rendering:pixelated}</style><main><img class="brand" src="/assets/branding/framely-logo-light.svg" alt="Framely"><h1>设置访问密码</h1><p>首次访问网络管理面板，请设置至少 8 个字符的密码。保存后使用该密码登录。</p><form method="post" action="/setup"><label>访问密码<input name="password" type="password" required autocomplete="new-password" minlength="8" maxlength="512"></label><label>确认密码<input name="confirmPassword" type="password" required autocomplete="new-password" minlength="8" maxlength="512"></label><button>保存访问密码</button></form></main></html>"#;
const NETWORK_LOGIN: &str = r#"<!doctype html><html lang="zh"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Framely</title><link rel="icon" href="/assets/branding/framely-app-icon.svg" type="image/svg+xml"><link rel="alternate icon" href="/assets/branding/framely.ico"><link rel="apple-touch-icon" href="/assets/branding/framely-app-icon.png"><style>body{background:#17191c;color:#f0f1f3;font:18px system-ui;margin:0;display:grid;min-height:100vh;place-items:center}main{padding:32px;max-width:360px}input,button{box-sizing:border-box;width:100%;padding:14px;margin:12px 0;border:1px solid #59616d;border-radius:5px;background:#292c31;color:inherit;font:inherit}p{color:#a4a8b0}.brand{display:block;width:200px;height:48px;margin-bottom:24px;image-rendering:pixelated}</style><main><h1><img class="brand" src="/assets/branding/framely-logo-light.svg" alt="Framely"></h1><p>输入你在设备设置中配置的访问密码。</p><form method="post" action="/login"><label>访问密码<input name="password" type="password" required autocomplete="current-password" maxlength="512"></label><button>打开管理面板</button></form></main></html>"#;

fn network_listener(agent: Arc<Agent>, keep_running: impl Fn() -> bool) {
    let mut server: Option<Server> = None;
    let mut active: Option<NetworkPanel> = None;
    while keep_running() {
        if let Ok(status) = agent.core("status", json!({})) {
            let config =
                serde_json::from_value::<NetworkPanel>(status["database"]["networkPanel"].clone())
                    .unwrap_or_default();
            if active.as_ref() != Some(&config) {
                server = None;
                active = None;
                if config.enabled {
                    match Server::http(("0.0.0.0", config.port)) {
                        Ok(listener) => {
                            server = Some(listener);
                            active = Some(config.clone());
                            *agent.network_status.lock().unwrap() =
                                json!({"enabled":true,"port":config.port});
                        }
                        Err(e) => {
                            *agent.network_status.lock().unwrap() = json!({"enabled":false,"error":format!("端口 {} 无法监听：{e}",config.port)});
                        }
                    }
                } else {
                    active = Some(config);
                    *agent.network_status.lock().unwrap() = json!({"enabled":false});
                }
            }
        }
        if let Some(listener) = &server {
            if let Ok(Some(request)) = listener.recv_timeout(Duration::from_millis(500)) {
                let a = agent.clone();
                std::thread::spawn(move || {
                    let _ = a.handle_http(request, true);
                });
            }
        } else {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

fn steam_session_ready(states: &str) -> bool {
    let states: Vec<_> = states.lines().collect();
    states.len() == 3 && states.iter().all(|state| *state == "active")
}

fn wait_for_steam_session() {
    loop {
        let ready = Command::new("systemctl")
            .args([
                "--user",
                "is-active",
                "steamvr.service",
                "gamescope-session.service",
                "steam.service",
            ])
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|output| {
                output.status.success()
                    && steam_session_ready(&String::from_utf8_lossy(&output.stdout))
            });
        if ready {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn catalog_progress_reports_fast_source_and_cancellation_does_not_wait_for_slow_source() {
        use std::sync::mpsc;
        let fast = Server::http("127.0.0.1:0").unwrap();
        let slow = Server::http("127.0.0.1:0").unwrap();
        let source = |id: &str, port: String| Source {
            id: id.into(),
            name: id.into(),
            url: format!("http://{port}/catalog.json"),
            enabled: true,
            allow_http: true,
        };
        let sources = vec![
            source("fast", fast.server_addr().to_string()),
            source("slow", slow.server_addr().to_string()),
        ];
        let fast_worker = std::thread::spawn(move || {
            let req = fast.recv().unwrap();
            req.respond(Response::from_string(
                r#"{"schemaVersion":1,"name":"Fast source","plugins":[]}"#,
            ))
            .unwrap();
        });
        let (release_tx, release_rx) = mpsc::channel();
        let slow_worker = std::thread::spawn(move || {
            let req = slow.recv().unwrap();
            release_rx.recv().unwrap();
            let _ = req.respond(Response::empty(500));
        });
        let (tx, rx) = mpsc::channel();
        let token = crate::jobs::Cancellation::default();
        let worker_token = token.clone();
        let worker = std::thread::spawn(move || {
            fetch_catalogs_progress(
                sources,
                Default::default(),
                Default::default(),
                &worker_token,
                &move |v| {
                    let _ = tx.send(v);
                },
            )
        });
        loop {
            let event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
            if event["completedSources"] == 1 {
                assert_eq!(event["partial"][0]["source"]["id"], "fast");
                assert_eq!(event["totalSources"], 2);
                break;
            }
        }
        let started = std::time::Instant::now();
        token.stop();
        assert!(worker.join().unwrap().is_err());
        assert!(started.elapsed() < Duration::from_millis(500));
        release_tx.send(()).unwrap();
        slow_worker.join().unwrap();
        fast_worker.join().unwrap();
    }
    #[test]
    fn native_workers_require_all_steam_services() {
        assert!(super::steam_session_ready("active\nactive\nactive\n"));
        for states in [
            "active\ninactive\nactive\n",
            "activating\nactive\nactive\n",
            "active\ndeactivating\nactive\n",
            "active\n",
            "",
            "active\nactive\nactive\nactive\n",
        ] {
            assert!(!super::steam_session_ready(states), "{states:?}");
        }
    }
    use super::*;
    use crate::package;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn binary_upload_endpoint_requires_auth_origin_and_order() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::http("127.0.0.1:0").unwrap();
        let agent = agent(&server, root.path());
        let socket = UnixListener::bind(&agent.socket).unwrap();
        socket.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let core_stop = stop.clone();
        let core = std::thread::spawn(move || {
            while !core_stop.load(Ordering::Relaxed) {
                if let Ok((mut stream, _)) = socket.accept() {
                    let request = ipc::read(&mut stream).unwrap();
                    assert_eq!(request["method"], "agreement.status");
                    ipc::write(&mut stream, &json!({"result":{"accepted":true}})).unwrap();
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        let worker = http_server(server, agent.clone(), stop.clone());
        let id = agent.jobs.uploads.start(3).unwrap()["upload"]
            .as_str()
            .unwrap()
            .to_owned();
        let url = format!("{}/api/upload/{id}/0", agent.origin);
        let client = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(3))
            .build();
        assert!(client
            .post(&url)
            .set("Origin", &agent.origin)
            .send_bytes(b"a")
            .is_err());
        let cookie = format!("framely={}", agent.web_key);
        assert!(client
            .post(&url)
            .set("Cookie", &cookie)
            .set("Origin", "http://evil.test")
            .send_bytes(b"a")
            .is_err());
        let response: Value = client
            .post(&url)
            .set("Cookie", &cookie)
            .set("Origin", &agent.origin)
            .send_bytes(b"ab")
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(response["result"]["received"], 2);
        assert!(client
            .post(&url)
            .set("Cookie", &cookie)
            .set("Origin", &agent.origin)
            .send_bytes(b"c")
            .is_err());
        client
            .post(&format!("{}/api/upload/{id}/2", agent.origin))
            .set("Cookie", &cookie)
            .set("Origin", &agent.origin)
            .send_bytes(b"c")
            .unwrap();
        let staged = agent.jobs.uploads.take(&id).unwrap();
        assert_eq!(std::fs::read(&staged.path).unwrap(), b"abc");
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        core.join().unwrap();
    }
    #[test]
    fn event_cursors_do_not_steal_events_from_other_views() {
        let mut log = EventLog::default();
        log.append(json!({"kind":"plugin.event","plugin":"a"}));
        assert_eq!(log.after(0), log.after(0));
        assert_eq!(log.after(0)["events"].as_array().unwrap().len(), 1);
        assert!(log.after(1)["events"].as_array().unwrap().is_empty());
        for _ in 0..300 {
            log.append(json!({"kind":"plugin.event"}));
        }
        assert_eq!(log.after(0)["events"].as_array().unwrap().len(), 256);
        assert_eq!(log.after(0)["cursor"], 301);
    }

    fn agent(server: &Server, root: &Path) -> Arc<Agent> {
        Arc::new(Agent {
            socket: root.join("control.sock"),
            state: root.join("state"),
            assets: std::env::var_os("FRAMELY_TEST_ASSETS")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/dist")),
            origin: format!("http://{}", server.server_addr()),
            web_key: random_key(),
            network_key: Mutex::new(random_key()),
            network_status: Mutex::new(json!({"enabled":false})),
            frame_access: Mutex::default(),
            native_key: random_key(),
            commands: Mutex::default(),
            events: Mutex::default(),
            jobs: crate::jobs::Jobs::default(),
            catalog_cache: Arc::default(),
        })
    }
    fn http_server(
        server: Server,
        agent: Arc<Agent>,
        stop: Arc<AtomicBool>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Ok(Some(r)) = server.recv_timeout(Duration::from_millis(50)) {
                    let _ = agent.handle(r);
                }
            }
        })
    }
    #[test]
    fn declining_agreement_only_closes_current_view() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::http("127.0.0.1:0").unwrap();
        let agent = agent(&server, root.path());
        assert_eq!(
            agent
                .api(json!({"method":"agreement.decline","params":{"view":"menu"}}))
                .unwrap()["closed"],
            true
        );
        assert_eq!(
            agent.commands.lock().unwrap().pop_front().unwrap(),
            json!({"kind":"agreement.decline","view":"menu"})
        );
        assert!(agent.commands.lock().unwrap().is_empty());
        assert!(agent
            .api(json!({"method":"agreement.decline","params":{"view":"invalid"}}))
            .is_err());
        assert!(!agent.state.join("state.json").exists());
    }
    #[test]
    fn agreement_blocks_session_jobs_before_consent() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::http("127.0.0.1:0").unwrap();
        let agent = agent(&server, root.path());
        let listener = std::os::unix::net::UnixListener::bind(&agent.socket).unwrap();
        let state = agent.state.clone();
        let worker = std::thread::spawn(move || {
            let mut core = crate::service::Service::load(&state, 1000).unwrap();
            for i in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let request = ipc::read(&mut stream).unwrap();
                assert_eq!(
                    request["method"],
                    if i == 3 {
                        "language.save"
                    } else {
                        "agreement.status"
                    }
                );
                let result = core
                    .handle(
                        request["method"].as_str().unwrap(),
                        request["params"].clone(),
                    )
                    .unwrap();
                ipc::write(&mut stream, &json!({"result":result})).unwrap();
            }
        });
        for method in ["catalog.start", "subscriptions.preview.start", "proxy.save"] {
            assert!(agent
                .api(json!({"method":method,"params":{}}))
                .unwrap_err()
                .to_string()
                .contains("请先同意"));
        }
        assert_eq!(
            agent
                .api(json!({"method":"language.save","params":{"language":"en-US"}}))
                .unwrap(),
            json!(true)
        );
        worker.join().unwrap();
        let core = crate::service::Service::load(&agent.state, 1000).unwrap();
        assert_eq!(core.db.language, "en-US");
        assert!(core.db.agreement_acceptance.is_none());
    }
    #[test]
    fn network_panel_rebinds_disables_and_rejects_occupied_ports() {
        let root = tempfile::tempdir().unwrap();
        let local = Server::http("127.0.0.1:0").unwrap();
        let agent = agent(&local, root.path());
        let free_port = || {
            let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            socket.local_addr().unwrap().port()
        };
        let port = free_port();
        let mut core = crate::tests::accepted_service(&agent.state, 1000).unwrap();
        core.handle(
            "network.save",
            json!({"enabled":true,"port":port,"password":"fixture password"}),
        )
        .unwrap();
        let socket = UnixListener::bind(&agent.socket).unwrap();
        socket.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let core_stop = stop.clone();
        let core_worker = std::thread::spawn(move || {
            while !core_stop.load(Ordering::Relaxed) {
                if let Ok((mut stream, _)) = socket.accept() {
                    let request = ipc::read(&mut stream).unwrap();
                    let result = core
                        .handle(
                            request["method"].as_str().unwrap(),
                            request["params"].clone(),
                        )
                        .unwrap();
                    ipc::write(&mut stream, &json!({"result":result})).unwrap();
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        let listener_agent = agent.clone();
        let listener_stop = stop.clone();
        let listener = std::thread::spawn(move || {
            network_listener(listener_agent, || !listener_stop.load(Ordering::Relaxed))
        });
        let wait = |port: Option<u16>| {
            for _ in 0..100 {
                let status = agent.network_status.lock().unwrap().clone();
                if port.map_or(status["enabled"] == false, |p| {
                    status["enabled"] == true && status["port"] == p
                }) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            panic!("listener did not update");
        };
        wait(Some(port));
        assert_eq!(
            ureq::AgentBuilder::new()
                .redirects(0)
                .build()
                .get(&format!("http://127.0.0.1:{port}"))
                .call()
                .unwrap()
                .status(),
            302
        );
        let occupied = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let blocked = occupied.local_addr().unwrap().port();
        assert!(agent
            .api(json!({"method":"network.save","params":{"enabled":true,"port":blocked}}))
            .is_err());
        assert_eq!(
            agent.core("status", json!({})).unwrap()["database"]["networkPanel"]["port"],
            port
        );
        let next = free_port();
        agent
            .api(json!({"method":"network.save","params":{"enabled":true,"port":next}}))
            .unwrap();
        wait(Some(next));
        assert!(ureq::get(&format!("http://127.0.0.1:{port}"))
            .call()
            .is_err());
        agent
            .api(json!({"method":"network.save","params":{"enabled":false,"port":next}}))
            .unwrap();
        wait(None);
        assert!(ureq::get(&format!("http://127.0.0.1:{next}"))
            .call()
            .is_err());
        stop.store(true, Ordering::Relaxed);
        listener.join().unwrap();
        core_worker.join().unwrap();
    }

    #[test]
    fn network_panel_requires_login_and_same_origin_and_blocks_native_routes() {
        let password = "密码".repeat(70);
        let root = tempfile::tempdir().unwrap();
        let server = Server::http("127.0.0.1:0").unwrap();
        let agent = agent(&server, root.path());
        let mut core = crate::service::Service::load(&agent.state, 1000).unwrap();
        let socket = UnixListener::bind(&agent.socket).unwrap();
        socket.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_agent = agent.clone();
        let worker_stop = stop.clone();
        let core_stop = stop.clone();
        let core_worker = std::thread::spawn(move || {
            while !core_stop.load(Ordering::Relaxed) {
                if let Ok((mut stream, _)) = socket.accept() {
                    let request = ipc::read(&mut stream).unwrap();
                    let response = match core.handle(
                        request["method"].as_str().unwrap(),
                        request["params"].clone(),
                    ) {
                        Ok(v) => json!({"result":v}),
                        Err(e) => json!({"error":e.to_string()}),
                    };
                    ipc::write(&mut stream, &response).unwrap();
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        let worker = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                if let Ok(Some(r)) = server.recv_timeout(Duration::from_millis(50)) {
                    let _ = worker_agent.handle_http(r, true);
                }
            }
        });
        let origin = agent.origin.clone();
        let page = ureq::get(&origin).call().unwrap().into_string().unwrap();
        assert!(page.contains("设置访问密码"));
        assert!(page.contains("action=\"/setup\""));
        assert!(page.contains("/assets/branding/framely-logo-light.svg"));
        let logo = ureq::get(&format!("{origin}/assets/branding/framely-logo-light.svg"))
            .call()
            .unwrap();
        assert_eq!(logo.header("Content-Type"), Some("image/svg+xml"));
        assert_eq!(logo.header("X-Content-Type-Options"), Some("nosniff"));
        assert_eq!(
            logo.into_string().unwrap(),
            include_str!("../assets/branding/framely-logo-light.svg")
        );
        let icon = ureq::get(&format!("{origin}/favicon.ico")).call().unwrap();
        assert_eq!(icon.header("Content-Type"), Some("image/x-icon"));
        for path in [
            "/assets/app.js",
            "/assets/branding/../app.js",
            "/assets/branding/unknown.svg",
        ] {
            assert!(matches!(
                ureq::get(&format!("{origin}{path}")).call(),
                Err(ureq::Error::Status(401, _))
            ));
        }
        assert!(!page.contains(password.as_str()));
        assert!(ureq::post(&format!("{origin}/api"))
            .set("Origin", &origin)
            .send_json(json!({"method":"ui.events"}))
            .is_err());
        assert!(ureq::get(&format!("{origin}/boot/{}", agent.web_key))
            .call()
            .is_err());
        assert!(ureq::get(&format!("{origin}/host/poll"))
            .set("X-Framely-Native", &agent.native_key)
            .call()
            .is_err());
        let client = ureq::AgentBuilder::new().redirects(0).build();
        assert!(client
            .post(&format!("{origin}/setup"))
            .set("Origin", "http://other.example")
            .send_form(&[
                ("password", password.as_str()),
                ("confirmPassword", password.as_str())
            ])
            .is_err());
        for (password, confirmation) in [("short", "short"), (password.as_str(), "different")] {
            assert!(client
                .post(&format!("{origin}/setup"))
                .set("Origin", &origin)
                .send_form(&[("password", password), ("confirmPassword", confirmation)])
                .is_err());
        }
        assert!(!agent.state.join("network-password.json").exists());
        fs::create_dir(agent.state.join("state.json.tmp")).unwrap();
        assert!(client
            .post(&format!("{origin}/setup"))
            .set("Origin", &origin)
            .send_form(&[
                ("password", password.as_str()),
                ("confirmPassword", password.as_str())
            ])
            .is_err());
        assert!(!agent.state.join("network-password.json").exists());
        fs::remove_dir(agent.state.join("state.json.tmp")).unwrap();
        let setup = client
            .post(&format!("{origin}/setup"))
            .set("Origin", &origin)
            .send_form(&[
                ("password", password.as_str()),
                ("confirmPassword", password.as_str()),
            ])
            .unwrap();
        assert_eq!(setup.status(), 303);
        assert!(setup.header("Set-Cookie").is_none());
        assert!(client
            .post(&format!("{origin}/setup"))
            .set("Origin", &origin)
            .send_form(&[
                ("password", "replacement password"),
                ("confirmPassword", "replacement password")
            ])
            .is_err());
        let login = client.get(&origin).call().unwrap().into_string().unwrap();
        assert!(login.contains("action=\"/login\""));
        let saved = fs::read(agent.state.join("network-password.json")).unwrap();
        assert!(!String::from_utf8_lossy(&saved).contains(password.as_str()));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(agent.state.join("network-password.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let restored = crate::service::Service::load(&agent.state, 1000).unwrap();
        assert!(restored.db.network_panel.password_enabled);
        assert!(restored.db.agreement_acceptance.is_none());
        assert!(client
            .post(&format!("{origin}/login"))
            .set("Origin", &origin)
            .send_form(&[("password", "wrong")])
            .is_err());
        let response = client
            .post(&format!("{origin}/login"))
            .set("Origin", &origin)
            .send_form(&[("password", password.as_str())])
            .unwrap();
        assert_eq!(response.status(), 302);
        let cookie = response
            .header("Set-Cookie")
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let result: Value = client
            .post(&format!("{origin}/api"))
            .set("Cookie", cookie)
            .set("Origin", &origin)
            .send_json(json!({"method":"ui.events","params":{}}))
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(result["result"]["cursor"], 0);
        assert!(client
            .post(&format!("{origin}/api"))
            .set("Cookie", cookie)
            .set("Origin", "http://other.example")
            .send_json(json!({"method":"ui.events"}))
            .is_err());
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        core_worker.join().unwrap();
    }

    #[test]
    fn http_session_rejects_unauthenticated_and_opaque_plugin_requests() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::http("127.0.0.1:0").unwrap();
        let agent = agent(&server, root.path());
        let stop = Arc::new(AtomicBool::new(false));
        let thread = http_server(server, agent.clone(), stop.clone());
        let url = format!("{}/api", agent.origin);
        assert!(ureq::post(&url)
            .send_json(json!({"method":"ui.events"}))
            .is_err());
        assert!(ureq::post(&url)
            .set("Cookie", &format!("framely={}", agent.web_key))
            .set("Origin", "null")
            .send_json(json!({"method":"ui.events"}))
            .is_err());
        let v: Value = ureq::post(&url)
            .set("Cookie", &format!("framely={}", agent.web_key))
            .set("Origin", &agent.origin)
            .send_json(json!({"method":"ui.events","params":{}}))
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(v["result"]["cursor"], 0);
        assert!(ureq::get(&format!("{}/host/poll", agent.origin))
            .call()
            .is_err());
        let v: Value = ureq::get(&format!("{}/host/poll", agent.origin))
            .set("X-Framely-Native", &agent.native_key)
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        assert_eq!(v["commands"], json!([]));
        stop.store(true, Ordering::Relaxed);
        thread.join().unwrap();
    }
    #[test]
    fn version_history_checks_identity_duplicates_and_schema() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let source = Source {
            id: "history-test".into(),
            name: "History".into(),
            url: format!("http://{}/catalog.json", server.server_addr()),
            enabled: true,
            allow_http: true,
        };
        let entry = json!({"id":"test.history","name":"History","version":"1.0.0","description":"","author":"Test","apiVersion":1,"runAs":"steamos","url":"http://example.org/plugin.framely","sha256":"ab".repeat(32),"future":true});
        let documents = vec![
            json!({"schemaVersion":1,"id":"test.history","versions":[entry.clone()],"future":true}),
            json!({"schemaVersion":1,"id":"other.plugin","versions":[]}),
            json!({"schemaVersion":1,"id":"test.history","versions":[entry.clone(),entry]}),
            json!({"schemaVersion":2,"id":"test.history","versions":[]}),
        ];
        let thread = std::thread::spawn(move || {
            for doc in documents {
                let request = server.recv().unwrap();
                assert_eq!(request.url(), "/plugins/test.history/versions.json");
                request
                    .respond(tiny_http::Response::from_data(
                        serde_json::to_vec(&doc).unwrap(),
                    ))
                    .unwrap();
            }
        });
        assert_eq!(
            fetch_plugin_versions_cancel(
                &source,
                "test.history",
                &Default::default(),
                &Default::default()
            )
            .unwrap()
            .len(),
            1
        );
        for _ in 0..3 {
            assert!(fetch_plugin_versions_cancel(
                &source,
                "test.history",
                &Default::default(),
                &Default::default()
            )
            .is_err());
        }
        thread.join().unwrap();
    }

    #[test]
    fn catalog_sources_fail_independently_and_keep_cached_data() {
        let server = Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}/catalog.json", server.server_addr());
        let source = Source {
            id: "good".into(),
            name: "Good".into(),
            url,
            enabled: true,
            allow_http: true,
        };
        let offline = Source {
            id: "offline".into(),
            name: "Offline".into(),
            url: "http://127.0.0.1:1/catalog.json".into(),
            enabled: true,
            allow_http: true,
        };
        let handle = std::thread::spawn(move || {
            let r = server.recv().unwrap();
            r.respond(Response::from_string(
                r#"{"schemaVersion":1,"name":"Test store","plugins":[]}"#,
            ))
            .unwrap();
        });
        let cache = Arc::default();
        let result = fetch_catalogs(vec![source.clone(), offline], Arc::clone(&cache));
        handle.join().unwrap();
        let results = result.as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["catalog"]["name"], "Test store");
        assert!(results[1].get("error").is_some());
        let stale = fetch_catalogs(vec![source], cache);
        assert_eq!(stale[0]["cached"], true);
        assert_eq!(stale[0]["catalog"]["name"], "Test store");
    }
    #[test]
    fn remote_frames_load_scoped_assets_and_unchanged_settings_keep_session() {
        let root = tempfile::tempdir().unwrap();
        let local = Server::http("127.0.0.1:0").unwrap();
        let assets = root.path().join("ui");
        std::fs::create_dir_all(assets.join("assets")).unwrap();
        let sdk_fixture = "window.__framely_test_sdk = true;";
        std::fs::write(assets.join("assets/plugin-bootstrap.js"), sdk_fixture).unwrap();
        let mut agent = agent(&local, root.path());
        Arc::get_mut(&mut agent).unwrap().assets = assets;
        let remote = Server::http("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", remote.server_addr());
        let mut core = crate::tests::accepted_service(&agent.state, 1000).unwrap();
        core.handle("network.save", json!({"enabled":true,"port":15915,"passwordEnabled":false,"password":"fixture password"})).unwrap();
        let bytes = crate::tests::fixture(root.path(), "test.remote", "1", 1, None);
        core.handle(
            "install",
            json!({"package":STANDARD.encode(bytes),"approve":true}),
        )
        .unwrap();
        *agent.network_status.lock().unwrap() = json!({"enabled":true,"port":15915});
        let socket = UnixListener::bind(&agent.socket).unwrap();
        socket.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let core_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            while !core_stop.load(Ordering::Relaxed) {
                if let Ok((mut stream, _)) = socket.accept() {
                    let request = ipc::read(&mut stream).unwrap();
                    let response = match core.handle(
                        request["method"].as_str().unwrap(),
                        request["params"].clone(),
                    ) {
                        Ok(v) => json!({"result":v}),
                        Err(e) => json!({"error":e.to_string()}),
                    };
                    ipc::write(&mut stream, &response).unwrap();
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        let http_stop = stop.clone();
        let a = agent.clone();
        let http = std::thread::spawn(move || {
            while !http_stop.load(Ordering::Relaxed) {
                if let Ok(Some(r)) = remote.recv_timeout(Duration::from_millis(50)) {
                    let _ = a.handle_http(r, true);
                }
            }
        });
        let old_key = agent.network_key.lock().unwrap().clone();
        let cookie = format!("framely-network={old_key}");
        let frame = ureq::get(&format!("{origin}/plugin-frame/test.remote/quick"))
            .set("Cookie", &cookie)
            .call()
            .unwrap();
        let csp = frame.header("Content-Security-Policy").unwrap().to_owned();
        assert!(csp.contains("connect-src https: http: ws: wss:"));
        assert!(csp.contains("img-src ") && csp.contains("data: blob: https: http:"));
        let html = frame.into_string().unwrap();
        assert!(!html.contains(&agent.origin));
        assert!(!csp.contains(&agent.origin));
        assert!(csp.contains(&origin));
        let scripts: Vec<_> = html
            .split("<script src='")
            .skip(1)
            .map(|s| s.split('\'').next().unwrap())
            .collect();
        assert_eq!(scripts.len(), 2);
        for (index, script) in scripts.iter().enumerate() {
            let response = ureq::get(script).set("Origin", "null").call().unwrap();
            assert_eq!(response.header("Access-Control-Allow-Origin"), Some("*"));
            if index == 0 {
                assert_eq!(response.into_string().unwrap(), sdk_fixture);
            }
        }
        assert!(
            ureq::get(&format!("{origin}/plugin-assets/test.remote/page.js"))
                .call()
                .is_err()
        );
        let forged = scripts[0].replace("sdk.js", "api");
        assert!(ureq::post(&forged)
            .send_json(json!({"method":"status"}))
            .is_err());
        let response=ureq::post(&format!("{origin}/api")).set("Origin",&origin).set("Cookie",&cookie).send_json(json!({"method":"network.save","params":{"enabled":true,"port":15915,"passwordEnabled":false}})).unwrap().into_json::<Value>().unwrap();
        assert_eq!(response["result"]["reauthenticate"], false);
        assert_eq!(*agent.network_key.lock().unwrap(), old_key);
        assert!(ureq::post(&format!("{origin}/api"))
            .set("Origin", &origin)
            .set("Cookie", &cookie)
            .send_json(json!({"method":"status"}))
            .is_ok());
        agent.api(json!({"method":"network.save","params":{"enabled":false,"port":15915,"passwordEnabled":false}})).unwrap();
        assert!(ureq::get(scripts[0]).call().is_err());
        let error = ureq::post(&format!("{origin}/api"))
            .set("Origin", &origin)
            .set("Cookie", &cookie)
            .send_json(json!({"method":"status"}));
        match error {
            Err(ureq::Error::Status(401, r)) => {
                assert_eq!(r.header("X-Framely-Reauthenticate"), Some("1"))
            }
            other => panic!("unexpected result: {other:?}"),
        }
        stop.store(true, Ordering::Relaxed);
        http.join().unwrap();
        worker.join().unwrap();
    }
    #[test]
    #[ignore = "requires compiled tests/browser_probe.cpp and built React UI"]
    fn cef_manager_plugin_bridge_integration() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::http("127.0.0.1:0").unwrap();
        let agent = agent(&server, root.path());
        let mut core =
            crate::tests::accepted_service(&agent.state, unsafe { libc::getuid() }).unwrap();
        core.handle("language.save", json!({"language":"zh-CN"}))
            .unwrap();
        let payload = root.path().join("payload");
        fs::create_dir(&payload).unwrap();
        let keyboard_fixture = fs::read_to_string("target/keyboard-fixture.js").unwrap();
        fs::write(payload.join("page.js"),keyboard_fixture+r#"
(async()=>{try{
          for(let i=0;i<300&&!window.__framelyKeyboardFixtureDone;i++)await new Promise(r=>setTimeout(r,20));if(!window.__framelyKeyboardFixtureDone)throw Error('React keyboard fixture failed');
          const bridge=window.__framelyBridge;let isolated=false;try{parent.document.body;}catch{isolated=true;}if(!isolated)throw new Error('iframe can read manager DOM');
          let blocked=false;try{const response=await fetch('/api',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({method:'status'})});blocked=!response.ok;}catch{blocked=true;}if(!blocked)throw new Error('opaque plugin called admin API');
          await bridge.request('haptic',{view:'framely.window.forged'});
          await bridge.request('window.open',{plugin:'another.plugin',window:'main'});
          await bridge.request('notification.send',{plugin:'another.plugin',notification:{id:'test',title:'Bridge test',body:'one',actions:[]}});
          await bridge.request('notification.send',{notification:{id:'test',title:'Updated',body:'two',actions:[]}});
          await bridge.request('notification.remove',{id:'test'});
          await bridge.request('window.close',{window:'main'});
          const dependencies=await bridge.request('dependencies',{plugin:'forged'});if(!Array.isArray(dependencies)||dependencies.length)throw new Error('dependency bridge scope');let denied=false;try{await bridge.request('install',{});}catch{denied=true;}if(!denied)throw new Error('plugin installation allowed');
          console.log('FRAMELY_BRIDGE_PASS');
        }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e.stack);}})();"#).unwrap();
        let manifest = root.path().join("manifest.json");
        fs::write(&manifest,serde_json::to_vec(&json!({"schemaVersion":1,"apiVersion":1,"id":"test.bridge","name":"Bridge","author":"Test","version":"1","backend":null,"ui":{"quickPage":"page.js","windows":{"main":{"entry":"page.js","title":"Test","dockIcon":true}}},"files":{}})).unwrap()).unwrap();
        let out = root.path().join("test.framely");
        package::pack(&manifest, &payload, &out).unwrap();
        core.handle(
            "install",
            json!({"package":STANDARD.encode(fs::read(&out).unwrap()),"approve":true}),
        )
        .unwrap();
        let shop = Server::http("127.0.0.1:0").unwrap();
        let shop_url = format!("http://{}", shop.server_addr());
        let mut next: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        next["version"] = json!("2.0.0");
        next["details"] = json!("Store integration details");
        next["tags"] = json!(["工具"]);
        next["changelog"] = json!("Updated version");
        fs::write(&manifest, serde_json::to_vec(&next).unwrap()).unwrap();
        let next_package = root.path().join("next.framely");
        package::pack(&manifest, &payload, &next_package).unwrap();
        let next_bytes = fs::read(next_package).unwrap();
        let prior_bytes = fs::read(&out).unwrap();
        let catalog = json!({"futureMetadata":{"new":true},"schemaVersion":1,"name":"Integration store","plugins":[{"id":"test.bridge","name":"Bridge","author":"Test","version":"2.0.0","apiVersion":1,"futureEntryField":true,"description":"Store test","details":"Store integration details","tags":["工具"],"changelog":"Updated version","runAs":"root","url":format!("{shop_url}/next.framely"),"sha256":package::digest(&next_bytes)}]});
        let history = json!({"futureHistoryField":true,"schemaVersion":1,"id":"test.bridge","versions":[{"id":"test.bridge","name":"Bridge","author":"Test","version":"1","apiVersion":1,"description":"Original version","runAs":"steamos","url":format!("{shop_url}/prior.framely"),"sha256":package::digest(&prior_bytes)}]});
        core.handle("sources.save",json!({"sources":[{"id":"test-shop","name":"Test store","url":format!("{shop_url}/catalog.json"),"enabled":true,"allowHttp":true}]})).unwrap();
        core.handle("subscriptions.add",json!({"url":"https://example.org/subscription-fixture.json","document":{"schemaVersion":1,"name":"Subscription fixture","sources":[]},"approve":true})).unwrap();
        let shop_stop = Arc::new(AtomicBool::new(false));
        let done = shop_stop.clone();
        let shop_thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if let Ok(Some(r)) = shop.recv_timeout(Duration::from_millis(50)) {
                    let data = if r.url() == "/next.framely" {
                        next_bytes.clone()
                    } else if r.url() == "/prior.framely" {
                        prior_bytes.clone()
                    } else if r.url() == "/plugins/test.bridge/versions.json" {
                        serde_json::to_vec(&history).unwrap()
                    } else {
                        serde_json::to_vec(&catalog).unwrap()
                    };
                    let _ = r.respond(Response::from_data(data));
                }
            }
        });
        let listener = UnixListener::bind(&agent.socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let end = stop.clone();
        let core_thread = std::thread::spawn(move || {
            while !end.load(Ordering::Relaxed) {
                if let Ok((mut stream, _)) = listener.accept() {
                    let request = ipc::read(&mut stream).unwrap();
                    let response = match core.handle(
                        request["method"].as_str().unwrap(),
                        request["params"].clone(),
                    ) {
                        Ok(v) => json!({"result":v}),
                        Err(e) => json!({"error":e.to_string()}),
                    };
                    ipc::write(&mut stream, &response).unwrap();
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            core
        });
        let http = http_server(server, agent.clone(), stop.clone());
        let probe = std::env::var_os("FRAMELY_BROWSER_PROBE").expect("set FRAMELY_BROWSER_PROBE");
        let result = Command::new(probe)
            .arg(format!("{}/boot/{}", agent.origin, agent.web_key))
            .arg(root.path())
            .env("HOME", root.path())
            .env("XDG_RUNTIME_DIR", root.path())
            .env("GSETTINGS_BACKEND", "memory")
            .args(["--ozone-platform=headless", "--disable-gpu"])
            .output()
            .unwrap();
        agent.api(json!({"method":"network.save","params":{"enabled":true,"port":15915,"passwordEnabled":false,"password":"fixture password"}})).unwrap();
        let remote_server = Server::http("127.0.0.1:0").unwrap();
        let remote_origin = format!("http://{}", remote_server.server_addr());
        let remote_stop = stop.clone();
        let remote_agent = agent.clone();
        let remote_http = std::thread::spawn(move || {
            while !remote_stop.load(Ordering::Relaxed) {
                if let Ok(Some(r)) = remote_server.recv_timeout(Duration::from_millis(50)) {
                    let _ = remote_agent.handle_http(r, true);
                }
            }
        });
        let remote_result = Command::new(std::env::var_os("FRAMELY_BROWSER_PROBE").unwrap())
            .arg(format!("{remote_origin}/manager"))
            .arg(root.path())
            .env("FRAMELY_REMOTE_TEST", "1")
            .env("HOME", root.path())
            .env("XDG_RUNTIME_DIR", root.path())
            .env("GSETTINGS_BACKEND", "memory")
            .args(["--ozone-platform=headless", "--disable-gpu"])
            .output()
            .unwrap();
        print!("{}", String::from_utf8_lossy(&remote_result.stdout));
        eprint!("{}", String::from_utf8_lossy(&remote_result.stderr));
        let store_result = Command::new(std::env::var_os("FRAMELY_BROWSER_PROBE").unwrap())
            .arg(format!(
                "{}/boot/{}?view=manager",
                agent.origin, agent.web_key
            ))
            .arg(root.path())
            .env("FRAMELY_STORE_TEST", "1")
            .env("HOME", root.path())
            .env("XDG_RUNTIME_DIR", root.path())
            .env("GSETTINGS_BACKEND", "memory")
            .args(["--ozone-platform=headless", "--disable-gpu"])
            .output()
            .unwrap();
        print!("{}", String::from_utf8_lossy(&store_result.stdout));
        eprint!("{}", String::from_utf8_lossy(&store_result.stderr));
        agent.core("notification.send",json!({"plugin":"test.bridge","notification":{"id":"toast","title":"Toast integration","body":"Compact notification body","image":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aN1kAAAAASUVORK5CYII=","durationMs":15000,"actions":[{"id":"ok","label":"确认","icon":"✓"},{"id":"save","label":"保存","icon":"↓"},{"id":"more","label":"详情","icon":"⋯"}]}})).unwrap();
        let notification_result = Command::new(std::env::var_os("FRAMELY_BROWSER_PROBE").unwrap())
            .arg(format!(
                "{}/boot/{}?view=notifications",
                agent.origin, agent.web_key
            ))
            .arg(root.path())
            .env("FRAMELY_NOTIFICATION_TEST", "1")
            .env("HOME", root.path())
            .env("XDG_RUNTIME_DIR", root.path())
            .env("GSETTINGS_BACKEND", "memory")
            .args(["--ozone-platform=headless", "--disable-gpu"])
            .output()
            .unwrap();
        print!("{}", String::from_utf8_lossy(&notification_result.stdout));
        eprint!("{}", String::from_utf8_lossy(&notification_result.stderr));
        assert!(
            notification_result.status.success(),
            "CEF notification integration failed"
        );
        shop_stop.store(true, Ordering::Relaxed);
        shop_thread.join().unwrap();
        stop.store(true, Ordering::Relaxed);
        http.join().unwrap();
        remote_http.join().unwrap();
        assert!(
            remote_result.status.success(),
            "CEF remote iframe integration failed"
        );
        let mut core = core_thread.join().unwrap();
        print!("{}", String::from_utf8_lossy(&result.stdout));
        eprint!("{}", String::from_utf8_lossy(&result.stderr));
        assert!(result.status.success(), "CEF browser integration failed");
        assert!(
            store_result.status.success(),
            "CEF store integration failed"
        );
        assert_eq!(core.db.plugins["test.bridge"].manifest.version, "2.0.0");
        assert!(
            core.db.subscriptions.is_empty(),
            "CEF subscription removal did not persist"
        );
        assert!(
            agent
                .commands
                .lock()
                .unwrap()
                .iter()
                .any(|c| c["kind"] == "manager.open"),
            "Settings must open the native manager window"
        );
        let commands = agent.commands.lock().unwrap();
        assert!(commands
            .iter()
            .any(|c| c["kind"] == "haptic" && c["view"] == "menu"));
        assert!(!commands
            .iter()
            .any(|c| c["view"] == "framely.window.forged"));
        drop(commands);
        let events = core.handle("events", json!({})).unwrap();
        assert!(events
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "window.open" && e["plugin"] == "test.bridge"));
        assert!(events
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event"] == "notification.action"
                && e["data"]["id"] == "toast"
                && e["data"]["action"] == "ok"));
        assert!(core.handle("status", json!({})).unwrap()["notifications"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
