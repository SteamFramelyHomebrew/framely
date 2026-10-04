use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub auto_refresh: bool,
    pub last_success: Option<u64>,
    pub next_refresh: u64,
    pub error: Option<String>,
    pub entries: Vec<SubscriptionSource>,
    pub pending: Vec<SubscriptionSource>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub failures: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionSource {
    pub id: String,
    pub name: String,
    pub url: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Origins {
    pub manual: bool,
    pub subscriptions: BTreeMap<String, String>,
}
use crate::{
    model::{valid_id, validate_url, Database, Source},
    relations::canonical,
};
use anyhow::{ensure, Context, Result};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    pub name: String,
    pub sources: Vec<SubscriptionSource>,
}
impl Document {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && self.sources.len() <= 100,
            "无效订阅版本或源数量超过 100"
        );
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 120,
            "无效订阅名称"
        );
        let mut ids = std::collections::BTreeSet::new();
        let mut urls = std::collections::BTreeSet::new();
        for source in &self.sources {
            valid_id(&source.id)?;
            ensure!(
                !source.name.trim().is_empty() && source.name.len() <= 120,
                "无效源名称"
            );
            validate_url(&source.url, false)?;
            ensure!(
                ids.insert(&source.id) && urls.insert(canonical(&source.url)?),
                "订阅中存在重复源"
            );
        }
        Ok(())
    }
}
const DEFAULT_SOURCES: [(&str, &str, &str); 2] = [
    (
        "framely-community-stable",
        "Framely 社区 · 稳定",
        "https://raw.githubusercontent.com/SteamFramelyHomebrew/framely-plugin-database/refs/heads/publish/stable/catalog.json",
    ),
    (
        "framely-community-testing",
        "Framely 社区 · 测试",
        "https://raw.githubusercontent.com/SteamFramelyHomebrew/framely-plugin-database/refs/heads/publish/testing/catalog.json",
    ),
];

pub fn initialize_defaults(db: &mut Database) -> Result<bool> {
    if db.default_sources_initialized {
        return Ok(false);
    }
    for (id, name, url) in DEFAULT_SOURCES {
        let url = canonical(url)?;
        if db.sources.len() >= 200
            || db
                .sources
                .iter()
                .any(|source| canonical(&source.url).ok().as_deref() == Some(url.as_str()))
        {
            continue;
        }
        let mut key = id.to_owned();
        let mut suffix = 1;
        while db.sources.iter().any(|source| source.id == key) {
            key = format!("{id}-{suffix}");
            suffix += 1;
        }
        db.sources.push(Source {
            id: key,
            name: name.into(),
            enabled: db.source_enabled.get(&url).copied().unwrap_or(true),
            url,
            allow_http: false,
        });
    }
    db.default_sources_initialized = true;
    Ok(true)
}

