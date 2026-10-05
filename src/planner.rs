use crate::{
    model::*,
    relations::{self, Dependency},
};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub manifest: Manifest,
    pub source: Option<String>,
    pub action: String,
    pub run_as_changed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub fingerprint: String,
    pub root: String,
    pub items: Vec<Item>,
    pub add_sources: Vec<Source>,
    pub enable: Vec<String>,
    pub disable: Vec<String>,
    pub affected: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ChoiceRequired {
    pub dependency: String,
    pub candidates: Vec<Source>,
}
impl std::fmt::Display for ChoiceRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "依赖 {} 需要选择来源", self.dependency)
    }
}
impl std::error::Error for ChoiceRequired {}
pub struct Prepared {
    pub plan: Plan,
    pub packages: Vec<(std::sync::Arc<crate::package::Staged>, Option<String>)>,
}
struct Selection {
    manifest: Manifest,
    source: Option<String>,
    bytes: Option<std::sync::Arc<crate::package::Staged>>,
}
struct Resolver<'a> {
    db: &'a Database,
    sources: Vec<Source>,
    catalogs: BTreeMap<String, Catalog>,
    histories: std::collections::BTreeSet<(String, String)>,
    selected: BTreeMap<String, Selection>,
    choices: BTreeMap<String, String>,
    cancel: crate::jobs::Cancellation,
}
impl Resolver<'_> {
    fn catalog(&mut self, source: &Source) -> Result<Catalog> {
        if let Some(c) = self.catalogs.get(&source.id) {
            return Ok(c.clone());
        }
        self.cancel.check()?;
        let c = crate::session::fetch_catalog_proxy_cancel(source, &self.db.proxy, &self.cancel)?;
        self.catalogs.insert(source.id.clone(), c.clone());
        Ok(c)
    }
    fn matching_catalog(&mut self, source: &Source, id: &str, dep: &Dependency) -> Result<Catalog> {
        let mut catalog = self.catalog(source)?;
        if catalog.plugins.iter().any(|p| p.id == id)
            && !catalog.plugins.iter().any(|p| {
                p.id == id
                    && p.engines
                        .as_ref()
                        .is_none_or(|e| e.matches(env!("CARGO_PKG_VERSION")))
                    && dep.matches(&p.version).unwrap_or(false)
            })
            && !self.histories.contains(&(source.id.clone(), id.to_owned()))
        {
            let history = crate::session::fetch_plugin_versions_cancel(
                source,
                id,
                &self.db.proxy,
                &self.cancel,
            )?;
            for entry in history {
                if !catalog
                    .plugins
                    .iter()
                    .any(|p| p.id == entry.id && p.version == entry.version)
                {
                    catalog.plugins.push(entry);
                }
            }
            self.catalogs.insert(source.id.clone(), catalog.clone());
            self.histories.insert((source.id.clone(), id.to_owned()));
        }
        Ok(catalog)
    }
    fn explicit_source(&mut self, url: &str) -> Result<Source> {
        let url = relations::canonical(url)?;
        if let Some(source) = self
            .sources
            .iter()
            .find(|s| relations::canonical(&s.url).ok().as_deref() == Some(url.as_str()))
        {
            return Ok(source.clone());
        }
        let source = Source {
            id: format!("source-{}", &crate::package::digest(url.as_bytes())[..20]),
            name: url.clone(),
            url,
            enabled: true,
            allow_http: false,
        };
        source.validate()?;
        ensure!(self.sources.len() < 200, "插件源数量超过 200");
        self.sources.push(source.clone());
        Ok(source)
    }
    fn choose(&mut self, id: &str, dep: &Dependency, origin: Option<&str>) -> Result<Selection> {
        if let Some(p) = self.db.plugins.get(id) {
            if dep.matches(&p.manifest.version)? && relations::source_matches(self.db, dep, id)? {
                return Ok(Selection {
                    manifest: p.manifest.clone(),
                    source: p.source.clone(),
                    bytes: None,
                });
            }
        }
        let selected = self.choices.get(id).cloned();
        let source = if let Some(url) = dep.source() {
            let source = self.explicit_source(url)?;
            if let Some(choice) = selected {
                ensure!(
                    relations::canonical(&choice)? == relations::canonical(url)?,
                    "不能覆盖显式依赖来源：{id}"
                );
            }
            source
        } else {
            let mut candidates = vec![];
            let mut errors = vec![];
            let mut sources = self.sources.clone();
            sources.sort_by_key(|s| Some(s.id.as_str()) != origin);
            for source in sources.into_iter().filter(|s| s.enabled) {
                match self.matching_catalog(&source, id, dep) {
                    Ok(c) => {
                        if c.plugins.iter().any(|p| {
                            p.id == id
                                && p.engines
                                    .as_ref()
                                    .is_none_or(|e| e.matches(env!("CARGO_PKG_VERSION")))
                                && dep.matches(&p.version).unwrap_or(false)
                        }) {
                            let preferred =
                                selected.is_none() && Some(source.id.as_str()) == origin;
                            candidates.push(source);
                            if preferred {
                                break;
                            }
                        }
                    }
                    Err(e) => errors.push(format!("{}：{e}", source.name)),
                }
            }
            if let Some(choice) = selected {
                candidates
                    .into_iter()
                    .find(|s| {
                        relations::canonical(&s.url).ok() == relations::canonical(&choice).ok()
                    })
                    .context("所选来源没有满足版本的依赖")?
            } else if let Some(source) = candidates.iter().find(|s| Some(s.id.as_str()) == origin) {
                source.clone()
            } else if !candidates.is_empty() {
                return Err(ChoiceRequired {
                    dependency: id.to_owned(),
                    candidates,
                }
                .into());
            } else {
                bail!("找不到依赖 {id} {}。{}", dep.version(), errors.join("；"))
            }
        };
        ensure!(
            source.enabled,
            "指定依赖源已停用，请先启用：{}",
            source.name
        );
        let catalog = self.matching_catalog(&source, id, dep)?;
        let entry = catalog
            .plugins
            .iter()
            .find(|p| {
                p.id == id
                    && p.engines
                        .as_ref()
                        .is_none_or(|e| e.matches(env!("CARGO_PKG_VERSION")))
                    && dep.matches(&p.version).unwrap_or(false)
            })
            .context("指定源中不存在依赖")?;
        ensure!(
            dep.matches(&entry.version)?,
            "指定源没有满足版本范围的依赖：{id} {}",
            dep.version()
        );
        let bytes = crate::package::stage_request_proxy(
            &serde_json::json!({"url":entry.url,"sha256":entry.sha256,"allowHttp":source.allow_http}),
            &self.db.proxy,
            |_, _| self.cancel.check(),
        )?;
        let manifest = bytes.manifest()?;
        if let Some(e) = &manifest.engines {
            e.check()?;
        }
        ensure!(
            manifest.id == entry.id && manifest.version == entry.version,
            "依赖包 ID 或版本与目录不一致"
        );
        ensure!(
            serde_json::to_value(manifest.relations())? == serde_json::to_value(entry.relations())?,
            "依赖包关系声明与目录不一致"
        );
        Ok(Selection {
            manifest,
            source: Some(source.id),
            bytes: Some(bytes),
        })
    }
    fn visit(
        &mut self,
        id: &str,
        path: &mut Vec<String>,
        done: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) -> Result<()> {
        self.cancel.check()?;
        if done.contains(id) {
            return Ok(());
        }
        ensure!(self.selected.len() <= 32, "依赖计划超过 32 个插件");
        if let Some(at) = path.iter().position(|p| p == id) {
            bail!("循环依赖：{} → {id}", path[at..].join(" → "))
        }
        path.push(id.to_owned());
        let m = self.selected[id].manifest.clone();
        let origin = self.selected[id].source.clone();
        for (target, dep) in &m.dependencies {
            if !self.selected.contains_key(target) {
                let selection = self.choose(target, dep, origin.as_deref())?;
                self.selected.insert(target.clone(), selection);
            }
            let target_plugin = &self.selected[target];
            ensure!(
                dep.matches(&target_plugin.manifest.version)?,
                "依赖约束无法同时满足：{target} {}",
                dep.version()
            );
            if let Some(expected) = dep.source() {
                let actual = target_plugin
                    .source
                    .as_ref()
                    .and_then(|id| self.sources.iter().find(|s| &s.id == id))
                    .map(|s| s.url.as_str())
                    .or_else(|| {
                        target_plugin
                            .source
                            .as_ref()
                            .and_then(|id| self.db.source_urls.get(id))
                            .map(String::as_str)
                    });
                ensure!(
                    actual.map(relations::canonical).transpose()?.as_deref()
                        == Some(relations::canonical(expected)?.as_str()),
                    "依赖来源约束无法同时满足：{target}"
                );
            }
            self.visit(target, path, done, order)?;
        }
        path.pop();
        done.insert(id.to_owned());
        order.push(id.to_owned());
        Ok(())
    }
}
pub fn prepare(
    db: &Database,
    bytes: Vec<u8>,
    source: Option<String>,
    choices: BTreeMap<String, String>,
) -> Result<Prepared> {
    prepare_cancel(db, bytes, source, choices, Default::default())
}
pub fn prepare_cancel(
    db: &Database,
    bytes: Vec<u8>,
    source: Option<String>,
    choices: BTreeMap<String, String>,
    cancel: crate::jobs::Cancellation,
) -> Result<Prepared> {
    prepare_staged(
        db,
        crate::package::Staged::from_bytes(&bytes)?,
        source,
        choices,
        cancel,
    )
}
pub fn prepare_staged(
    db: &Database,
    bytes: std::sync::Arc<crate::package::Staged>,
    source: Option<String>,
    choices: BTreeMap<String, String>,
    cancel: crate::jobs::Cancellation,
) -> Result<Prepared> {
    cancel.check()?;
    let manifest = bytes.manifest()?;
    if let Some(e) = &manifest.engines {
        e.check()?;
    }
    let root = manifest.id.clone();
    if let Some(source) = &source {
        ensure!(db.sources.iter().any(|s| &s.id == source), "未知安装来源");
    }
    let mut resolver = Resolver {
        db,
        sources: db.sources.clone(),
        catalogs: BTreeMap::new(),
        histories: Default::default(),
        selected: BTreeMap::new(),
        choices,
        cancel,
    };
    resolver.selected.insert(
        root.clone(),
        Selection {
            manifest,
            source,
            bytes: Some(bytes),
        },
    );
    let mut ordered = vec![];
    resolver.visit(&root, &mut vec![], &mut BTreeSet::new(), &mut ordered)?;
    let mut proposed = db.clone();
    proposed.sources = resolver.sources.clone();
    let mut items = vec![];
    let mut packages = vec![];
    let mut affected = BTreeSet::new();
    for id in &ordered {
        let selection = resolver.selected.remove(id).unwrap();
        let old = db.plugins.get(id);
        let changed = old.is_some_and(|p| p.manifest.run_as() != selection.manifest.run_as());
        let action = if selection.bytes.is_none() {
            "reuse"
        } else if old.is_some_and(|p| p.manifest.version == selection.manifest.version) {
            "reinstall"
        } else if old.is_some() {
            "update"
        } else {
            "install"
        };
        let item = Item {
            manifest: selection.manifest.clone(),
            source: selection.source.clone(),
            action: action.into(),
            run_as_changed: changed,
        };
        items.push(item);
        if let Some(bytes) = selection.bytes {
            packages.push((bytes, selection.source.clone()));
            affected.insert(id.clone());
            affected.extend(relations::dependents(db, id));
        }
        let mut installed = old.cloned().unwrap_or(Installed {
            manifest: selection.manifest.clone(),
            enabled: true,
            favorite: false,
            order: proposed.plugins.len() as u32,
            source: None,
            error: None,
        });
        installed.manifest = selection.manifest;
        installed.source = selection.source;
        proposed.plugins.insert(id.clone(), installed);
    }
    // Check reverse dependencies, including disabled installed dependents.
    relations::order(&proposed, std::slice::from_ref(&root), false)?;
    relations::validate_updates(
        &proposed,
        &packages
            .iter()
            .map(|(bytes, _)| bytes.manifest().map(|v| v.id))
            .collect::<Result<Vec<_>>>()?,
    )?;
    let activation = if proposed.plugins[&root].enabled {
        Some(relations::activation(&proposed, &root)?)
    } else {
        None
    };
    let (enable, disable) = activation
        .map(|p| (p.enable, p.disable))
        .unwrap_or_default();
    for id in &disable {
        proposed.plugins.get_mut(id).unwrap().enabled = false;
        affected.insert(id.clone());
    }
    for id in &enable {
        proposed.plugins.get_mut(id).unwrap().enabled = true;
    }
    relations::check_enabled(&proposed)?;
    let add_sources = resolver
        .sources
        .into_iter()
        .filter(|s| !db.sources.iter().any(|old| old.id == s.id))
        .collect();
    Ok(Prepared {
        plan: Plan {
            fingerprint: relations::fingerprint(db)?,
            root,
            items,
            add_sources,
            enable,
            disable,
            affected: affected.into_iter().collect(),
        },
        packages,
    })
}
