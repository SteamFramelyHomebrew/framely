use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path};

pub const API_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunAs {
    #[default]
    Steamos,
    Root,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Backend {
    pub entry: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub run_as: RunAs,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub restart: RestartPolicy,
    #[serde(default = "restart_limit")]
    pub restart_limit: u32,
    #[serde(
        rename = "memoryLimitMiB",
        default = "default_memory_limit_mib",
        skip_serializing_if = "is_default_memory_limit_mib"
    )]
    pub memory_limit_mib: u32,
}
pub const DEFAULT_MEMORY_LIMIT_MIB: u32 = 512;
fn default_memory_limit_mib() -> u32 {
    DEFAULT_MEMORY_LIMIT_MIB
}
fn is_default_memory_limit_mib(value: &u32) -> bool {
    *value == DEFAULT_MEMORY_LIMIT_MIB
}
fn restart_limit() -> u32 {
    3
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    Never,
    #[default]
    OnFailure,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Hook {
    pub entry: String,
    #[serde(default)]
    pub args: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lifecycle {
    #[serde(default)]
    pub run_as: Option<RunAs>,
    #[serde(default)]
    pub on_install: Option<Hook>,
    #[serde(default)]
    pub on_update: Option<Hook>,
    #[serde(default)]
    pub on_uninstall: Option<Hook>,
    #[serde(default)]
    pub on_crash_cleanup: Option<Hook>,
    #[serde(default)]
    pub on_start: bool,
    #[serde(default)]
    pub on_stop: bool,
    #[serde(default = "hook_timeout")]
    pub timeout_seconds: u64,
}
fn hook_timeout() -> u64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Window {
    pub entry: String,
    pub title: String,
    #[serde(default)]
    pub dock_icon: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_web: bool,
    #[serde(default = "window_width")]
    pub width: u32,
    #[serde(default = "window_height")]
    pub height: u32,
    #[serde(
        default = "window_width_meters",
        skip_serializing_if = "Option::is_none"
    )]
    pub width_meters: Option<f32>,
}
fn window_width() -> u32 {
    1600
}
fn window_height() -> u32 {
    900
}
fn window_width_meters() -> Option<f32> {
    Some(3.0)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ui {
    pub quick_page: Option<String>,
    #[serde(default)]
    pub windows: BTreeMap<String, Window>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Publish {
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub screenshots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub details: String,
    // Accept old packages and catalogs without exporting their retired category.
    #[serde(default, rename = "category", skip_serializing)]
    pub _legacy_category: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub screenshots: Vec<String>,
    #[serde(default)]
    pub changelog: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, crate::relations::Dependency>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub optional_dependencies: BTreeMap<String, crate::relations::Dependency>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub conflicts: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclusive_resources: Vec<String>,
    pub backend: Option<Backend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<Lifecycle>,
    #[serde(default)]
    pub ui: Ui,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish: Option<Publish>,
    pub files: BTreeMap<String, String>,
}
impl Manifest {
    pub fn relations(&self) -> crate::relations::Relations {
        crate::relations::Relations {
            dependencies: self.dependencies.clone(),
            optional_dependencies: self.optional_dependencies.clone(),
            conflicts: self.conflicts.clone(),
            exclusive_resources: self.exclusive_resources.clone(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.api_version == API_VERSION,
            "Unsupported manifest/API version"
        );
        valid_id(&self.id)?;
        self.relations().validate(&self.id, &self.version)?;
        for link in self
            .author_url
            .iter()
            .chain(self.documentation_url.iter())
            .chain(self.homepage.iter())
        {
            validate_web_link(link)?;
        }
        if let Some(url) = &self.download_url {
            validate_url(url, false)?;
        }
        if let Some(publish) = &self.publish {
            ensure!(publish.screenshots.len() <= 8, "Too many store screenshots");
            for image in publish.icon.iter().chain(publish.screenshots.iter()) {
                validate_url(image, false)?;
            }
        }
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 120,
            "Invalid plugin name"
        );
        ensure!(
            !self.author.trim().is_empty() && self.author.len() <= 120,
            "Invalid author"
        );
        ensure!(
            !self.version.is_empty()
                && self.version.len() <= 64
                && self
                    .version
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".-+".contains(&c)),
            "Invalid version"
        );
        ensure!(
            self.files.len() <= 2048 && !self.files.is_empty(),
            "Invalid file count"
        );
        for (path, hash) in &self.files {
            safe_path(path)?;
            ensure!(
                hash.len() == 64 && hex::decode(hash)?.len() == 32,
                "Invalid SHA256"
            );
        }
        let check = |p: &str| -> Result<()> {
            safe_path(p)?;
            ensure!(self.files.contains_key(p), "Entry is not hashed: {p}");
            Ok(())
        };
        validate_presentation(
            &self.details,
            &self.tags,
            &self.screenshots,
            &self.changelog,
        )?;
        if let Some(icon) = &self.icon {
            check(icon)?;
            ensure!(icon.ends_with(".png"), "Plugin icon must be PNG");
        }
        for screenshot in &self.screenshots {
            check(screenshot)?;
            ensure!(
                screenshot.ends_with(".png")
                    || screenshot.ends_with(".jpg")
                    || screenshot.ends_with(".jpeg"),
                "Screenshot must be PNG/JPEG"
            );
        }
        if let Some(b) = &self.backend {
            check(&b.entry)?;
            ensure!(
                b.memory_limit_mib > 0,
                "Backend memoryLimitMiB must be a positive integer"
            );
            ensure!((1..=10).contains(&b.restart_limit), "Invalid restart limit");
            ensure!(
                b.args.len() <= 64 && b.args.iter().all(|a| a.len() <= 4096 && !a.contains('\0')),
                "Invalid backend arguments"
            );
        }
        if let Some(l) = &self.lifecycle {
            ensure!(
                (1..=15).contains(&l.timeout_seconds),
                "Lifecycle timeout must be 1–15 seconds"
            );
            ensure!(
                self.backend.is_some() || (!l.on_start && !l.on_stop),
                "Start/stop hooks require a backend"
            );
            if let (Some(b), Some(user)) = (&self.backend, l.run_as) {
                ensure!(
                    b.run_as == user,
                    "Lifecycle and backend must use the same user"
                );
            }
            for hook in [
                &l.on_install,
                &l.on_update,
                &l.on_uninstall,
                &l.on_crash_cleanup,
            ]
            .into_iter()
            .flatten()
            {
                check(&hook.entry)?;
                ensure!(
                    hook.args.len() <= 64
                        && hook
                            .args
                            .iter()
                            .all(|a| a.len() <= 4096 && !a.contains('\0')),
                    "Invalid hook arguments"
                );
            }
        }
        if let Some(p) = &self.ui.quick_page {
            check(p)?;
        }
        ensure!(self.ui.windows.len() <= 8, "Too many declared windows");
        for (key, w) in &self.ui.windows {
            valid_id(key)?;
            check(&w.entry)?;
            ensure!(
                (640..=2560).contains(&w.width) && (360..=1440).contains(&w.height),
                "Window size outside supported range"
            );
            if let Some(width) = w.width_meters {
                ensure!(
                    width.is_finite() && (0.4..=4.0).contains(&width),
                    "Invalid physical window width"
                );
            }
            ensure!(
                !w.title.is_empty() && w.title.len() <= 120,
                "Invalid window title"
            );
        }
        Ok(())
    }
    pub fn memory_limit_mib(&self) -> u32 {
        self.backend
            .as_ref()
            .map(|b| b.memory_limit_mib)
            .unwrap_or(DEFAULT_MEMORY_LIMIT_MIB)
    }
    pub fn run_as(&self) -> RunAs {
        self.backend
            .as_ref()
            .map(|b| b.run_as)
            .or_else(|| self.lifecycle.as_ref().and_then(|l| l.run_as))
            .unwrap_or_default()
    }
}
fn validate_presentation(
    details: &str,
    tags: &[String],
    screenshots: &[String],
    changelog: &str,
) -> Result<()> {
    ensure!(
        details.len() <= 32768 && changelog.len() <= 16384,
        "Plugin description too long"
    );
    ensure!(
        tags.len() <= 12 && screenshots.len() <= 8,
        "Invalid plugin presentation"
    );
    ensure!(
        tags.iter().all(|t| !t.trim().is_empty() && t.len() <= 80),
        "Invalid plugin tags"
    );
    Ok(())
}
pub fn valid_id(s: &str) -> Result<()> {
    ensure!(
        !s.is_empty()
            && s.len() <= 80
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-_".contains(&b))
            && !s.starts_with('.')
            && !s.contains(".."),
        "Invalid identifier"
    );
    Ok(())
}
pub fn safe_path(s: &str) -> Result<()> {
    ensure!(
        !s.is_empty()
            && s.len() <= 512
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/._-+".contains(&c)),
        "Invalid package path"
    );
    for c in Path::new(s).components() {
        if !matches!(c, Component::Normal(_)) {
            bail!("Unsafe package path: {s}");
        }
    }
    ensure!(
        !s.split('/').any(|p| p.is_empty() || p == "." || p == ".."),
        "Unsafe package path"
    );
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub manifest: Manifest,
    pub enabled: bool,
    pub favorite: bool,
    pub order: u32,
    pub source: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub allow_http: bool,
}
fn yes() -> bool {
    true
}
impl Source {
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        ensure!(
            !self.name.is_empty() && self.name.len() <= 120,
            "Invalid source name"
        );
        validate_url(&self.url, self.allow_http)
    }
}
/// User-facing web links may include fragments, but cannot launch other URI schemes.
pub fn validate_web_link(value: &str) -> Result<()> {
    ensure!(
        value.len() <= 2048 && !value.chars().any(char::is_whitespace),
        "Invalid web link"
    );
    let url = url::Url::parse(value)?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "Invalid web link"
    );
    Ok(())
}

