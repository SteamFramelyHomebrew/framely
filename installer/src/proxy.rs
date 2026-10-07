//! Read proxy settings without changing the computer or the SSH device.
use anyhow::{Result, bail, ensure};
use std::{collections::HashMap, net::IpAddr};
use url::Url;

/// Installer-only download preferences. Explicit HTTP proxy overrides automatic
/// discovery; GitHub routing rewrites only trusted GitHub source hosts.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct DownloadSettings {
    pub system: bool,
    pub http: String,
    pub github: String,
}
impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            system: true,
            http: String::new(),
            github: String::new(),
        }
    }
}
impl DownloadSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.http.len() <= 2048 && self.github.len() <= 2048,
            "代理地址过长"
        );
        if !self.http.is_empty() {
            parse_proxy(&self.http)?;
        }
        if !self.github.is_empty() {
            let url =
                Url::parse(&self.github).map_err(|_| anyhow::anyhow!("GitHub 代理地址无效"))?;
            ensure!(
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "GitHub 代理必须是 HTTPS 地址，不能含账号、查询参数或片段"
            );
        }
        Ok(())
    }
    pub fn route(&self, url: &Url) -> Url {
        if !self.github.is_empty()
            && matches!(
                url.host_str(),
                Some(
                    "github.com"
                        | "api.github.com"
                        | "raw.githubusercontent.com"
                        | "objects.githubusercontent.com"
                        | "release-assets.githubusercontent.com"
                )
            )
            && !url
                .as_str()
                .starts_with(&format!("{}/", self.github.trim_end_matches('/')))
        {
            Url::parse(&format!("{}/{}", self.github.trim_end_matches('/'), url))
                .expect("validated GitHub proxy")
        } else {
            url.clone()
        }
    }
    pub fn proxy_for(&self, url: &Url) -> Result<Option<ureq::Proxy>> {
        if !self.http.is_empty() {
            return parse_proxy(&self.http).map(Some);
        }
        if self.system { for_url(url) } else { Ok(None) }
    }
}
static DOWNLOAD_SETTINGS: std::sync::OnceLock<std::sync::RwLock<DownloadSettings>> =
    std::sync::OnceLock::new();
fn active_settings() -> &'static std::sync::RwLock<DownloadSettings> {
    DOWNLOAD_SETTINGS.get_or_init(|| std::sync::RwLock::new(DownloadSettings::default()))
}
pub fn current() -> DownloadSettings {
    active_settings().read().unwrap().clone()
}
fn settings_path() -> Result<std::path::PathBuf> {
    Ok(
        directories::ProjectDirs::from("org", "Framely", "Installer")
            .ok_or_else(|| anyhow::anyhow!("无法确定配置目录"))?
            .config_dir()
            .join("download-settings.json"),
    )
}
pub fn load() -> Result<DownloadSettings> {
    let path = settings_path()?;
    let settings = if path.exists() {
        serde_json::from_slice::<DownloadSettings>(&std::fs::read(path)?)?
    } else {
        DownloadSettings::default()
    };
    settings.validate()?;
    *active_settings().write().unwrap() = settings.clone();
    Ok(settings)
}
pub fn save(settings: DownloadSettings) -> Result<()> {
    settings.validate()?;
    let path = settings_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(&settings)?)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    *active_settings().write().unwrap() = settings;
    Ok(())
}

