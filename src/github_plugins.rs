//! GitHub-native plugin sources. Discovery reads metadata, never package bytes.
use crate::{jobs::Cancellation, model::*};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

const TOPIC: &str = "topic:framely-plugin archived:false fork:false";
const TTL: u64 = 600_000;
const MAX_PAGES: usize = 10;
static REQUESTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct RequestPermit;
impl RequestPermit {
    fn acquire(cancel: &Cancellation) -> Result<Self> {
        use std::sync::atomic::Ordering;
        loop {
            cancel.check()?;
            if REQUESTS
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                    (n < 6).then_some(n + 1)
                })
                .is_ok()
            {
                return Ok(Self);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for RequestPermit {
    fn drop(&mut self) {
        REQUESTS.fetch_sub(1, std::sync::atomic::Ordering::Release);
    }
}
#[derive(Clone, Deserialize, Serialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize, Serialize)]
struct Cached {
    at: u64,
    etag: Option<String>,
    body: String,
}
pub struct Client<'a> {
    proxy: &'a ProxySettings,
    cancel: &'a Cancellation,
    root: PathBuf,
    refresh: bool,
    pub warnings: Mutex<Vec<String>>,
}
impl<'a> Client<'a> {
    pub fn system(
        proxy: &'a ProxySettings,
        cancel: &'a Cancellation,
        refresh: bool,
    ) -> Result<Self> {
        Ok(Self::new(
            proxy,
            cancel,
            crate::steam::home()?.join(".cache/framely/github-plugins"),
            refresh,
        ))
    }
    fn new(
        proxy: &'a ProxySettings,
        cancel: &'a Cancellation,
        root: PathBuf,
        refresh: bool,
    ) -> Self {
        Self {
            proxy,
            cancel,
            root,
            refresh,
            warnings: Mutex::default(),
        }
    }
    fn warn(&self, message: String) {
        self.warnings.lock().unwrap().push(message);
    }
    fn text(&self, url: &str, limit: usize) -> Result<String> {
        self.cancel.check()?;
        let path = self
            .root
            .join(format!("{}.json", crate::package::digest(url.as_bytes())));
        let saved = fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file() && m.len() <= (limit as u64 * 6 + 4096))
            .and_then(|_| fs::read(&path).ok())
            .and_then(|v| serde_json::from_slice::<Cached>(&v).ok())
            .filter(|v| v.body.len() <= limit);
        if !self.refresh {
            if let Some(value) = &saved {
                if crate::service::now_ms().saturating_sub(value.at) < TTL {
                    return Ok(value.body.clone());
                }
            }
        }
        let _permit = RequestPermit::acquire(self.cancel)?;
        let mut headers = vec![
            ("User-Agent", "Framely"),
            ("Accept", "application/vnd.github+json"),
        ];
        if let Some(etag) = saved
            .as_ref()
            .and_then(|v| v.etag.as_deref())
            .filter(|v| v.len() <= 1024 && !v.contains(['\r', '\n']))
        {
            headers.push(("If-None-Match", etag));
        }
        let mut retryable = false;
        let result = crate::http::with_github_fallback(url, &headers, self.proxy, |proxy| {
            let attempt = (|| {
                let response = crate::http::get_with_proxy_cancel(
                    url,
                    false,
                    Duration::from_secs(15),
                    &headers,
                    proxy,
                    self.cancel,
                )?;
                if response.status() == 304 {
                    let value = saved.as_ref().context("Unexpected GitHub 304")?;
                    return Ok(Cached {
                        at: crate::service::now_ms(),
                        etag: value.etag.clone(),
                        body: value.body.clone(),
                    });
                }
                let etag = response.header("ETag").map(str::to_owned);
                let bytes = crate::http::read_cancel(response, limit, self.cancel)?;
                Ok(Cached {
                    at: crate::service::now_ms(),
                    etag,
                    body: String::from_utf8(bytes)?,
                })
            })();
            if let Err(error) = &attempt {
                retryable = crate::http::github_fallback::retryable(error);
            }
            attempt
        });
        self.cancel.check()?;
        match result {
            Ok(value) => {
                // Cache failures do not prevent browsing; write atomically.
                let _ = (|| -> Result<()> {
                    fs::create_dir_all(&self.root)?;
                    let mut file = tempfile::NamedTempFile::new_in(&self.root)?;
                    file.write_all(&serde_json::to_vec(&value)?)?;
                    file.as_file().sync_all()?;
                    file.persist(&path)?;
                    Ok(())
                })();
                Ok(value.body)
            }
            Err(error) => {
                // Never conceal changed/missing releases, certificates or bad
                // JSON behind old metadata. Only network failures use cache.
                if retryable {
                    if let Some(value) = saved {
                        self.warn(format!(
                            "GitHub: {} (cached)",
                            crate::http::github_fallback::diagnostic(&error)
                        ));
                        return Ok(value.body);
                    }
                }
                Err(error)
            }
        }
    }
    fn json(&self, url: &str, limit: usize) -> Result<Value> {
        Ok(serde_json::from_str(&self.text(url, limit)?)?)
    }
    pub fn discover(&self) -> Result<Vec<Source>> {
        let mut sources = vec![];
        let mut seen = BTreeSet::new();
        for page in 1..=MAX_PAGES {
            self.cancel.check()?;
            let mut url = url::Url::parse("https://api.github.com/search/repositories")?;
            url.query_pairs_mut()
                .append_pair("q", TOPIC)
                .append_pair("sort", "stars")
                .append_pair("order", "desc")
                .append_pair("per_page", "100")
                .append_pair("page", &page.to_string());
            let value = self.json(url.as_str(), 4 * 1024 * 1024)?;
            ensure!(
                value["incomplete_results"] != true,
                "Incomplete GitHub plugin search"
            );
            let items = value["items"]
                .as_array()
                .context("Invalid GitHub plugin search")?;
            ensure!(items.len() <= 100, "Invalid GitHub search page size");
            for item in items {
                if item["archived"] != false
                    || item["fork"] != false
                    || !item["topics"]
                        .as_array()
                        .is_some_and(|v| v.iter().any(|t| t == "framely-plugin"))
                {
                    continue;
                }
                if let Some(repo) = item["full_name"].as_str() {
                    if let Ok(source) = source(repo) {
                        if seen.insert(source.id.clone()) {
                            sources.push(source);
                        }
                    }
                }
            }
            let total = value["total_count"].as_u64().unwrap_or(0);
            if items.len() < 100 || (page * 100) as u64 >= total {
                return Ok(sources);
            }
        }
        self.warn("GitHub search reached its 1000-repository limit".into());
        Ok(sources)
    }
    pub fn entries(&self, repo: &str) -> Result<Vec<CatalogEntry>> {
        source(repo)?;
        let mut releases = vec![];
        for page in 1..=MAX_PAGES {
            let values = self.json(
                &format!("https://api.github.com/repos/{repo}/releases?per_page=100&page={page}"),
                8 * 1024 * 1024,
            )?;
            let values = values.as_array().context("Invalid GitHub releases")?;
            ensure!(values.len() <= 100, "Invalid GitHub release page size");
            for value in values {
                let release: Release = serde_json::from_value(value.clone())?;
                if !release.draft {
                    releases.push(release);
                }
            }
            if values.len() < 100 {
                break;
            }
            ensure!(
                page < MAX_PAGES,
                "GitHub release history exceeds 1000 entries"
            );
        }
        let mut entries = vec![];
        for chunk in releases.chunks(6) {
            self.cancel.check()?;
            let results = std::thread::scope(|scope| {
                chunk
                    .iter()
                    .map(|release| scope.spawn(move || self.release(repo, release)))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|h| {
                        h.join().unwrap_or_else(|_| {
                            Err(anyhow::anyhow!("Release metadata worker failed"))
                        })
                    })
                    .collect::<Vec<_>>()
            });
            self.cancel.check()?;
            for (release, result) in chunk.iter().zip(results) {
                match result {
                    Ok(mut values) => entries.append(&mut values),
                    Err(error) => self.warn(format!(
                        "{repo} {}: {}",
                        release.tag_name,
                        crate::http::github_fallback::diagnostic(&error)
                    )),
                }
            }
        }
        entries.sort_by(|a, b| {
            version_cmp(&b.version, &a.version)
                .then_with(|| a.id.cmp(&b.id))
                .then_with(|| a.sha256.cmp(&b.sha256))
        });
        entries.dedup_by(|a, b| a.id == b.id && a.version == b.version && a.sha256 == b.sha256);
        Ok(entries)
    }
    fn release(&self, repo: &str, release: &Release) -> Result<Vec<CatalogEntry>> {
        let mut names = BTreeSet::new();
        ensure!(
            release.assets.iter().all(|a| names.insert(&a.name)),
            "Duplicate release asset names"
        );
        if !release.assets.iter().any(|a| a.name.ends_with(".framely")) {
            return Ok(vec![]);
        }
        for asset in release.assets.iter().filter(|a| {
            a.name.ends_with(".framely") || matches!(a.name.as_str(), "release.json" | "SHA256SUMS")
        }) {
            asset.validate(repo, &release.tag_name)?;
        }
        let checksums = if let Some(asset) = release.assets.iter().find(|a| a.name == "SHA256SUMS")
        {
            Some(self.text(&asset.browser_download_url, 1024 * 1024)?)
        } else {
            None
        };
        if let Some(asset) = release.assets.iter().find(|a| a.name == "release.json") {
            let value = self.json(&asset.browser_download_url, 2 * 1024 * 1024)?;
            return parse_descriptor(value, repo, release, checksums.as_deref());
        }
        // Old releases are bound to the tag's resolved commit, never HEAD.
        let encoded: String =
            url::form_urlencoded::byte_serialize(release.tag_name.as_bytes()).collect();
        let commit = self.json(
            &format!("https://api.github.com/repos/{repo}/commits/{encoded}"),
            2 * 1024 * 1024,
        )?;
        let sha = commit["sha"].as_str().context("Missing release commit")?;
        ensure!(
            sha.len() == 40 && sha.bytes().all(|c| c.is_ascii_hexdigit()),
            "Invalid release commit"
        );
        let manifest = self.json(
            &format!("https://raw.githubusercontent.com/{repo}/{sha}/manifest.json"),
            2 * 1024 * 1024,
        )?;
        let version = manifest["version"]
            .as_str()
            .context("Missing legacy version")?;
        ensure!(
            release
                .tag_name
                .strip_prefix('v')
                .unwrap_or(&release.tag_name)
                == version,
            "Legacy release tag/version mismatch"
        );
        let id = manifest["id"].as_str().context("Missing plugin ID")?;
        let filename = format!("{id}-{version}.framely");
        let asset = if let Some(url) = manifest["downloadUrl"].as_str() {
            release
                .assets
                .iter()
                .find(|a| a.browser_download_url == url)
        } else {
            release.assets.iter().find(|a| a.name == filename)
        }
        .context("Missing legacy package asset")?;
        ensure!(
            asset.name.ends_with(".framely"),
            "Legacy downloadUrl does not select a plugin package"
        );
        asset.validate(repo, &release.tag_name)?;
        let digest =
            asset_hash(asset, checksums.as_deref())?.context("Legacy release has no SHA256")?;
        if let Some(declared) = manifest["downloadSha256"].as_str() {
            ensure!(
                declared.eq_ignore_ascii_case(&digest),
                "Legacy SHA256 mismatch"
            );
        }
        let mut value = manifest.clone();
        value["runAs"] = if manifest.get("backend").is_some_and(|b| !b.is_null())
            || manifest.get("lifecycle").is_some_and(|b| !b.is_null())
        {
            json!(manifest["backend"]["runAs"]
                .as_str()
                .or_else(|| manifest["lifecycle"]["runAs"].as_str())
                .unwrap_or("steamos"))
        } else {
            Value::Null
        };
        value["url"] = json!(asset.browser_download_url);
        value["sha256"] = json!(digest);
        value["icon"] = match manifest["publish"]["icon"].as_str() {
            Some(url) => json!(url),
            None => manifest["icon"]
                .as_str()
                .map(|p| pinned_image(repo, sha, p))
                .transpose()?
                .map_or(Value::Null, |s| json!(s)),
        };
        value["screenshots"] = manifest["publish"]["screenshots"]
            .as_array()
            .cloned()
            .map_or(json!([]), |v| json!(v));
        let mut entry: CatalogEntry = serde_json::from_value(value)?;
        validate_entry(&mut entry, release.prerelease)?;
        Ok(vec![entry])
    }
}
impl Asset {
    fn validate(&self, repo: &str, tag: &str) -> Result<()> {
        ensure!(
            !self.name.is_empty()
                && !self.name.contains(['/', '\\', '\r', '\n'])
                && self.size > 0
                && self.size <= 2 * 1024 * 1024 * 1024,
            "Invalid release asset"
        );
        let url = url::Url::parse(&self.browser_download_url)?;
        let prefix = format!("/{repo}/releases/download/{tag}/");
        let path = percent_decode(url.path())?;
        ensure!(
            url.scheme() == "https"
                && url.host_str() == Some("github.com")
                && url.port_or_known_default() == Some(443)
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && path == format!("{prefix}{}", self.name),
            "Release asset is outside its release"
        );
        Ok(())
    }
}
fn percent_decode(value: &str) -> Result<String> {
    // form_urlencoded decodes '+' as space; preserve literal '+' in URL paths.
    let encoded = format!("p={}", value.replace('+', "%2B"));
    Ok(url::form_urlencoded::parse(encoded.as_bytes())
        .next()
        .context("Invalid URL path")?
        .1
        .into_owned())
}
fn pinned_image(repo: &str, commit: &str, path: &str) -> Result<String> {
    crate::model::safe_path(path)?;
    Ok(format!(
        "https://raw.githubusercontent.com/{repo}/{commit}/{path}"
    ))
}
pub fn repository(source: &Source) -> Option<String> {
    if source.allow_http {
        return None;
    }
    let url = url::Url::parse(&source.url).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let repo = url.path().trim_matches('/');
    valid_repo(repo).ok()?;
    Some(repo.to_owned())
}
fn valid_repo(repo: &str) -> Result<()> {
    let parts: Vec<_> = repo.split('/').collect();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|p| !p.is_empty()
                && p.len() <= 100
                && *p != "."
                && *p != ".."
                && p.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))),
        "Invalid GitHub plugin repository"
    );
    Ok(())
}
pub fn source(repo: &str) -> Result<Source> {
    valid_repo(repo)?;
    Ok(Source {
        id: format!(
            "github-{}",
            &crate::package::digest(repo.to_ascii_lowercase().as_bytes())[..20]
        ),
        name: repo.chars().take(120).collect(),
        url: format!("https://github.com/{repo}"),
        enabled: true,
        allow_http: false,
    })
}
fn hex_hash(value: &str) -> Result<String> {
    ensure!(
        value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid release SHA256"
    );
    Ok(value.to_ascii_lowercase())
}
fn asset_hash(asset: &Asset, checksums: Option<&str>) -> Result<Option<String>> {
    let digest = asset
        .digest
        .as_deref()
        .map(|v| {
            hex_hash(
                v.strip_prefix("sha256:")
                    .context("Unsupported asset digest")?,
            )
        })
        .transpose()?;
    let mut checksum = None;
    if let Some(text) = checksums {
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            ensure!(
                line.len() > 66 && line.is_char_boundary(64) && line.is_char_boundary(66),
                "Invalid SHA256SUMS"
            );
            let bytes = line.as_bytes();
            ensure!(
                bytes[64] == b' ' && matches!(bytes[65], b' ' | b'*'),
                "Invalid SHA256SUMS"
            );
            let hash = hex_hash(&line[..64])?;
            if line[66..] == asset.name {
                ensure!(checksum.is_none(), "Duplicate SHA256SUMS entry");
                checksum = Some(hash);
            }
        }
        ensure!(checksum.is_some(), "Package missing from SHA256SUMS");
    }
    if let (Some(a), Some(b)) = (&digest, &checksum) {
        ensure!(a == b, "Asset digest/SHA256SUMS mismatch");
    }
    Ok(digest.or(checksum))
}
fn validate_entry(entry: &mut CatalogEntry, prerelease: bool) -> Result<()> {
    let version = semver::Version::parse(&entry.version)?;
    entry.validate(false)?;
    entry.compatible = Some(
        entry.api_version == API_VERSION
            && entry
                .engines
                .as_ref()
                .is_none_or(|e| e.matches(env!("CARGO_PKG_VERSION"))),
    );
    // GitHub's prerelease flag is retained separately from SemVer, including
    // releases tagged stable but marked prerelease in the API.
    entry.prerelease = prerelease || !version.pre.is_empty();
    Ok(())
}
fn parse_descriptor(
    value: Value,
    repo: &str,
    release: &Release,
    checksums: Option<&str>,
) -> Result<Vec<CatalogEntry>> {
    ensure!(
        value["schemaVersion"] == 1,
        "Unsupported release.json schema"
    );
    let plugins = value["plugins"]
        .as_array()
        .context("Missing release.json plugins")?;
    ensure!(
        !plugins.is_empty() && plugins.len() <= 100,
        "Invalid release.json plugin count"
    );
    let mut ids = BTreeSet::new();
    let mut entries = vec![];
    for plugin in plugins {
        ensure!(
            plugin.get("runAs").is_some(),
            "release.json must declare runAs"
        );
        let asset_name = plugin["asset"].as_str().context("Missing package asset")?;
        ensure!(
            asset_name.ends_with(".framely"),
            "Invalid plugin package extension"
        );
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .context("release.json package missing")?;
        asset.validate(repo, &release.tag_name)?;
        ensure!(
            plugin["size"].as_u64() == Some(asset.size),
            "release.json package size mismatch"
        );
        let hash = hex_hash(plugin["sha256"].as_str().context("Missing SHA256")?)?;
        if let Some(actual) = asset_hash(asset, checksums)? {
            ensure!(actual == hash, "release.json SHA256 mismatch");
        }
        let image = |value: &str| -> Result<String> {
            if let Some(asset) = release.assets.iter().find(|a| a.name == value) {
                asset.validate(repo, &release.tag_name)?;
                return Ok(asset.browser_download_url.clone());
            }
            validate_url(value, false)?;
            Ok(value.into())
        };
        let mut normalized = plugin.clone();
        normalized["url"] = json!(asset.browser_download_url);
        normalized["sha256"] = json!(hash);
        if let Some(icon) = plugin["icon"].as_str() {
            normalized["icon"] = json!(image(icon)?);
        }
        if let Some(screenshots) = plugin["screenshots"].as_array() {
            normalized["screenshots"] = json!(screenshots
                .iter()
                .map(|v| image(v.as_str().context("Invalid screenshot")?))
                .collect::<Result<Vec<_>>>()?);
        }
        let mut entry: CatalogEntry = serde_json::from_value(normalized)?;
        ensure!(
            ids.insert(entry.id.clone()),
            "Duplicate release.json plugin ID"
        );
        validate_entry(&mut entry, release.prerelease)?;
        entries.push(entry);
    }
    Ok(entries)
}
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(a), Ok(b)) => a.cmp_precedence(&b),
        _ => a.cmp(b),
    }
}
pub fn catalog(name: &str, entries: &[CatalogEntry]) -> Catalog {
    let mut groups = BTreeMap::<String, Vec<CatalogEntry>>::new();
    for entry in entries {
        groups
            .entry(entry.id.clone())
            .or_default()
            .push(entry.clone());
    }
    Catalog {
        schema_version: 1,
        name: name.into(),
        plugins: groups
            .into_values()
            .map(|versions| {
                versions
                    .iter()
                    .find(|v| v.compatible == Some(true))
                    .unwrap_or(&versions[0])
                    .clone()
            })
            .collect(),
    }
}
pub fn check_package(entry: &CatalogEntry, manifest: &Manifest) -> Result<()> {
    ensure!(
        entry.id == manifest.id
            && entry.version == manifest.version
            && entry.api_version == manifest.api_version
            && serde_json::to_value(&entry.engines)? == serde_json::to_value(&manifest.engines)?
            && entry.run_as
                == (manifest.backend.is_some() || manifest.lifecycle.is_some())
                    .then(|| manifest.run_as())
            && serde_json::to_value(entry.relations())?
                == serde_json::to_value(manifest.relations())?,
        "Plugin package declarations differ from GitHub release metadata"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Mock {
        routes: crate::http::github_fallback::TestRoutes,
        responses: std::sync::Arc<Mutex<BTreeMap<String, (u16, String)>>>,
        seen: std::sync::Arc<Mutex<Vec<(String, bool)>>>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    impl Mock {
        fn new(routes: Vec<(String, u16, String)>) -> Self {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let address = format!("http://{}", server.server_addr());
            let responses = std::sync::Arc::new(Mutex::new(BTreeMap::new()));
            let mut urls = vec![];
            for (i, (url, status, body)) in routes.into_iter().enumerate() {
                let path = format!("/{i}");
                responses
                    .lock()
                    .unwrap()
                    .insert(path.clone(), (status, body));
                urls.push((url, format!("{address}{path}")));
            }
            let seen = std::sync::Arc::new(Mutex::new(vec![]));
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker = {
                let responses = responses.clone();
                let seen = seen.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(std::sync::atomic::Ordering::Acquire) {
                        if let Some(request) =
                            server.recv_timeout(Duration::from_millis(20)).unwrap()
                        {
                            let conditional = request.headers().iter().any(|h| {
                                h.field.equiv("If-None-Match") && h.value.as_str() == "\"test\""
                            });
                            seen.lock()
                                .unwrap()
                                .push((request.url().into(), conditional));
                            let (status, body) = responses
                                .lock()
                                .unwrap()
                                .get(request.url())
                                .unwrap()
                                .clone();
                            request
                                .respond(
                                    tiny_http::Response::from_string(body)
                                        .with_status_code(status)
                                        .with_header(
                                            tiny_http::Header::from_bytes("ETag", "\"test\"")
                                                .unwrap(),
                                        ),
                                )
                                .unwrap();
                        }
                    }
                })
            };
            Self {
                routes: crate::http::github_fallback::TestRoutes::new(urls),
                responses,
                seen,
                stop,
                worker: Some(worker),
            }
        }
    }
    impl Drop for Mock {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Release);
            self.worker.take().unwrap().join().unwrap();
            let _ = &self.routes;
        }
    }
    fn asset(repo: &str, tag: &str, name: &str, hash: Option<&str>) -> Asset {
        Asset {
            name: name.into(),
            browser_download_url: format!(
                "https://github.com/{repo}/releases/download/{tag}/{name}"
            ),
            size: 123,
            digest: hash.map(|h| format!("sha256:{h}")),
        }
    }
    fn plugin(version: &str) -> Value {
        json!({"id":"example.test","name":"Test","version":version,"apiVersion":1,"author":"Author","description":"Test plugin", "asset":"plugin.framely","size":123,"sha256":"a".repeat(64),"runAs":null})
    }
    #[test]
    fn release_descriptor_validates_identity_hash_size_and_image_assets() {
        let release = Release {
            tag_name: "v1.0.0".into(),
            draft: false,
            prerelease: false,
            assets: vec![
                asset(
                    "example/test",
                    "v1.0.0",
                    "plugin.framely",
                    Some(&"a".repeat(64)),
                ),
                asset("example/test", "v1.0.0", "icon.png", None),
            ],
        };
        let mut p = plugin("1.0.0");
        p["icon"] = json!("icon.png");
        let entry = parse_descriptor(
            json!({"schemaVersion":1,"plugins":[p]}),
            "example/test",
            &release,
            None,
        )
        .unwrap()
        .remove(0);
        assert!(entry.icon.unwrap().ends_with("/v1.0.0/icon.png"));
        assert_eq!(entry.compatible, Some(true));
        for change in [
            json!({"size":122}),
            json!({"sha256":"b".repeat(64)}),
            json!({"asset":"missing.framely"}),
            json!({"icon":"../icon.png"}),
            json!({"version":"invalid"}),
        ] {
            let mut p = plugin("1.0.0");
            for (k, v) in change.as_object().unwrap() {
                p[k] = v.clone();
            }
            assert!(parse_descriptor(
                json!({"schemaVersion":1,"plugins":[p]}),
                "example/test",
                &release,
                None
            )
            .is_err());
        }
        let mut p = plugin("1.0.0");
        p.as_object_mut().unwrap().remove("runAs");
        assert!(parse_descriptor(
            json!({"schemaVersion":1,"plugins":[p]}),
            "example/test",
            &release,
            None
        )
        .is_err());
        assert!(parse_descriptor(
            json!({"schemaVersion":1,"plugins":[plugin("1.0.0"),plugin("1.0.0")]}),
            "example/test",
            &release,
            None
        )
        .is_err());
        assert!(parse_descriptor(
            json!({"schemaVersion":2,"plugins":[plugin("1.0.0")]}),
            "example/test",
            &release,
            None
        )
        .is_err());
        assert!(parse_descriptor(
            json!({"schemaVersion":1,"plugins":[plugin("1.0.0")]}),
            "example/test",
            &release,
            Some(&format!("{}  plugin.framely\n", "b".repeat(64)))
        )
        .is_err());
        assert!(source("../escape").is_err());
        assert!(source("owner/repo?token=x").is_err());
        assert!(repository(&Source {
            url: "https://github.com.evil/owner/repo".into(),
            ..source("owner/repo").unwrap()
        })
        .is_none());
    }
    #[test]
    fn release_version_precedence_and_package_metadata_checks() {
        assert!(version_cmp("1.0.0-preview.10", "1.0.0-preview.9").is_gt());
        assert!(version_cmp("1.0.0", "1.0.0-preview.10").is_gt());
        let release = Release {
            tag_name: "v1.0.0".into(),
            draft: false,
            prerelease: true,
            assets: vec![asset("example/test", "v1.0.0", "plugin.framely", None)],
        };
        let entry = parse_descriptor(
            json!({"schemaVersion":1,"plugins":[plugin("1.0.0")]}),
            "example/test",
            &release,
            None,
        )
        .unwrap()
        .remove(0);
        assert!(entry.prerelease);
        let mut manifest: Manifest=serde_json::from_value(json!({"schemaVersion":1,"apiVersion":1,"id":"example.test","name":"Test","version":"1.0.0","author":"Author","files":{},"ui":{}})).unwrap();
        check_package(&entry, &manifest).unwrap();
        manifest.api_version = 2;
        assert!(check_package(&entry, &manifest).is_err());
        manifest.api_version = 1;
        manifest.dependencies.insert(
            "example.dep".into(),
            crate::relations::Dependency::Version("^1.0.0".into()),
        );
        assert!(check_package(&entry, &manifest).is_err());
    }
    #[test]
    fn github_metadata_cache_revalidates_and_only_network_failures_use_stale_data() {
        let url = "https://api.github.com/repos/cache/test/releases?per_page=100&page=1";
        let mock = Mock::new(vec![(url.into(), 200, "[]".into())]);
        let root = tempfile::tempdir().unwrap();
        let cancel = Cancellation::default();
        let proxy = ProxySettings {
            github_auto_fallback: false,
            ..Default::default()
        };
        let client = Client::new(&proxy, &cancel, root.path().into(), false);
        assert_eq!(client.text(url, 100).unwrap(), "[]");
        assert_eq!(client.text(url, 100).unwrap(), "[]");
        assert_eq!(mock.seen.lock().unwrap().len(), 1);
        mock.responses
            .lock()
            .unwrap()
            .insert("/0".into(), (304, String::new()));
        let refresh = Client::new(&proxy, &cancel, root.path().into(), true);
        assert_eq!(refresh.text(url, 100).unwrap(), "[]");
        assert!(mock.seen.lock().unwrap()[1].1);
        mock.responses
            .lock()
            .unwrap()
            .insert("/0".into(), (503, "unavailable".into()));
        assert_eq!(refresh.text(url, 100).unwrap(), "[]");
        assert_eq!(refresh.warnings.lock().unwrap().len(), 1);
        mock.responses
            .lock()
            .unwrap()
            .insert("/0".into(), (404, "missing".into()));
        assert!(refresh.text(url, 100).is_err());
        cancel.stop();
        assert!(client.text(url, 100).is_err());
    }
    #[test]
    fn github_discovery_paginates_and_filters_forks_archives_and_missing_topics() {
        let url = |page: usize| {
            let mut url = url::Url::parse("https://api.github.com/search/repositories").unwrap();
            url.query_pairs_mut()
                .append_pair("q", TOPIC)
                .append_pair("sort", "stars")
                .append_pair("order", "desc")
                .append_pair("per_page", "100")
                .append_pair("page", &page.to_string());
            url.to_string()
        };
        let item = |name: String| json!({"full_name":name,"archived":false,"fork":false,"topics":["framely-plugin"]});
        let first: Vec<_> = (0..100)
            .map(|i| item(format!("fixture/plugin-{i}")))
            .collect();
        let mut fork = item("fixture/fork".into());
        fork["fork"] = json!(true);
        let mut archived = item("fixture/archive".into());
        archived["archived"] = json!(true);
        let mut other = item("fixture/other".into());
        other["topics"] = json!([]);
        let mock=Mock::new(vec![(url(1),200,json!({"total_count":104,"incomplete_results":false,"items":first}).to_string()),
            (url(2),200,json!({"total_count":104,"incomplete_results":false,"items":[fork,archived,other,item("fixture/final".into())]}).to_string())]);
        let root = tempfile::tempdir().unwrap();
        let cancel = Cancellation::default();
        let proxy = ProxySettings::default();
        let results = Client::new(&proxy, &cancel, root.path().into(), false)
            .discover()
            .unwrap();
        assert_eq!(results.len(), 101);
        assert_eq!(results.last().unwrap().name, "fixture/final");
        assert_eq!(mock.seen.lock().unwrap().len(), 2);
        assert_eq!(
            source("Fixture/Final").unwrap().id,
            results.last().unwrap().id
        );
    }
    #[test]
    fn github_releases_read_unsorted_new_and_legacy_metadata_without_downloading_packages() {
        let repo = "fixture/releases";
        let commit = "c".repeat(40);
        let hash = "a".repeat(64);
        let releases = ["1.0.0-preview.9", "1.0.0-preview.10", "1.0.0"];
        let mut pages = vec![];
        let mut routes = vec![];
        for version in releases {
            let tag = format!("v{version}");
            let name = format!("example.test-{version}.framely");
            let package = asset(repo, &tag, &name, Some(&hash));
            let mut assets = vec![package];
            if version == "1.0.0" {
                let manifest = json!({"schemaVersion":1,"apiVersion":1,"id":"example.test","name":"Test","version":version,"author":"Author","description":"Test plugin","icon":"icon.png","files":{},"ui":{}});
                routes.push((
                    format!("https://api.github.com/repos/{repo}/commits/{tag}"),
                    200,
                    json!({"sha":commit}).to_string(),
                ));
                routes.push((
                    format!("https://raw.githubusercontent.com/{repo}/{commit}/manifest.json"),
                    200,
                    manifest.to_string(),
                ));
            } else {
                let descriptor = asset(repo, &tag, "release.json", None);
                let mut p = plugin(version);
                p["asset"] = json!(name);
                routes.push((
                    descriptor.browser_download_url.clone(),
                    200,
                    json!({"schemaVersion":1,"plugins":[p]}).to_string(),
                ));
                assets.push(descriptor);
            }
            pages.push(
                json!({"tag_name":tag,"draft":false,"prerelease":version!="1.0.0","assets":assets}),
            );
        }
        routes.push((
            format!("https://api.github.com/repos/{repo}/releases?per_page=100&page=1"),
            200,
            json!(pages).to_string(),
        ));
        let mock = Mock::new(routes);
        let root = tempfile::tempdir().unwrap();
        let cancel = Cancellation::default();
        let proxy = ProxySettings {
            github_auto_fallback: false,
            ..Default::default()
        };
        let client = Client::new(&proxy, &cancel, root.path().into(), false);
        let entries = client.entries(repo).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|p| p.version.as_str())
                .collect::<Vec<_>>(),
            vec!["1.0.0", "1.0.0-preview.10", "1.0.0-preview.9"]
        );
        assert!(entries[0].icon.as_ref().unwrap().contains(&commit));
        assert!(client.warnings.lock().unwrap().is_empty());
        assert_eq!(mock.seen.lock().unwrap().len(), 5);
        assert_eq!(catalog("Test", &entries).plugins.len(), 1);
    }
    #[test]
    fn invalid_new_release_never_falls_back_to_legacy_manifest() {
        let repo = "fixture/invalid";
        let release = Release {
            tag_name: "v1.0.0".into(),
            draft: false,
            prerelease: false,
            assets: vec![
                asset(repo, "v1.0.0", "plugin.framely", None),
                asset(repo, "v1.0.0", "release.json", None),
            ],
        };
        let mock = Mock::new(vec![(
            release.assets[1].browser_download_url.clone(),
            200,
            "{broken".into(),
        )]);
        let root = tempfile::tempdir().unwrap();
        let cancel = Cancellation::default();
        let proxy = ProxySettings::default();
        assert!(Client::new(&proxy, &cancel, root.path().into(), false)
            .release(repo, &release)
            .is_err());
        assert_eq!(mock.seen.lock().unwrap().len(), 1);
        assert!(asset_hash(&release.assets[0], None).unwrap().is_none());
        assert!(asset_hash(
            &release.assets[0],
            Some(&format!("{}  plugin.framely", "a".repeat(64)))
        )
        .unwrap()
        .is_some());
        assert!(asset_hash(
            &release.assets[0],
            Some(&format!("{}é  plugin.framely", "a".repeat(63)))
        )
        .is_err());
    }
    #[test]
    fn generated_release_uses_verified_packages_and_sources_wait_for_confirmation() {
        let root = tempfile::tempdir().unwrap();
        crate::tests::fixture(root.path(), "example.test", "1.0.0", 0, None);
        let manifest_path = root.path().join("manifest-1.0.0-0.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["icon"] = json!("icon.png");
        manifest["screenshots"] = json!(["icon.png"]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let icon = fs::read("assets/branding/icons/64.png").unwrap();
        fs::write(root.path().join("payload-1.0.0-0/icon.png"), &icon).unwrap();
        let output = root.path().join("1.0.0-0.framely");
        crate::package::pack(
            &manifest_path,
            &root.path().join("payload-1.0.0-0"),
            &output,
        )
        .unwrap();
        let bytes = fs::read(output).unwrap();
        let value = release_json(root.path()).unwrap();
        assert_eq!(
            value["plugins"][0]["sha256"],
            crate::package::digest(&bytes)
        );
        assert_eq!(value["plugins"][0]["size"], bytes.len());
        assert!(value["plugins"][0]["runAs"].is_null());
        assert_eq!(value["plugins"][0]["icon"], "example.test-1.0.0-icon.png");
        assert_eq!(
            fs::read(root.path().join("example.test-1.0.0-icon.png")).unwrap(),
            icon
        );
        assert_eq!(
            fs::read(root.path().join("example.test-1.0.0-screen-0.png")).unwrap(),
            icon
        );
        let source = source("example/test").unwrap();
        let mut db = Database::default();
        let staged = crate::package::Staged::from_bytes(&bytes).unwrap();
        let prepared = crate::planner::prepare_staged_sources(
            &db,
            staged.clone(),
            Some(source.id.clone()),
            Default::default(),
            Default::default(),
            vec![source.clone()],
        )
        .unwrap();
        assert_eq!(prepared.plan.add_sources.len(), 1);
        assert!(db.sources.is_empty());
        db.sources.push(Source {
            enabled: false,
            ..source.clone()
        });
        assert!(crate::planner::prepare_staged_sources(
            &db,
            staged,
            Some(source.id.clone()),
            Default::default(),
            Default::default(),
            vec![source]
        )
        .is_err());
    }
}
pub fn release_json(packages: &Path) -> Result<Value> {
    let mut plugins = vec![];
    let mut ids = BTreeSet::new();
    let mut paths = fs::read_dir(packages)?
        .map(|p| p.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    for path in paths
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "framely"))
    {
        let bytes = fs::read(&path)?;
        let verified = crate::package::verify(&bytes)?;
        let manifest = verified.manifest;
        ensure!(
            ids.insert(manifest.id.clone()),
            "Duplicate plugin ID in release"
        );
        semver::Version::parse(&manifest.version)?;
        let mut value = serde_json::to_value(&manifest)?;
        for key in [
            "schemaVersion",
            "backend",
            "ui",
            "files",
            "publish",
            "downloadUrl",
            "lifecycle",
            "icon",
            "screenshots",
        ] {
            value.as_object_mut().unwrap().remove(key);
        }
        value["asset"] = json!(path
            .file_name()
            .context("Missing asset name")?
            .to_str()
            .context("Invalid asset name")?);
        value["size"] = json!(bytes.len());
        value["sha256"] = json!(crate::package::digest(&bytes));
        value["runAs"] =
            json!((manifest.backend.is_some() || manifest.lifecycle.is_some())
                .then(|| manifest.run_as()));
        let export_image = |label: &str, path: &str| -> Result<String> {
            let data = verified.files.get(path).context("Missing packaged image")?;
            let ext = Path::new(path)
                .extension()
                .and_then(|v| v.to_str())
                .context("Missing image extension")?;
            let name = format!("{}-{}-{label}.{ext}", manifest.id, manifest.version);
            let output = packages.join(&name);
            if output.exists() {
                ensure!(
                    fs::read(&output)? == *data,
                    "Refusing to replace a different release image: {name}"
                );
            } else {
                let mut file = tempfile::NamedTempFile::new_in(packages)?;
                file.write_all(data)?;
                file.as_file().sync_all()?;
                file.persist_noclobber(&output)?;
            }
            Ok(name)
        };
        let publish = manifest.publish.as_ref();
        if let Some(icon) = publish.and_then(|p| p.icon.clone()) {
            value["icon"] = json!(icon);
        } else if let Some(icon) = &manifest.icon {
            value["icon"] = json!(export_image("icon", icon)?);
        }
        value["screenshots"] =
            if let Some(images) = publish.map(|p| &p.screenshots).filter(|s| !s.is_empty()) {
                json!(images)
            } else {
                json!(manifest
                    .screenshots
                    .iter()
                    .enumerate()
                    .map(|(i, p)| export_image(&format!("screen-{i}"), p))
                    .collect::<Result<Vec<_>>>()?)
            };
        plugins.push(value);
    }
    ensure!(!plugins.is_empty(), "No .framely packages found");
    Ok(json!({"schemaVersion":1,"plugins":plugins}))
}