pub fn validate_url(url: &str, allow_http: bool) -> Result<()> {
    ensure!(
        url.len() <= 2048
            && (url.starts_with("https://") || (allow_http && url.starts_with("http://"))),
        "An HTTPS URL is required (HTTP must be explicitly enabled)"
    );
    ensure!(
        !url.contains('@') && !url.contains('#') && !url.chars().any(char::is_whitespace),
        "Invalid URL"
    );
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkPanel {
    pub enabled: bool,
    pub port: u16,
    #[serde(default)]
    pub password_enabled: bool,
}
impl Default for NetworkPanel {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 15915,
            password_enabled: false,
        }
    }
}
impl NetworkPanel {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.port >= 1024, "端口必须在 1024–65535 之间");
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProxySettings {
    #[serde(default)]
    pub http_enabled: bool,
    #[serde(default)]
    pub github_enabled: bool,
    #[serde(default)]
    pub http: String,
    #[serde(default)]
    pub github: String,
}
impl ProxySettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.http_enabled || !self.http.is_empty(),
            "请填写 HTTP 代理地址"
        );
        ensure!(
            !self.github_enabled || !self.github.is_empty(),
            "请填写 GitHub 代理地址"
        );
        for (name, value) in [("HTTP", &self.http), ("GitHub", &self.github)] {
            if value.is_empty() {
                continue;
            }
            ensure!(
                value.len() <= 2048 && !value.chars().any(char::is_whitespace),
                "{name} 代理地址无效"
            );
            let parsed =
                url::Url::parse(value).map_err(|_| anyhow::anyhow!("{name} 代理地址无效"))?;
            ensure!(
                matches!(parsed.scheme(), "http" | "https")
                    && parsed.host_str().is_some()
                    && parsed.username().is_empty()
                    && parsed.password().is_none()
                    && parsed.query().is_none()
                    && parsed.fragment().is_none(),
                "{name} 代理请填写不含账号密码的 HTTP(S) 地址"
            );
            if name == "HTTP" {
                ensure!(parsed.path() == "/", "HTTP 代理不能包含路径");
            }
        }
        Ok(())
    }
}