#[derive(Default)]
struct Settings {
    proxies: HashMap<String, String>,
    bypass: Vec<String>,
    exclude_simple: bool,
    reverse_bypass: bool,
}
impl Settings {
    fn select(&self, url: &Url) -> Option<&str> {
        let host = url.host_str()?;
        let excluded = self.exclude_simple && !host.contains('.')
            || self
                .bypass
                .iter()
                .any(|pattern| bypass_matches(pattern, host, url.port_or_known_default()));
        if excluded != self.reverse_bypass {
            return None;
        }
        self.proxies
            .get(url.scheme())
            .or_else(|| self.proxies.get("*"))
            .map(String::as_str)
    }
}
fn env_settings(mut get: impl FnMut(&str) -> Option<String>) -> Option<Settings> {
    let mut settings = Settings::default();
    for (scheme, lower, upper) in [
        ("https", "https_proxy", "HTTPS_PROXY"),
        ("http", "http_proxy", "HTTP_PROXY"),
        ("*", "all_proxy", "ALL_PROXY"),
    ] {
        if let Some(value) = get(lower).or_else(|| get(upper)) {
            settings.proxies.insert(scheme.into(), value);
        }
    }
    (!settings.proxies.is_empty()).then_some(settings)
}
pub fn for_url(url: &Url) -> Result<Option<ureq::Proxy>> {
    if let Ok(bypass) = std::env::var("no_proxy").or_else(|_| std::env::var("NO_PROXY")) {
        if split_bypass(&bypass).iter().any(|pattern| {
            bypass_matches(
                pattern,
                url.host_str().unwrap_or(""),
                url.port_or_known_default(),
            )
        }) {
            return Ok(None);
        }
    }
    let settings = match env_settings(|key| std::env::var(key).ok()) {
        Some(settings) => settings,
        None => system_settings()?.unwrap_or_default(),
    };
    settings
        .select(url)
        .filter(|value| !value.trim().is_empty())
        .map(parse_proxy)
        .transpose()
}
fn parse_proxy(value: &str) -> Result<ureq::Proxy> {
    let value = value.trim();
    let value = if value.contains("://") {
        value.to_owned()
    } else {
        format!("http://{value}")
    };
    let value = value
        .strip_prefix("socks5h://")
        .map(|rest| format!("socks5://{rest}"))
        .unwrap_or(value);
    let parsed = Url::parse(&value)?;
    ensure!(
        matches!(
            parsed.scheme(),
            "http" | "socks" | "socks4" | "socks4a" | "socks5"
        ) && parsed.host_str().is_some(),
        "系统代理地址无效或协议不受支持"
    );
    ensure!(
        !matches!(parsed.host(), Some(url::Host::Ipv6(_))),
        "当前下载客户端暂不支持 IPv6 代理地址，请使用主机名或 IPv4 地址"
    );
    ureq::Proxy::new(value).map_err(|_| anyhow::anyhow!("系统代理地址无效"))
}
fn split_bypass(value: &str) -> Vec<String> {
    value
        .split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
fn bypass_matches(pattern: &str, host: &str, port: Option<u16>) -> bool {
    let pattern = pattern.trim().to_ascii_lowercase();
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if pattern == "<local>" {
        return !host.contains('.');
    }
    if let Some((network, bits)) = pattern.split_once('/') {
        if let (Ok(network), Ok(address), Ok(bits)) = (
            network.parse::<IpAddr>(),
            host.parse::<IpAddr>(),
            bits.parse::<u32>(),
        ) {
            return match (network, address) {
                (IpAddr::V4(network), IpAddr::V4(address)) if bits <= 32 => {
                    bits == 0
                        || u32::from(network) >> (32 - bits) == u32::from(address) >> (32 - bits)
                }
                (IpAddr::V6(network), IpAddr::V6(address)) if bits <= 128 => {
                    bits == 0
                        || u128::from(network) >> (128 - bits)
                            == u128::from(address) >> (128 - bits)
                }
                _ => false,
            };
        }
    }
    let (pattern, required_port) = match pattern.rsplit_once(':') {
        Some((name, number)) if !name.contains(':') => (name, number.parse::<u16>().ok()),
        _ => (pattern.as_str(), None),
    };
    if required_port.is_some() && required_port != port {
        return false;
    }
    let domain = pattern.trim_start_matches('.');
    if !pattern.contains('*') {
        return host == domain || host.ends_with(&format!(".{domain}"));
    }
    // Match '*' throughout the hostname, including Windows IP wildcards.
    let (pattern, host) = (pattern.as_bytes(), host.as_bytes());
    let (mut p, mut h, mut star, mut resume) = (0, 0, None, 0);
    while h < host.len() {
        if p < pattern.len() && pattern[p] == host[h] {
            p += 1;
            h += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            resume = h;
        } else if let Some(index) = star {
            p = index + 1;
            resume += 1;
            h = resume;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}
const PAC_ERROR: &str = "暂不支持 PAC 自动代理脚本，请配置手动系统代理或 HTTPS_PROXY";

#[cfg(target_os = "windows")]
fn system_settings() -> Result<Option<Settings>> {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, KEY_READ},
    };
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
        KEY_READ,
    ) else {
        return Ok(None);
    };
    if key.get_value::<u32, _>("ProxyEnable").unwrap_or(0) != 1 {
        if key
            .get_value::<String, _>("AutoConfigURL")
            .is_ok_and(|value| !value.trim().is_empty())
        {
            bail!(PAC_ERROR);
        }
        return Ok(None);
    }
    let Ok(server) = key.get_value::<String, _>("ProxyServer") else {
        return Ok(None);
    };
    let mut settings = parse_windows_servers(&server);
    settings.bypass = split_bypass(
        &key.get_value::<String, _>("ProxyOverride")
            .unwrap_or_default(),
    );
    settings.exclude_simple = settings.bypass.iter().any(|value| value == "<local>");
    Ok(Some(settings))
}
#[cfg(target_os = "macos")]
fn system_settings() -> Result<Option<Settings>> {
    let output = std::process::Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    parse_macos(&String::from_utf8_lossy(&output.stdout))
}
#[cfg(any(target_os = "macos", test))]
fn parse_macos(text: &str) -> Result<Option<Settings>> {
    let mut values = HashMap::new();
    let mut settings = Settings::default();
    let (mut depth, mut exceptions) = (0usize, false);
    for line in text.lines().map(str::trim) {
        if line == "}" {
            depth = depth.saturating_sub(1);
            if depth == 1 {
                exceptions = false;
            }
            continue;
        }
        if depth == 1 {
            if let Some((key, value)) = line.split_once(" : ") {
                if key == "ExceptionsList" {
                    exceptions = true;
                } else {
                    values.insert(key, value);
                }
            }
        } else if depth == 2 && exceptions {
            if let Some((_, value)) = line.split_once(" : ") {
                settings.bypass.push(value.into());
            }
        }
        if line.ends_with('{') {
            depth += 1;
        }
    }
    for (scheme, name, protocol) in [
        ("http", "HTTP", "http"),
        ("https", "HTTPS", "http"),
        ("*", "SOCKS", "socks5"),
    ] {
        if values.get(format!("{name}Enable").as_str()) == Some(&"1") {
            if let (Some(host), Some(port)) = (
                values.get(format!("{name}Proxy").as_str()),
                values.get(format!("{name}Port").as_str()),
            ) {
                settings
                    .proxies
                    .insert(scheme.into(), format!("{protocol}://{host}:{port}"));
            }
        }
    }
    settings.exclude_simple = values.get("ExcludeSimpleHostnames") == Some(&"1");
    if settings.proxies.is_empty() && values.get("ProxyAutoConfigEnable") == Some(&"1") {
        bail!(PAC_ERROR);
    }
    Ok(Some(settings))
}
#[cfg(any(target_os = "windows", test))]
fn parse_windows_servers(server: &str) -> Settings {
    let mut settings = Settings::default();
    for entry in server.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (scheme, value) = entry.split_once('=').unwrap_or(("*", entry));
        let scheme = scheme.to_ascii_lowercase();
        if scheme == "socks" {
            let value = if value.contains("://") {
                value.into()
            } else {
                format!("socks5://{value}")
            };
            settings.proxies.insert("*".into(), value);
        } else {
            settings.proxies.insert(scheme, value.into());
        }
    }
    settings
}
#[cfg(target_os = "linux")]
fn system_settings() -> Result<Option<Settings>> {
    let kde = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("kde");
    if kde {
        if let Some(settings) = kde_settings()? {
            return Ok(Some(settings));
        }
    }
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["list-recursively", "org.gnome.system.proxy"])
        .output()
    {
        if output.status.success() {
            if let Some(settings) = parse_gnome(&String::from_utf8_lossy(&output.stdout))? {
                return Ok(Some(settings));
            }
        }
    }
    if !kde {
        return kde_settings();
    }
    Ok(None)
}
#[cfg(target_os = "linux")]
fn kde_settings() -> Result<Option<Settings>> {
    let mut paths = Vec::new();
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        paths.push(std::path::PathBuf::from(config));
    } else if let Some(home) = std::env::var_os("HOME") {
        paths.push(std::path::PathBuf::from(home).join(".config"));
    }
    paths.extend(std::env::split_paths(
        &std::env::var_os("XDG_CONFIG_DIRS").unwrap_or_else(|| "/etc/xdg".into()),
    ));
    for path in paths {
        if let Ok(text) = std::fs::read_to_string(path.join("kioslaverc")) {
            if let Some(settings) = parse_kde(&text)? {
                return Ok(Some(settings));
            }
        }
    }
    Ok(None)
}
#[cfg(any(target_os = "linux", test))]
fn unquote(value: &str) -> String {
    let value = value.trim();
    let quoted = value
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .or_else(|| value.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
        .unwrap_or(value);
    quoted
        .replace("\\'", "'")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}
#[cfg(any(target_os = "linux", test))]
fn parse_gnome(text: &str) -> Result<Option<Settings>> {
    let mut values = HashMap::new();
    for line in text.lines() {
        let mut fields = line.splitn(3, ' ');
        if let (Some(schema), Some(key), Some(value)) =
            (fields.next(), fields.next(), fields.next())
        {
            values.insert(format!("{schema}.{key}"), value.trim());
        }
    }
    let get = |key: &str| {
        values
            .get(&format!("org.gnome.system.proxy.{key}"))
            .copied()
            .unwrap_or("")
    };
    match unquote(get("mode")).as_str() {
        "auto" => bail!(PAC_ERROR),
        "manual" => {}
        "none" => return Ok(Some(Settings::default())),
        _ => return Ok(None),
    }
    let mut settings = Settings::default();
    for (scheme, service) in [("http", "http"), ("https", "https"), ("*", "socks")] {
        let host = unquote(get(&format!("{service}.host")));
        let port = get(&format!("{service}.port")).parse::<u16>().unwrap_or(0);
        if !host.is_empty() && port > 0 {
            let protocol = if service == "socks" { "socks5" } else { "http" };
            settings
                .proxies
                .insert(scheme.into(), format!("{protocol}://{host}:{port}"));
        }
    }
    if get("use-same-proxy") == "true" {
        if let Some(http) = settings.proxies.get("http").cloned() {
            settings.proxies.insert("https".into(), http);
        }
    }
    settings.bypass = get("ignore-hosts")
        .trim_matches(['[', ']'])
        .split(',')
        .map(unquote)
        .filter(|s| !s.is_empty())
        .collect();
    Ok(Some(settings))
}
#[cfg(any(target_os = "linux", test))]
fn parse_kde(text: &str) -> Result<Option<Settings>> {
    let mut values = HashMap::new();
    let mut proxy_group = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            proxy_group = line == "[Proxy Settings]";
        } else if proxy_group {
            if let Some((key, value)) = line.split_once('=') {
                values.insert(key.trim(), value.trim());
            }
        }
    }
    match values.get("ProxyType").copied() {
        Some("1") => {}
        Some("2" | "3") => bail!(PAC_ERROR),
        Some("0") => return Ok(Some(Settings::default())),
        _ => return Ok(None),
    }
    let mut settings = Settings::default();
    for (scheme, key) in [
        ("http", "httpProxy"),
        ("https", "httpsProxy"),
        ("*", "socksProxy"),
    ] {
        if let Some(value) = values.get(key).filter(|value| !value.is_empty()) {
            // KDE also stores endpoints as "http://host port".
            let value = value
                .rsplit_once(' ')
                .map(|(host, port)| format!("{host}:{port}"))
                .unwrap_or_else(|| (*value).into());
            let value = if key == "socksProxy" {
                value.replace("http://", "socks5://")
            } else {
                value
            };
            settings.proxies.insert(scheme.into(), value);
        }
    }
    settings.bypass = split_bypass(values.get("NoProxyFor").copied().unwrap_or(""));
    settings.reverse_bypass = values
        .get("ReversedException")
        .is_some_and(|value| *value == "true");
    Ok(Some(settings))
}
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn system_settings() -> Result<Option<Settings>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn environment_preserves_credentials_and_prefers_scheme_over_all() {
        let vars = HashMap::from([
            ("HTTPS_PROXY", "http://User:PaSS@localhost:8080"),
            ("ALL_PROXY", "socks5h://localhost:1080"),
        ]);
        let settings = env_settings(|key| vars.get(key).map(|s| s.to_string())).unwrap();
        assert_eq!(
            settings.select(&Url::parse("https://example.org").unwrap()),
            Some("http://User:PaSS@localhost:8080")
        );
        parse_proxy(
            settings
                .select(&Url::parse("https://example.org").unwrap())
                .unwrap(),
        )
        .unwrap();
        parse_proxy(
            settings
                .select(&Url::parse("http://example.org").unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(env_settings(|_| None).is_none());
        let direct = env_settings(|key| (key == "https_proxy").then(String::new)).unwrap();
        assert_eq!(
            direct.select(&Url::parse("https://example.org").unwrap()),
            Some("")
        );
    }
    #[test]
    fn bypass_matches_domains_ports_wildcards_and_subnets() {
        assert!(bypass_matches("*foo", "foofoo", Some(443)));
        for (rule, host, port) in [
            ("*", "example.org", 443),
            (".example.org", "SUB.EXAMPLE.ORG", 443),
            ("*.example.org:443", "sub.example.org", 443),
            ("192.168.*", "192.168.5.67", 443),
            ("127.0.0.0/8", "127.1.2.3", 443),
            ("::1", "[::1]", 443),
            ("<local>", "frame", 443),
        ] {
            assert!(bypass_matches(rule, host, Some(port)), "{rule} / {host}");
        }
        assert!(!bypass_matches("example.org", "badexample.org", Some(443)));
        assert!(!bypass_matches("example.org:80", "example.org", Some(443)));
        assert!(!bypass_matches("192.168.0.0/16", "10.1.2.3", Some(443)));
        assert!(parse_proxy("file:///tmp/proxy").is_err());
    }
    #[test]
    fn desktop_settings_select_https_or_socks_and_honor_disabled_mode() {
        let gnome = "org.gnome.system.proxy mode 'manual'\norg.gnome.system.proxy.https host '127.0.0.1'\norg.gnome.system.proxy.https port 7890\norg.gnome.system.proxy.socks host '127.0.0.1'\norg.gnome.system.proxy.socks port 7891\norg.gnome.system.proxy ignore-hosts ['localhost', '*.example.org']\n";
        let settings = parse_gnome(gnome).unwrap().unwrap();
        assert_eq!(
            settings.select(&Url::parse("https://github.com").unwrap()),
            Some("http://127.0.0.1:7890")
        );
        assert!(
            settings
                .select(&Url::parse("https://sub.example.org").unwrap())
                .is_none()
        );
        assert_eq!(
            settings.select(&Url::parse("http://github.com").unwrap()),
            Some("socks5://127.0.0.1:7891")
        );
        assert!(
            parse_gnome(&gnome.replace("'manual'", "'none'"))
                .unwrap()
                .unwrap()
                .proxies
                .is_empty()
        );
        assert!(parse_gnome(&gnome.replace("'manual'", "'auto'")).is_err());
        let kde = "[Proxy Settings]\nProxyType=1\nhttpsProxy=http://127.0.0.1 7890\nNoProxyFor=localhost,*.example.org\n";
        let settings = parse_kde(kde).unwrap().unwrap();
        assert_eq!(
            settings.select(&Url::parse("https://github.com").unwrap()),
            Some("http://127.0.0.1:7890")
        );
        assert!(
            settings
                .select(&Url::parse("https://sub.example.org").unwrap())
                .is_none()
        );
        assert!(
            parse_kde(&kde.replace("ProxyType=1", "ProxyType=0"))
                .unwrap()
                .unwrap()
                .proxies
                .is_empty()
        );
    }
    #[test]
    fn macos_and_windows_settings_preserve_protocol_and_bypass_rules() {
        let text = "<dictionary> {\n HTTPSEnable : 1\n HTTPSProxy : proxy.example.org\n HTTPSPort : 8080\n SOCKSEnable : 1\n SOCKSProxy : localhost\n SOCKSPort : 1080\n ExceptionsList : <array> {\n 0 : *.local\n }\n __SCOPED__ : <dictionary> {\n en0 : <dictionary> {\n HTTPSEnable : 0\n HTTPSProxy : wrong.example.org\n }\n }\n}";
        let settings = parse_macos(text).unwrap().unwrap();
        assert_eq!(
            settings.select(&Url::parse("https://github.com").unwrap()),
            Some("http://proxy.example.org:8080")
        );
        assert_eq!(
            settings.select(&Url::parse("http://github.com").unwrap()),
            Some("socks5://localhost:1080")
        );
        assert!(
            settings
                .select(&Url::parse("https://frame.local").unwrap())
                .is_none()
        );
        assert!(parse_macos("<dictionary> {\n ProxyAutoConfigEnable : 1\n}").is_err());
        let settings =
            parse_windows_servers("http=proxy1:8080;https=proxy2:8081;socks=localhost:1080");
        assert_eq!(
            settings.select(&Url::parse("https://github.com").unwrap()),
            Some("proxy2:8081")
        );
        assert_eq!(
            settings.select(&Url::parse("ftp://example.org").unwrap()),
            Some("socks5://localhost:1080")
        );
        let settings = parse_windows_servers("proxy.example.org:8080");
        assert_eq!(
            settings.select(&Url::parse("https://github.com").unwrap()),
            Some("proxy.example.org:8080")
        );
    }
}

#[cfg(test)]
mod download_settings_tests {
    use super::*;
    #[test]
    fn default_preserves_system_discovery_and_old_preferences() {
        assert!(DownloadSettings::default().system);
        assert!(
            serde_json::from_str::<DownloadSettings>("{}")
                .unwrap()
                .system
        );
    }
    #[test]
    fn disabled_system_and_explicit_override() {
        let mut settings = DownloadSettings {
            system: false,
            ..Default::default()
        };
        let target = Url::parse("https://github.com/a/b").unwrap();
        assert!(settings.proxy_for(&target).unwrap().is_none());
        settings.http = "http://127.0.0.1:7890".into();
        settings.validate().unwrap();
        assert!(settings.proxy_for(&target).unwrap().is_some());
    }
    #[test]
    fn github_routes_only_exact_hosts_and_does_not_double_wrap_redirects() {
        let settings = DownloadSettings {
            github: "https://proxy.example/prefix/".into(),
            ..Default::default()
        };
        settings.validate().unwrap();
        for host in [
            "github.com",
            "api.github.com",
            "raw.githubusercontent.com",
            "objects.githubusercontent.com",
            "release-assets.githubusercontent.com",
        ] {
            let original = Url::parse(&format!("https://{host}/a?x=1")).unwrap();
            let routed = settings.route(&original);
            assert_eq!(
                routed.as_str(),
                format!("https://proxy.example/prefix/{original}")
            );
            assert_eq!(settings.route(&routed), routed);
        }
        for host in ["github.com.evil.example", "example.org", "proxy.example"] {
            let url = Url::parse(&format!("https://{host}/a")).unwrap();
            assert_eq!(settings.route(&url), url);
        }
    }
    #[test]
    fn invalid_preferences_are_rejected_before_saving() {
        for github in [
            "http://proxy.example",
            "https://proxy.example?q=1",
            "https://u:p@proxy.example",
            "https://proxy.example/#x",
        ] {
            assert!(
                DownloadSettings {
                    github: github.into(),
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
        for http in ["file:///tmp/x", "http://[::1]:1234"] {
            assert!(
                DownloadSettings {
                    http: http.into(),
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
    }
}