pub fn initialize(db: &mut Database) -> Result<()> {
    for source in &db.sources {
        db.source_enabled
            .entry(canonical(&source.url)?)
            .or_insert(source.enabled);
        db.source_urls
            .insert(source.id.clone(), canonical(&source.url)?);
        db.source_origins
            .entry(source.id.clone())
            .or_insert_with(|| Origins {
                manual: true,
                ..Default::default()
            });
    }
    Ok(())
}
fn reconcile(db: &mut Database, id: &str) -> Result<()> {
    let sub = db
        .subscriptions
        .iter()
        .find(|s| s.id == id)
        .context("订阅不存在")?
        .clone();
    for origins in db.source_origins.values_mut() {
        origins.subscriptions.remove(id);
    }
    if sub.enabled {
        for entry in &sub.entries {
            let url = canonical(&entry.url)?;
            let key = if let Some(source) = db
                .sources
                .iter()
                .find(|s| canonical(&s.url).ok().as_deref() == Some(url.as_str()))
            {
                source.id.clone()
            } else {
                ensure!(db.sources.len() < 200, "插件源数量超过 200");
                let key = format!("source-{}", &crate::package::digest(url.as_bytes())[..20]);
                ensure!(!db.sources.iter().any(|s| s.id == key), "插件源 ID 冲突");
                db.sources.push(Source {
                    id: key.clone(),
                    name: entry.name.clone(),
                    url: url.clone(),
                    enabled: db.source_enabled.get(&url).copied().unwrap_or(true),
                    allow_http: false,
                });
                key
            };
            db.source_urls.insert(key.clone(), url);
            db.source_origins
                .entry(key)
                .or_default()
                .subscriptions
                .insert(id.to_owned(), entry.id.clone());
        }
    }
    for source in &mut db.sources {
        if let Some(origin) = db.source_origins.get(&source.id).filter(|o| !o.manual) {
            if let Some((subscription, entry)) = origin.subscriptions.iter().next() {
                if let Some(name) = db
                    .subscriptions
                    .iter()
                    .find(|s| &s.id == subscription)
                    .and_then(|s| s.entries.iter().find(|e| &e.id == entry))
                    .map(|e| e.name.clone())
                {
                    source.name = name;
                }
            }
        }
    }
    db.sources.retain(|s| {
        db.source_origins
            .get(&s.id)
            .is_some_and(|o| o.manual || !o.subscriptions.is_empty())
    });
    Ok(())
}
pub fn add(db: &mut Database, url: &str, doc: Document) -> Result<String> {
    validate_url(url, false)?;
    let url = canonical(url)?;
    doc.validate()?;
    ensure!(db.subscriptions.len() < 20, "最多添加 20 个订阅");
    ensure!(
        !db.subscriptions.iter().any(|s| s.url == url),
        "该订阅已添加"
    );
    let id = format!(
        "subscription-{}",
        &crate::package::digest(url.as_bytes())[..20]
    );
    db.subscriptions.push(Subscription {
        id: id.clone(),
        name: doc.name,
        url,
        enabled: true,
        auto_refresh: true,
        last_success: Some(crate::service::now_ms()),
        next_refresh: crate::service::now_ms() + 86_400_000,
        error: None,
        entries: doc.sources,
        pending: vec![],
        etag: None,
        last_modified: None,
        failures: 0,
    });
    reconcile(db, &id)?;
    Ok(id)
}
pub fn apply(
    db: &mut Database,
    id: &str,
    doc: Option<Document>,
    etag: Option<String>,
    modified: Option<String>,
    error: Option<String>,
) -> Result<()> {
    if let Some(doc) = &doc {
        doc.validate()?;
    }
    let sub = db
        .subscriptions
        .iter_mut()
        .find(|s| s.id == id)
        .context("订阅不存在")?;
    if let Some(error) = error {
        sub.failures = sub.failures.saturating_add(1);
        sub.next_refresh =
            crate::service::now_ms() + (60_000u64 << (sub.failures.min(10) - 1)).min(86_400_000);
        sub.error = Some(error.chars().take(2048).collect());
        return Ok(());
    }
    if let Some(doc) = doc {
        let mut entries = vec![];
        let mut pending = vec![];
        for next in doc.sources {
            if let Some(old) = sub.entries.iter().find(|s| s.id == next.id) {
                if canonical(&old.url)? != canonical(&next.url)? {
                    entries.push(old.clone());
                    pending.push(next);
                    continue;
                }
            }
            entries.push(next);
        }
        sub.name = doc.name;
        sub.entries = entries;
        sub.pending = pending;
        sub.etag = etag;
        sub.last_modified = modified;
    }
    sub.last_success = Some(crate::service::now_ms());
    sub.next_refresh = crate::service::now_ms() + 86_400_000;
    sub.failures = 0;
    sub.error = None;
    reconcile(db, id)
}
pub fn change(
    db: &mut Database,
    id: &str,
    enabled: Option<bool>,
    auto: Option<bool>,
    accept: bool,
    remove: bool,
) -> Result<()> {
    let sub = db
        .subscriptions
        .iter_mut()
        .find(|s| s.id == id)
        .context("订阅不存在")?;
    if accept {
        for pending in std::mem::take(&mut sub.pending) {
            sub.entries.retain(|e| e.id != pending.id);
            sub.entries.push(pending);
        }
    }
    if let Some(value) = enabled {
        sub.enabled = value;
    }
    if let Some(value) = auto {
        sub.auto_refresh = value;
    }
    if remove {
        sub.enabled = false;
    }
    reconcile(db, id)?;
    if remove {
        db.subscriptions.retain(|s| s.id != id);
    }
    Ok(())
}
pub fn save_sources(db: &mut Database, sources: Vec<Source>) -> Result<()> {
    ensure!(sources.len() <= 200, "插件源数量超过 200");
    let mut ids = std::collections::BTreeSet::new();
    let mut urls = std::collections::BTreeSet::new();
    for source in &sources {
        source.validate()?;
        ensure!(
            ids.insert(source.id.clone()) && urls.insert(canonical(&source.url)?),
            "重复源 ID 或 URL"
        );
        if db
            .source_origins
            .get(&source.id)
            .is_some_and(|o| !o.subscriptions.is_empty())
        {
            ensure!(
                db.source_urls.get(&source.id) == Some(&canonical(&source.url)?),
                "订阅源 URL 请在订阅变更中确认"
            );
        }
    }
    let mut result = sources.clone();
    for old in &db.sources {
        if !sources.iter().any(|s| s.id == old.id) {
            let origin = db.source_origins.entry(old.id.clone()).or_default();
            origin.manual = false;
            if !origin.subscriptions.is_empty() {
                result.push(old.clone());
            }
        }
    }
    for source in &sources {
        db.source_enabled
            .insert(canonical(&source.url)?, source.enabled);
        db.source_urls
            .insert(source.id.clone(), canonical(&source.url)?);
        db.source_origins
            .entry(source.id.clone())
            .or_insert_with(|| Origins {
                manual: true,
                ..Default::default()
            });
    }
    db.sources = result;
    Ok(())
}
pub fn fetch_cancel(
    url: &str,
    etag: Option<&str>,
    modified: Option<&str>,
    proxy: &crate::model::ProxySettings,
    cancel: &crate::jobs::Cancellation,
) -> Result<(Option<Document>, Option<String>, Option<String>)> {
    cancel.check()?;

    validate_url(url, false)?;
    let mut headers = vec![];
    if let Some(v) = etag {
        ensure!(v.len() <= 2048 && !v.contains(['\r', '\n']), "无效 ETag");
        headers.push(("If-None-Match", v));
    }
    if let Some(v) = modified {
        ensure!(v.len() <= 2048 && !v.contains(['\r', '\n']), "无效更新时间");
        headers.push(("If-Modified-Since", v));
    }
    let response = crate::http::get_with_proxy(
        url,
        false,
        std::time::Duration::from_secs(20),
        &headers,
        proxy,
    )?;
    if response.status() == 304 {
        return Ok((None, None, None));
    }
    let etag = response
        .header("ETag")
        .map(|v| v.chars().take(2048).collect());
    let modified = response
        .header("Last-Modified")
        .map(|v| v.chars().take(2048).collect());
    let bytes = crate::http::read_cancel(response, 512 * 1024, cancel)?;
    ensure!(bytes.len() <= 512 * 1024, "订阅超过 512 KiB");
    let doc: Document = serde_json::from_slice(&bytes).context("订阅 JSON 格式无效")?;
    doc.validate()?;
    Ok((Some(doc), etag, modified))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn doc(url: &str) -> Document {
        Document {
            schema_version: 1,
            name: "test".into(),
            sources: vec![SubscriptionSource {
                id: "stable".into(),
                name: "stable".into(),
                url: url.into(),
            }],
        }
    }
    #[test]
    fn defaults_are_added_once_and_removal_survives_reload() {
        let mut db = Database::default();
        assert!(initialize_defaults(&mut db).unwrap());
        initialize(&mut db).unwrap();
        assert_eq!(db.sources.len(), 2);
        for (source, (_, _, url)) in db.sources.iter().zip(DEFAULT_SOURCES) {
            assert_eq!(source.url, url);
            assert!(source.enabled);
            assert!(!source.allow_http);
            source.validate().unwrap();
        }
        save_sources(&mut db, vec![]).unwrap();
        let mut reloaded: Database =
            serde_json::from_slice(&serde_json::to_vec(&db).unwrap()).unwrap();
        assert!(!initialize_defaults(&mut reloaded).unwrap());
        assert!(reloaded.sources.is_empty());
    }

    #[test]
    fn defaults_preserve_existing_sources_disabled_preferences_and_id_collisions() {
        let mut db = Database::default();
        db.sources.push(Source {
            id: "my-stable".into(),
            name: "My disabled stable".into(),
            url: DEFAULT_SOURCES[0].2.into(),
            enabled: false,
            allow_http: false,
        });
        db.sources.push(Source {
            id: DEFAULT_SOURCES[1].0.into(),
            name: "Custom source".into(),
            url: "https://example.org/catalog.json".into(),
            enabled: true,
            allow_http: false,
        });
        db.source_enabled.insert(DEFAULT_SOURCES[1].2.into(), false);
        initialize_defaults(&mut db).unwrap();
        assert_eq!(db.sources.len(), 3);
        assert_eq!(db.sources[0].name, "My disabled stable");
        assert!(!db.sources[0].enabled);
        assert_eq!(db.sources[1].url, "https://example.org/catalog.json");
        assert_eq!(db.sources[2].id, "framely-community-testing-1");
        assert!(!db.sources[2].enabled);
    }

    #[test]
    fn sharing_pending_urls_cache_and_disabled_preferences() {
        let mut db = Database::default();
        let a = add(
            &mut db,
            "https://example.org/a.json",
            doc("https://example.org/catalog.json"),
        )
        .unwrap();
        let b = add(
            &mut db,
            "https://example.org/b.json",
            doc("https://example.org/catalog.json"),
        )
        .unwrap();
        assert_eq!(db.sources.len(), 1);
        let mut sources = db.sources.clone();
        sources[0].enabled = false;
        save_sources(&mut db, sources).unwrap();
        apply(
            &mut db,
            &a,
            Some(doc("https://other.org/catalog.json")),
            Some("v2".into()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(db.subscriptions[0].pending.len(), 1);
        assert_eq!(db.sources[0].url, "https://example.org/catalog.json");
        assert!(!db.sources[0].enabled);
        apply(&mut db, &a, None, None, None, Some("offline".into())).unwrap();
        assert_eq!(db.subscriptions[0].entries.len(), 1);
        assert!(db.subscriptions[0].error.is_some());
        change(&mut db, &a, None, None, true, false).unwrap();
        assert_eq!(db.sources.len(), 2);
        change(&mut db, &a, None, None, false, true).unwrap();
        assert_eq!(db.sources.len(), 1);
        change(&mut db, &b, None, None, false, true).unwrap();
        assert!(db.sources.is_empty());
        add(
            &mut db,
            "https://example.org/c.json",
            doc("https://example.org/catalog.json"),
        )
        .unwrap();
        assert!(!db.sources[0].enabled);
    }
    #[test]
    fn manual_source_survives_subscription_removal() {
        let mut db = Database::default();
        save_sources(
            &mut db,
            vec![Source {
                id: "manual".into(),
                name: "manual".into(),
                url: "https://example.org/catalog.json".into(),
                enabled: true,
                allow_http: false,
            }],
        )
        .unwrap();
        let id = add(
            &mut db,
            "https://example.org/sub.json",
            doc("https://example.org/catalog.json"),
        )
        .unwrap();
        change(&mut db, &id, None, None, false, true).unwrap();
        assert_eq!(db.sources.len(), 1);
        assert!(db.source_origins["manual"].manual);
    }
    #[test]
    fn strict_document_rejects_nesting_http_and_duplicate() {
        assert!(doc("http://example.org/catalog.json").validate().is_err());
        let mut d = doc("https://example.org/catalog.json");
        d.sources.push(d.sources[0].clone());
        assert!(d.validate().is_err());
        assert!(serde_json::from_value::<Document>(
            serde_json::json!({"schemaVersion":1,"name":"bad","sources":[],"subscriptions":[]})
        )
        .is_err());
    }
}