pub fn default_language() -> String {
    "auto".into()
}
pub const AGREEMENT_VERSION: &str = "2026-10-03.1";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgreementAcceptance {
    pub version: String,
    pub accepted_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Database {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub agreement_acceptance: Option<AgreementAcceptance>,
    #[serde(default)]
    pub proxy: ProxySettings,
    #[serde(default)]
    pub network_panel: NetworkPanel,
    pub plugins: BTreeMap<String, Installed>,
    pub sources: Vec<Source>,
    #[serde(default)]
    pub default_sources_initialized: bool,
    pub safe_mode: bool,
    #[serde(default)]
    pub subscriptions: Vec<crate::subscriptions::Subscription>,
    #[serde(default)]
    pub source_origins: BTreeMap<String, crate::subscriptions::Origins>,
    #[serde(default)]
    pub source_urls: BTreeMap<String, String>,
    #[serde(default)]
    pub source_enabled: BTreeMap<String, bool>,

    #[serde(default)]
    pub update_source: Option<UpdateSource>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub icon: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Notification {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default = "ttl")]
    pub duration_ms: u64,
}
fn ttl() -> u64 {
    8000
}
impl Notification {
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        ensure!(
            self.title.len() <= 160 && self.body.len() <= 4096 && self.actions.len() <= 3,
            "Notification limit exceeded"
        );
        ensure!(
            (1000..=60000).contains(&self.duration_ms),
            "Invalid notification duration"
        );
        let mut ids = std::collections::BTreeSet::new();
        for a in &self.actions {
            valid_id(&a.id)?;
            ensure!(
                !a.label.is_empty()
                    && a.label.len() <= 80
                    && a.icon.len() <= 16
                    && ids.insert(&a.id),
                "Invalid/duplicate action"
            );
        }
        if let Some(i) = &self.image {
            ensure!(i.len() <= 1024 * 1024, "Notification image is too large");
            ensure!(
                i.starts_with("data:image/png;base64,")
                    || i.starts_with("data:image/jpeg;base64,")
                    || i.starts_with("https://"),
                "Unsupported image URL"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub schema_version: u32,
    pub name: String,
    pub plugins: Vec<CatalogEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    pub api_version: u32,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub run_as: Option<RunAs>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub details: String,
    // Accept old packages and catalogs without exporting their retired category.
    #[serde(default, rename = "category", skip_serializing)]
    pub _legacy_category: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub screenshots: Vec<String>,
    #[serde(default)]
    pub changelog: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, crate::relations::Dependency>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub optional_dependencies: BTreeMap<String, crate::relations::Dependency>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub conflicts: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclusive_resources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginVersions {
    pub schema_version: u32,
    pub id: String,
    pub versions: Vec<CatalogEntry>,
}

impl CatalogEntry {
    pub fn relations(&self) -> crate::relations::Relations {
        crate::relations::Relations {
            dependencies: self.dependencies.clone(),
            optional_dependencies: self.optional_dependencies.clone(),
            conflicts: self.conflicts.clone(),
            exclusive_resources: self.exclusive_resources.clone(),
        }
    }

    pub fn validate(&self, allow_http: bool) -> Result<()> {
        valid_id(&self.id)?;
        self.relations().validate(&self.id, &self.version)?;
        for link in self
            .author_url
            .iter()
            .chain(self.documentation_url.iter())
            .chain(self.homepage.iter())
        {
            validate_web_link(link)?;
        }
        validate_url(&self.url, allow_http)?;
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 120
                && self.author.len() <= 120
                && self.description.len() <= 4096
                && self.version.len() <= 64,
            "Invalid catalog entry"
        );
        ensure!(
            hex::decode(&self.sha256)?.len() == 32,
            "Invalid catalog integrity metadata"
        );
        validate_presentation(
            &self.details,
            &self.tags,
            &self.screenshots,
            &self.changelog,
        )?;
        if let Some(icon) = &self.icon {
            validate_url(icon, allow_http)?;
        }
        for screenshot in &self.screenshots {
            validate_url(screenshot, allow_http)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSource {
    pub url: String,
}
impl UpdateSource {
    pub fn validate(&self) -> Result<()> {
        validate_url(&self.url, false)?;
        Ok(())
    }
}

impl Default for Database {
    fn default() -> Self {
        serde_json::from_value(serde_json::json!({"plugins":{},"sources":[],"safeMode":false}))
            .expect("valid database defaults")
    }
}
