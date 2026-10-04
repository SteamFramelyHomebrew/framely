use crate::model::{valid_id, validate_url, Database, Manifest};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Dependency {
    Version(String),
    Source(DependencySource),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DependencySource {
    pub version: String,
    pub source: String,
}
impl Dependency {
    pub fn version(&self) -> &str {
        match self {
            Self::Version(v) => v,
            Self::Source(v) => &v.version,
        }
    }
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Version(_) => None,
            Self::Source(v) => Some(&v.source),
        }
    }
    pub fn matches(&self, version: &str) -> Result<bool> {
        Ok(requirement(self.version())?
            .matches(&semver::Version::parse(version).context("依赖目标必须使用 SemVer 版本")?))
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Relations {
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub optional_dependencies: BTreeMap<String, Dependency>,
    #[serde(default)]
    pub conflicts: BTreeMap<String, String>,
    #[serde(default)]
    pub exclusive_resources: Vec<String>,
}
pub fn canonical(url: &str) -> Result<String> {
    validate_url(url, true)?;
    let value = url::Url::parse(url)?;
    ensure!(
        value.host_str().is_some() && value.username().is_empty() && value.password().is_none(),
        "无效的插件源 URL"
    );
    Ok(value.to_string())
}
pub fn requirement(value: &str) -> Result<semver::VersionReq> {
    ensure!(!value.is_empty() && value.len() <= 120, "无效的版本范围");
    if semver::Version::parse(value).is_ok() {
        return Ok(semver::VersionReq::parse(&format!("={value}"))?);
    }
    if let Ok(req) = semver::VersionReq::parse(value) {
        return Ok(req);
    }
    let parts: Vec<_> = value.split_whitespace().collect();
    semver::VersionReq::parse(&parts.join(", ")).context("无效的 SemVer 版本范围")
}
impl Relations {
    pub fn validate(&self, id: &str, version: &str) -> Result<()> {
        ensure!(
            self.dependencies.len() + self.optional_dependencies.len() <= 64
                && self.conflicts.len() <= 64
                && self.exclusive_resources.len() <= 32,
            "插件关系声明过多"
        );
        if !self.dependencies.is_empty()
            || !self.optional_dependencies.is_empty()
            || !self.conflicts.is_empty()
            || !self.exclusive_resources.is_empty()
        {
            semver::Version::parse(version).context("声明关系的插件必须使用 SemVer")?;
        }
        for (target, dep) in self.dependencies.iter().chain(&self.optional_dependencies) {
            valid_id(target)?;
            ensure!(target != id, "插件不能依赖自身");
            requirement(dep.version())?;
            if let Some(source) = dep.source() {
                validate_url(source, false)?;
                canonical(source)?;
            }
            ensure!(
                !self.conflicts.contains_key(target),
                "依赖与冲突声明矛盾：{target}"
            );
        }
        ensure!(
            self.dependencies
                .keys()
                .all(|id| !self.optional_dependencies.contains_key(id)),
            "必需与可选依赖重复"
        );
        for (target, range) in &self.conflicts {
            valid_id(target)?;
            ensure!(target != id, "插件不能与自身冲突");
            requirement(range)?;
        }
        let mut unique = BTreeSet::new();
        for resource in &self.exclusive_resources {
            valid_id(resource)?;
            ensure!(unique.insert(resource), "重复独占资源");
        }
        Ok(())
    }
}
pub fn source_matches(db: &Database, dep: &Dependency, id: &str) -> Result<bool> {
    let Some(expected) = dep.source() else {
        return Ok(true);
    };
    let installed = db.plugins.get(id).context("缺少依赖")?;
    let actual = installed
        .source
        .as_ref()
        .and_then(|id| db.sources.iter().find(|s| &s.id == id))
        .map(|s| s.url.as_str())
        .or_else(|| {
            installed
                .source
                .as_ref()
                .and_then(|id| db.source_urls.get(id))
                .map(String::as_str)
        });
    Ok(actual.map(canonical).transpose()?.as_deref() == Some(canonical(expected)?.as_str()))
}
pub fn order(db: &Database, roots: &[String], enabled: bool) -> Result<Vec<String>> {
    fn visit(
        db: &Database,
        id: &str,
        enabled: bool,
        path: &mut Vec<String>,
        done: &mut BTreeSet<String>,
        out: &mut Vec<String>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        if let Some(at) = path.iter().position(|p| p == id) {
            bail!("循环依赖：{} → {id}", path[at..].join(" → "))
        }
        let plugin = db
            .plugins
            .get(id)
            .with_context(|| format!("缺少依赖：{id}"))?;
        ensure!(!enabled || plugin.enabled, "依赖已停用：{id}");
        path.push(id.to_owned());
        for (target, dep) in &plugin.manifest.dependencies {
            let target_plugin = db
                .plugins
                .get(target)
                .with_context(|| format!("{} 缺少依赖 {target}", plugin.manifest.name))?;
            ensure!(
                dep.matches(&target_plugin.manifest.version)?,
                "依赖版本不匹配：{target} 需要 {}，已安装 {}",
                dep.version(),
                target_plugin.manifest.version
            );
            ensure!(source_matches(db, dep, target)?, "依赖来源不匹配：{target}");
            visit(db, target, enabled, path, done, out)?;
        }
        path.pop();
        done.insert(id.to_owned());
        out.push(id.to_owned());
        Ok(())
    }
    let mut out = vec![];
    let mut done = BTreeSet::new();
    for id in roots {
        visit(db, id, enabled, &mut vec![], &mut done, &mut out)?;
    }
    Ok(out)
}
pub fn conflict(a: &Manifest, b: &Manifest) -> Result<Option<String>> {
    for (from, to) in [(a, b), (b, a)] {
        if let Some(range) = from.conflicts.get(&to.id) {
            if requirement(range)?.matches(
                &semver::Version::parse(&to.version).context("冲突目标版本必须使用 SemVer")?,
            ) {
                return Ok(Some(format!("{} 与 {} 冲突", a.name, b.name)));
            }
        }
    }
    for resource in &a.exclusive_resources {
        if b.exclusive_resources.contains(resource) {
            return Ok(Some(format!(
                "独占资源 {resource} 被 {} 与 {} 同时使用",
                a.name, b.name
            )));
        }
    }
    Ok(None)
}
pub fn check_enabled(db: &Database) -> Result<()> {
    let ids: Vec<_> = db
        .plugins
        .iter()
        .filter(|(_, p)| p.enabled)
        .map(|(id, _)| id.clone())
        .collect();
    order(db, &ids, false)?;
    for (i, id) in ids.iter().enumerate() {
        for other in &ids[i + 1..] {
            if let Some(reason) = conflict(&db.plugins[id].manifest, &db.plugins[other].manifest)? {
                bail!("{reason}")
            }
        }
    }
    Ok(())
}
pub fn dependents(db: &Database, id: &str) -> Vec<String> {
    let mut found = BTreeSet::new();
    found.insert(id.to_owned());
    loop {
        let before = found.len();
        for (key, p) in &db.plugins {
            if p.manifest.dependencies.keys().any(|d| found.contains(d)) {
                found.insert(key.clone());
            }
        }
        if found.len() == before {
            break;
        }
    }
    found.remove(id);
    found.into_iter().collect()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationPlan {
    pub enable: Vec<String>,
    pub disable: Vec<String>,
    pub fingerprint: String,
}
pub fn fingerprint(db: &Database) -> Result<String> {
    Ok(crate::package::digest(&serde_json::to_vec(db)?))
}
pub fn activation(db: &Database, id: &str) -> Result<ActivationPlan> {
    let enable = order(db, &[id.to_owned()], false)?;
    let mut disable = BTreeSet::new();
    for target in &enable {
        for (other, p) in &db.plugins {
            if p.enabled
                && !enable.contains(other)
                && conflict(&db.plugins[target].manifest, &p.manifest)?.is_some()
            {
                disable.insert(other.clone());
                for dep in dependents(db, other) {
                    if db.plugins[&dep].enabled {
                        disable.insert(dep);
                    }
                }
            }
        }
    }
    ensure!(
        enable.iter().all(|id| !disable.contains(id)),
        "必需依赖链内部存在冲突"
    );
    let mut proposed = db.clone();
    for id in &disable {
        proposed.plugins.get_mut(id).unwrap().enabled = false;
    }
    for id in &enable {
        proposed.plugins.get_mut(id).unwrap().enabled = true;
    }
    check_enabled(&proposed)?;
    Ok(ActivationPlan {
        enable,
        disable: disable.into_iter().collect(),
        fingerprint: fingerprint(db)?,
    })
}
/// Only changed targets constrain reverse dependencies; unrelated disabled plugins
/// may legitimately have missing dependencies after a confirmed uninstall.
pub fn validate_updates(db: &Database, changed: &[String]) -> Result<()> {
    for plugin in db.plugins.values() {
        for (id, dep) in &plugin.manifest.dependencies {
            if changed.contains(id) {
                let target = db.plugins.get(id).context("缺少更新目标")?;
                ensure!(
                    dep.matches(&target.manifest.version)? && source_matches(db, dep, id)?,
                    "更新会破坏 {} 对 {id} 的依赖约束 {}",
                    plugin.manifest.name,
                    dep.version()
                );
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    pub fn manifest(id: &str, deps: serde_json::Value) -> Manifest {
        serde_json::from_value(json!({"schemaVersion":1,"apiVersion":1,"id":id,"name":id,"author":"test","version":"1.0.0","dependencies":deps,"files":{"page.js":"a".repeat(64)}})).unwrap()
    }
    pub fn installed(db: &mut Database, m: Manifest) {
        db.plugins.insert(
            m.id.clone(),
            crate::model::Installed {
                manifest: m,
                enabled: true,
                favorite: false,
                order: 0,
                source: None,
                error: None,
            },
        );
    }
    #[test]
    fn semver_ranges_exact_and_invalid() {
        assert!(requirement(">=1.2.0 <2.0.0")
            .unwrap()
            .matches(&"1.4.0".parse().unwrap()));
        assert!(!requirement("1.2.0")
            .unwrap()
            .matches(&"1.2.1".parse().unwrap()));
        assert!(requirement("^1.2.0")
            .unwrap()
            .matches(&"1.9.0".parse().unwrap()));
        assert!(requirement("invalid").is_err());
    }
    #[test]
    fn cycle_order_conflict_and_activation() {
        let mut db = Database::default();
        installed(&mut db, manifest("base", json!({})));
        installed(&mut db, manifest("dependent", json!({"base":"^1.0.0"})));
        assert_eq!(
            order(&db, &["dependent".into()], true).unwrap(),
            ["base", "dependent"]
        );
        db.plugins
            .get_mut("base")
            .unwrap()
            .manifest
            .dependencies
            .insert("dependent".into(), Dependency::Version("*".into()));
        assert!(
            order(&db, &["dependent".into()], true)
                .unwrap_err()
                .to_string()
                .contains("base → dependent → base")
                || order(&db, &["dependent".into()], true)
                    .unwrap_err()
                    .to_string()
                    .contains("dependent → base → dependent")
        );
        db.plugins
            .get_mut("base")
            .unwrap()
            .manifest
            .dependencies
            .clear();
        let mut other = manifest("other", json!({}));
        other.conflicts.insert("base".into(), "*".into());
        installed(&mut db, other);
        db.plugins.get_mut("other").unwrap().enabled = false;
        let plan = activation(&db, "other").unwrap();
        assert_eq!(plan.disable, ["base", "dependent"]);
        db.plugins
            .get_mut("other")
            .unwrap()
            .manifest
            .dependencies
            .insert("base".into(), Dependency::Version("*".into()));
        assert!(activation(&db, "other").is_err());
    }
    #[test]
    fn explicit_source_survives_subscription_removal() {
        let mut db = Database::default();
        installed(&mut db, manifest("base", json!({})));
        db.plugins.get_mut("base").unwrap().source = Some("old-source".into());
        db.source_urls.insert(
            "old-source".into(),
            "https://example.org/catalog.json".into(),
        );
        let dep = Dependency::Source(DependencySource {
            version: "^1.0.0".into(),
            source: "https://EXAMPLE.org:443/catalog.json".into(),
        });
        assert!(source_matches(&db, &dep, "base").unwrap());
    }
}
