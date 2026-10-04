use crate::{
    ipc,
    model::*,
    package,
    process::{self, Events, Running},
    recovery::Recovery,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::{Read, Seek, SeekFrom},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{symlink, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub struct Service {
    verifier: crate::auth::Verifier,
    pub root: PathBuf,
    pub db: Database,
    pub manager: u32,
    running: BTreeMap<String, Running>,
    pub events: Events,
    notifications: BTreeMap<String, Value>,
    rates: BTreeMap<String, VecDeque<Instant>>,
    recovery: BTreeMap<String, Recovery>,
    updater: crate::update::Updater,
    runtime: BTreeMap<String, Value>,
    waiting: BTreeSet<String>,
    batching: bool,
}
impl Service {
    pub fn prepare_uninstall(&mut self) -> Result<Value> {
        self.updater.ensure_idle()?;
        self.updater.jobs.ensure_idle()?;
        let roots: Vec<_> = self.db.plugins.keys().cloned().collect();
        let mut ids = crate::relations::order(&self.db, &roots, false).unwrap_or(roots);
        ids.reverse();
        // Persist disabling before running hooks, so a failed cleanup or reboot
        // cannot restart a partially uninstalled plugin collection.
        self.db.safe_mode = true;
        for plugin in self.db.plugins.values_mut() {
            plugin.enabled = false;
        }
        self.save()?;
        for id in &ids {
            self.stop_one(id, "uninstall");
        }
        self.waiting.clear();
        for id in &ids {
            let installed = self.plugin(id)?.clone();
            let payload = self.payload(id)?;
            self.command_hook(
                &installed.manifest,
                &payload,
                "onUninstall",
                "uninstall",
                None,
                Value::Null,
            )
            .with_context(|| format!("插件 {id} 卸载失败，本体保留，请重试"))?;
            fs::remove_dir_all(self.root.join("plugins").join(id))?;
            self.db.plugins.remove(id);
            self.runtime.remove(id);
            self.save()?;
        }
        Ok(json!({"uninstalled": ids}))
    }
    pub fn load(root: &Path, manager: u32) -> Result<Self> {
        fs::create_dir_all(root)?;
        let p = root.join("state.json");
        let mut db: Database = if p.exists() {
            serde_json::from_slice(&fs::read(p)?)?
        } else {
            Database::default()
        };
        if db.update_source.is_none() {
            db.update_source = fs::read(root.join("update-source.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
        }
        let mut resume = BTreeSet::new();
        if let Ok(bytes) = fs::read(root.join("install-transaction.json")) {
            let journal: Value = serde_json::from_slice(&bytes)?;
            let original: Database = serde_json::from_value(journal["database"].clone())?;
            restore_links(
                root,
                &original,
                &serde_json::from_value::<Vec<String>>(journal["targets"].clone())?,
            )?;
            resume = serde_json::from_value(journal["running"].clone())?;
            db = original;
            fs::write(root.join("state.json.tmp"), serde_json::to_vec_pretty(&db)?)?;
            fs::set_permissions(
                root.join("state.json.tmp"),
                fs::Permissions::from_mode(0o600),
            )?;
            fs::File::open(root.join("state.json.tmp"))?.sync_all()?;
            fs::rename(root.join("state.json.tmp"), root.join("state.json"))?;
            fs::remove_file(root.join("install-transaction.json"))?;
        }
        let initialized_defaults = crate::subscriptions::initialize_defaults(&mut db)?;
        crate::subscriptions::initialize(&mut db)?;
        // Rehydrate presentation after a state-schema downgrade from immutable payloads.
        for installed in db.plugins.values_mut() {
            let path = root
                .join("plugins")
                .join(&installed.manifest.id)
                .join("versions")
                .join(&installed.manifest.version)
                .join("manifest.json");
            if let Ok(bytes) = fs::read(path) {
                if let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes) {
                    if manifest.id == installed.manifest.id
                        && manifest.version == installed.manifest.version
                        && manifest.validate().is_ok()
                    {
                        installed.manifest.ui = manifest.ui;
                        installed.manifest.icon = manifest.icon;
                        installed.manifest.author_url = manifest.author_url;
                        installed.manifest.documentation_url = manifest.documentation_url;
                        installed.manifest.homepage = manifest.homepage;
                        installed.manifest.details = manifest.details;
                        installed.manifest.tags = manifest.tags;
                        installed.manifest.screenshots = manifest.screenshots;
                        installed.manifest.changelog = manifest.changelog;
                        installed.manifest.lifecycle = manifest.lifecycle;
                        installed.manifest.backend = manifest.backend;
                        installed.manifest.dependencies = manifest.dependencies;
                        installed.manifest.optional_dependencies = manifest.optional_dependencies;
                        installed.manifest.conflicts = manifest.conflicts;
                        installed.manifest.exclusive_resources = manifest.exclusive_resources;
                    }
                }
            }
        }
        let service = Self {
            verifier: Default::default(),
            root: root.into(),
            db,
            manager,
            running: BTreeMap::new(),
            events: Arc::default(),
            notifications: BTreeMap::new(),
            rates: BTreeMap::new(),
            recovery: BTreeMap::new(),
            updater: crate::update::Updater::default(),
            runtime: BTreeMap::new(),
            waiting: resume,
            batching: false,
        };
        if initialized_defaults {
            service.save()?;
        }
        Ok(service)
    }
    fn save(&self) -> Result<()> {
        let tmp = self.root.join("state.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.db)?)?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
        fs::File::open(&tmp)?.sync_all()?;
        fs::rename(tmp, self.root.join("state.json"))?;
        fs::File::open(&self.root)?.sync_all()?;
        Ok(())
    }
    fn plugin(&self, id: &str) -> Result<&Installed> {
        valid_id(id)?;
        self.db.plugins.get(id).context("Plugin not installed")
    }
    fn active(&self, id: &str) -> Result<&Installed> {
        ensure!(self.agreement_accepted(), "请先同意用户协议和隐私声明");
        let p = self.plugin(id)?;
        ensure!(
            p.enabled && !self.db.safe_mode,
            "Plugin disabled or safe mode enabled"
        );
        Ok(p)
    }
    fn payload(&self, id: &str) -> Result<PathBuf> {
        let p = self.plugin(id)?;
        Ok(self
            .root
            .join("plugins")
            .join(id)
            .join("versions")
            .join(&p.manifest.version))
    }
    fn state(&mut self, id: &str, phase: &str, detail: Value) {
        let value = json!({"phase":phase,"detail":detail,"updatedAt":now_ms()});
        self.runtime.insert(id.into(), value.clone());
        process::event(
            &self.events,
            json!({"kind":"plugin.dependency.changed","plugin":id}),
        );
        use std::io::Write;
        let dir = self.root.join("logs");
        let _ = fs::create_dir_all(&dir);
        let log = dir.join(format!("{id}.lifecycle.log"));
        if fs::metadata(&log).is_ok_and(|m| m.len() > 2 * 1024 * 1024) {
            let _ = fs::rename(&log, log.with_extension("log.1"));
        }
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log) {
            let _ = writeln!(file, "{value}");
        }
        process::event(
            &self.events,
            json!({"kind":"plugin.lifecycle","plugin":id,"state":value}),
        );
    }
    fn cleanup_resources(&mut self, id: &str) {
        self.notifications.retain(|_, v| v["plugin"] != id);
        self.rates.remove(id);
        process::event(&self.events, json!({"kind":"plugin.disabled","plugin":id}));
        process::event(
            &self.events,
            json!({"kind":"notification.changed","plugin":id}),
        );
    }
    fn context(
        &self,
        m: &Manifest,
        phase: &str,
        reason: &str,
        previous: Option<&str>,
        extra: Value,
    ) -> Value {
        json!({"pluginId":m.id,"phase":phase,"reason":reason,"version":m.version,"previousVersion":previous,"dataDir":self.root.join("data").join(&m.id).join(match m.run_as(){RunAs::Steamos=>"steamos",RunAs::Root=>"root"}),"exit":extra})
    }
    fn command_hook(
        &mut self,
        m: &Manifest,
        payload: &Path,
        phase: &str,
        reason: &str,
        previous: Option<&str>,
        extra: Value,
    ) -> Result<()> {
        let Some(l) = &m.lifecycle else { return Ok(()) };
        let hook = match phase {
            "onInstall" => &l.on_install,
            "onUpdate" => &l.on_update,
            "onUninstall" => &l.on_uninstall,
            "onCrashCleanup" => &l.on_crash_cleanup,
            _ => return Ok(()),
        };
        let Some(hook) = hook else { return Ok(()) };
        self.state(&m.id, phase, json!({"reason":reason}));
        let result = process::hook(
            m,
            hook,
            payload,
            self.manager,
            &self.root.join("logs"),
            self.context(m, phase, reason, previous, extra),
            Duration::from_secs(l.timeout_seconds),
        );
        process::event(
            &self.events,
            json!({"kind":"plugin.hook","plugin":m.id,"phase":phase,"success":result.is_ok(),"error":result.as_ref().err().map(ToString::to_string)}),
        );
        result
    }
    fn stop(&mut self, id: &str) {
        self.stop_reason(id, "stop");
    }
    fn stop_one(&mut self, id: &str, reason: &str) {
        self.state(id, "stopping", json!({"reason":reason}));
        if let Some(mut running) = self.running.remove(id) {
            if let Some(m) = self.db.plugins.get(id).map(|p| p.manifest.clone()) {
                if let Some(l) = m.lifecycle.as_ref().filter(|l| l.on_stop) {
                    if !running.exited() {
                        let result = running.call_timeout(
                            "framely.lifecycle.stop",
                            self.context(&m, "onStop", reason, None, Value::Null),
                            Duration::from_secs(l.timeout_seconds),
                        );
                        process::event(
                            &self.events,
                            json!({"kind":"plugin.hook","plugin":id,"phase":"onStop","success":result.is_ok(),"error":result.err().map(|e|e.to_string())}),
                        );
                    }
                }
            }
            running.stop();
        }
        self.recovery.remove(id);
        self.cleanup_resources(id);
        self.state(id, "stopped", json!({"reason":reason}));
    }
    fn start(&mut self, id: &str) -> Result<()> {
        self.start_reason(id, "open")
    }
    fn start_one(&mut self, id: &str, reason: &str) -> Result<()> {
        self.active(id)?;
        if self.running.get_mut(id).is_some_and(|p| !p.exited()) {
            return Ok(());
        }
        if self.running.contains_key(id) {
            self.crashed(id);
        }
        self.active(id)?;
        let m = self.plugin(id)?.manifest.clone();
        if m.backend.is_none() {
            self.state(id, "ready", Value::Null);
            return Ok(());
        }
        self.state(id, "starting", json!({"reason":reason}));
        let result = (|| -> Result<Running> {
            let mut running = Running::start(
                &m,
                &self.payload(id)?,
                self.manager,
                self.events.clone(),
                &self.root.join("logs"),
            )?;
            if let Some(l) = m.lifecycle.as_ref().filter(|l| l.on_start) {
                running.call_timeout(
                    "framely.lifecycle.start",
                    self.context(&m, "onStart", reason, None, Value::Null),
                    Duration::from_secs(l.timeout_seconds),
                )?;
            }
            ensure!(!running.exited(), "Backend exited during startup");
            Ok(running)
        })();
        match result {
            Ok(running) => {
                self.running.insert(id.into(), running);
                self.recovery
                    .entry(id.into())
                    .or_default()
                    .started(Instant::now());
                self.db.plugins.get_mut(id).unwrap().error = None;
                self.state(id, "running", Value::Null);
                Ok(())
            }
            Err(error) => {
                self.cleanup_resources(id);
                if let Ok(payload) = self.payload(id) {
                    let _ = self.command_hook(
                        &m,
                        &payload,
                        "onCrashCleanup",
                        "start-failure",
                        None,
                        json!({"reason":"start-failure","message":error.to_string()}),
                    );
                }
                self.failed_backend(id, Instant::now(), &error.to_string());
                Err(error)
            }
        }
    }
    fn crashed(&mut self, id: &str) {
        self.suspend_dependents(id, "dependency-failed", true);
        let Some(mut running) = self.running.remove(id) else {
            return;
        };
        let exit = running.exit_info();
        running.stop();
        drop(running);
        self.cleanup_resources(id);
        if exit.reason == "exited"
            && exit.exit_code == Some(0)
            && exit.signal.is_none()
            && !exit.oom
        {
            self.recovery.remove(id);
            self.state(id, "stopped", json!({"reason":"backend-completed"}));
            if let Some(p) = self.db.plugins.get_mut(id) {
                p.error = None;
            }
            let _ = self.save();
            return;
        }
        self.state(id, "crashed", json!(exit));
        if let Ok(installed) = self.plugin(id).cloned() {
            if let Ok(payload) = self.payload(id) {
                if let Err(error) = self.command_hook(
                    &installed.manifest,
                    &payload,
                    "onCrashCleanup",
                    &exit.reason,
                    None,
                    json!(exit),
                ) {
                    process::event(
                        &self.events,
                        json!({"kind":"plugin.cleanup.failed","plugin":id,"message":error.to_string()}),
                    );
                }
            }
        }
        self.failed_backend(
            id,
            Instant::now(),
            &format!(
                "{} (exit={:?}, signal={:?}, OOM={})",
                exit.reason, exit.exit_code, exit.signal, exit.oom
            ),
        );
    }
    fn suspend_dependents(&mut self, id: &str, reason: &str, resume: bool) {
        let ids = crate::relations::dependents(&self.db, id);
        let ordered =
            crate::relations::order(&self.db, &ids, false).unwrap_or_else(|_| ids.clone());
        for target in ordered
            .into_iter()
            .rev()
            .filter(|target| ids.contains(target))
        {
            if self.running.contains_key(&target) {
                let budget = if resume {
                    self.recovery.remove(&target)
                } else {
                    None
                };
                self.stop_one(&target, reason);
                if resume {
                    if let Some(mut budget) = budget {
                        budget.started = None;
                        budget.next = None;
                        self.recovery.insert(target.clone(), budget);
                    }
                    self.waiting.insert(target.clone());
                    self.state(&target, "waiting-dependency", json!({"dependency":id}));
                }
            }
        }
        process::event(
            &self.events,
            json!({"kind":"plugin.dependency.changed","plugin":id}),
        );
    }
    fn stop_reason(&mut self, id: &str, reason: &str) {
        self.suspend_dependents(
            id,
            reason,
            ["restart", "update", "dependency-failed"].contains(&reason),
        );
        self.stop_one(id, reason);
    }
    fn start_reason(&mut self, id: &str, reason: &str) -> Result<()> {
        self.active(id)?;
        for (other, p) in &self.db.plugins {
            if other != id && p.enabled {
                if let Some(reason) =
                    crate::relations::conflict(&self.db.plugins[id].manifest, &p.manifest)?
                {
                    anyhow::bail!("{reason}")
                }
            }
        }
        let ordered = match crate::relations::order(&self.db, &[id.to_owned()], true) {
            Ok(v) => v,
            Err(e) => {
                self.state(id, "waiting-dependency", json!({"error":e.to_string()}));
                return Err(e);
            }
        };
        for target in ordered {
            if target != id
                && self
                    .recovery
                    .get(&target)
                    .is_some_and(|r| r.started.is_none())
            {
                self.state(id, "waiting-dependency", json!({"dependency":target}));
                anyhow::bail!("依赖正在恢复或已失败：{target}");
            }
            if let Err(e) = self.start_one(&target, reason) {
                if target != id {
                    self.state(
                        id,
                        "waiting-dependency",
                        json!({"dependency":target,"error":e.to_string()}),
                    );
                }
                return Err(e);
            }
        }
        self.waiting.remove(id);
        Ok(())
    }
    pub fn shutdown(&mut self) {
        let roots: Vec<_> = self.running.keys().cloned().collect();
        let ids = crate::relations::order(&self.db, &roots, false).unwrap_or(roots);
        for id in ids.into_iter().rev() {
            self.stop_one(&id, "manager-shutdown");
        }
        let _ = self.save();
    }
    pub fn autostart(&mut self) {
        if self.db.safe_mode || !self.agreement_accepted() {
            return;
        }
        let ids: Vec<_> = self
            .db
            .plugins
            .values()
            .filter(|p| p.enabled && p.manifest.backend.as_ref().is_some_and(|b| b.autostart))
            .map(|p| p.manifest.id.clone())
            .collect();
        for id in ids {
            let _ = self.start_reason(&id, "autostart");
        }
        let _ = self.save();
    }
    pub fn maintenance(&mut self) {
        if !self.agreement_accepted() {
            return;
        }
        let now = Instant::now();
        let exited: Vec<_> = self
            .running
            .iter_mut()
            .filter_map(|(id, r)| r.exited().then_some(id.clone()))
            .collect();
        for id in exited {
            self.crashed(&id);
        }
        let retry: Vec<_> = self
            .recovery
            .iter()
            .filter(|(id, r)| {
                !self.running.contains_key(*id)
                    && r.next.is_some_and(|t| now >= t)
                    && plugin_restarts(&self.db, id)
                    && !self.db.safe_mode
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in retry {
            let _ = self.start_reason(&id, "crash-recovery");
        }
        let waiting: Vec<_> = self.waiting.iter().cloned().collect();
        for id in waiting {
            if !self.db.plugins.get(&id).is_some_and(|p| p.enabled) {
                self.waiting.remove(&id);
                continue;
            }
            if self.db.safe_mode {
                continue;
            }
            if let Ok(order) = crate::relations::order(&self.db, std::slice::from_ref(&id), true) {
                if order.iter().filter(|target| *target != &id).all(|target| {
                    self.db.plugins[target].manifest.backend.is_none()
                        || self.running.contains_key(target)
                }) {
                    let _ = self.start_reason(&id, "dependency-restored");
                }
            }
        }
    }
    fn failed_backend(&mut self, id: &str, now: Instant, reason: &str) {
        let limit = self
            .db
            .plugins
            .get(id)
            .and_then(|p| p.manifest.backend.as_ref())
            .map(|b| b.restart_limit)
            .unwrap_or(3);
        let retry = plugin_restarts(&self.db, id) && !self.db.safe_mode;
        let quarantine = retry
            && self
                .recovery
                .entry(id.into())
                .or_default()
                .failed_with_limit(now, limit);
        if !retry {
            self.recovery.remove(id);
        }
        if let Some(p) = self.db.plugins.get_mut(id) {
            p.error = Some(if quarantine {
                format!("连续启动失败，已自动停用：{reason}")
            } else if retry {
                format!("正在等待自动重启：{reason}")
            } else {
                reason.into()
            });
            if quarantine {
                p.enabled = false;
            }
        }
        self.state(
            id,
            if retry && !quarantine {
                "recovering"
            } else {
                "failed"
            },
            json!({"reason":reason}),
        );
        if quarantine {
            process::event(&self.events, json!({"kind":"plugin.disabled","plugin":id}));
        }
        let _ = self.save();
    }
    fn agreement_accepted(&self) -> bool {
        self.db
            .agreement_acceptance
            .as_ref()
            .is_some_and(|a| a.version == AGREEMENT_VERSION && a.accepted_at > 0)
    }
    pub fn handle(&mut self, method: &str, p: Value) -> Result<Value> {
        ensure!(
            self.agreement_accepted()
                || matches!(
                    method,
                    "status"
                        | "events"
                        | "language.list"
                        | "language.save"
                        | "agreement.status"
                        | "agreement.accept"
                        | "agreement.revoke"
                        | "network.password.verify"
                        | "network.password.configured"
                        | "network.password.setup"
                        | "system.uninstall.prepare"
                ),
            "请先同意用户协议和隐私声明"
        );
        if method.starts_with("system.") && method != "system.job.status" {
            let installing = fs::read(self.root.join("update-status.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .is_some_and(|v| v["phase"] == "installing");
            ensure!(!installing, "正在切换 Framely 版本，请等待服务恢复");
        }
        let id = p["plugin"].as_str().unwrap_or("").to_owned();
        match method {
            "system.uninstall.prepare" => {
                ensure!(
                    p["approve"].as_bool() == Some(true),
                    "请确认卸载全部插件及 Framely"
                );
                self.prepare_uninstall()
            }
            "language.list" => crate::localization::list(&self.root),
            "language.install" => {
                let pack: crate::localization::LanguagePack =
                    serde_json::from_value(p["pack"].clone())?;
                crate::localization::install(&self.root, &pack)?;
                Ok(json!(true))
            }
            "language.save" => {
                let language = p["language"]
                    .as_str()
                    .context("Missing language")?
                    .to_owned();
                crate::localization::validate_selection(&self.root, &language)?;
                let old = self.db.language.clone();
                self.db.language = language;
                if let Err(e) = self.save() {
                    self.db.language = old;
                    return Err(e);
                }
                Ok(json!(true))
            }
            "agreement.status" => {
                Ok(json!({"version":AGREEMENT_VERSION,"accepted":self.agreement_accepted()}))
            }
            "agreement.revoke" => {
                ensure!(
                    p["approve"] == true && p["version"].as_str() == Some(AGREEMENT_VERSION),
                    "请确认拒绝协议将停用全部插件"
                );
                let old = self.db.clone();
                let targets: Vec<_> = self.db.plugins.keys().cloned().collect();
                let ordered = crate::relations::order(&self.db, &targets, false).unwrap_or(targets);
                self.db.agreement_acceptance = None;
                for plugin in self.db.plugins.values_mut() {
                    plugin.enabled = false;
                }
                if let Err(e) = self.save() {
                    self.db = old;
                    return Err(e);
                }
                self.waiting.clear();
                self.recovery.clear();
                for id in ordered.into_iter().rev() {
                    self.stop_one(&id, "agreement-revoked");
                }
                self.notifications.clear();
                process::event(&self.events, json!({"kind":"agreement.revoked"}));
                Ok(json!(true))
            }
            "agreement.accept" => {
                ensure!(
                    p["version"].as_str() == Some(AGREEMENT_VERSION)
                        && p["userAgreement"] == true
                        && p["privacyStatement"] == true,
                    "请明确同意当前用户协议和隐私声明"
                );
                if !self.agreement_accepted() {
                    let old = self.db.agreement_acceptance.clone();
                    self.db.agreement_acceptance = Some(AgreementAcceptance {
                        version: AGREEMENT_VERSION.into(),
                        accepted_at: now_ms(),
                    });
                    if let Err(e) = self.save() {
                        self.db.agreement_acceptance = old;
                        return Err(e);
                    }
                    self.autostart();
                }
                Ok(json!(true))
            }
            "status" => {
                self.maintenance();
                Ok(
                    json!({"agreement":{"version":AGREEMENT_VERSION,"accepted":self.agreement_accepted()},"version":env!("CARGO_PKG_VERSION"),"build":fs::read_to_string(self.root.join("current/VERSION")).unwrap_or_else(|_|env!("CARGO_PKG_VERSION").into()).trim(),"systemUpdate":fs::read(self.root.join("update-status.json")).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok()),"previousRelease":fs::read_to_string(self.root.join("previous-release")).ok().filter(|s|!s.trim().is_empty()),"apiVersion":API_VERSION,"runtime":self.runtime,"database":self.db,"running":self.running.iter_mut().filter_map(|(id,r)|if !r.exited(){Some(id.clone())}else{None}).collect::<Vec<_>>(),"notifications":self.notifications.values().collect::<Vec<_>>() }),
                )
            }
            "inspect" => {
                let (manifest, hash) = if p.get("packagePath").is_some() {
                    (
                        package::verify_manifest(package::open_staged(&p, self.manager)?)?,
                        p["sha256"]
                            .as_str()
                            .context("Missing package hash")?
                            .to_owned(),
                    )
                } else {
                    let staged = package::stage_request_proxy(&p, &self.db.proxy, |_, _| Ok(()))?;
                    (staged.manifest()?, staged.hash.clone())
                };
                let old = self.db.plugins.get(&manifest.id);
                let changed = old.is_some_and(|o| o.manifest.run_as() != manifest.run_as());
                Ok(
                    json!({"database":self.db,"fingerprint":crate::relations::fingerprint(&self.db)?,"manifest":manifest,"runAsChanged":changed,"sha256":hash}),
                )
            }
            "install" => self.install(p),
            "install.batch" => self.install_batch(p),
            "plugin.enable.preview" => Ok(json!(crate::relations::activation(&self.db, &id)?)),
            "plugin.disable.preview" => Ok(
                json!({"dependents":crate::relations::dependents(&self.db,&id).into_iter().filter(|id|self.db.plugins[id].enabled).collect::<Vec<_>>(),"fingerprint":crate::relations::fingerprint(&self.db)?}),
            ),
            "plugin.dependents" => Ok(json!(crate::relations::dependents(&self.db, &id))),
            "plugin.dependencies" => self.dependency_status(&id),
            "plugin.enable" => {
                let enabled = p["enabled"].as_bool().context("Missing enabled")?;
                self.plugin(&id)?;
                if enabled {
                    let plan = crate::relations::activation(&self.db, &id)?;
                    let extra = plan
                        .enable
                        .iter()
                        .any(|target| target != &id && !self.db.plugins[target].enabled)
                        || !plan.disable.is_empty();
                    ensure!(
                        !extra
                            || (p["approve"].as_bool() == Some(true)
                                && p["fingerprint"].as_str() == Some(plan.fingerprint.as_str())),
                        "请先确认依赖启用与冲突停用计划"
                    );
                    for target in &plan.disable {
                        self.stop_reason(target, "conflict");
                        self.db.plugins.get_mut(target).unwrap().enabled = false;
                        self.waiting.remove(target);
                    }
                    for target in &plan.enable {
                        let plugin = self.db.plugins.get_mut(target).unwrap();
                        plugin.enabled = true;
                        plugin.error = None;
                    }
                    self.save()?;
                    if plugin_autostart(&self.db, &id) {
                        self.start_reason(&id, "enable")?;
                    }
                } else {
                    self.disable_dependents(&id, &p)?;
                    self.stop_reason(&id, "disable");
                    self.db.plugins.get_mut(&id).unwrap().enabled = false;
                    self.waiting.remove(&id);
                    self.save()?;
                }
                process::event(
                    &self.events,
                    json!({"kind":"plugin.dependency.changed","plugin":id}),
                );
                Ok(json!(true))
            }
            "plugin.restart" => {
                self.active(&id)?;
                self.stop_reason(&id, "restart");
                self.start_reason(&id, "restart")?;
                self.save()?;
                Ok(json!(true))
            }
            "plugin.favorite" => {
                self.plugin(&id)?;
                let f = p["favorite"].as_bool().context("Missing favorite")?;
                self.db.plugins.get_mut(&id).unwrap().favorite = f;
                self.save()?;
                Ok(json!(true))
            }
            "plugin.order" => {
                let ids: Vec<String> = serde_json::from_value(p["plugins"].clone())?;
                let expected: std::collections::BTreeSet<_> =
                    self.db.plugins.keys().cloned().collect();
                ensure!(
                    ids.len() == expected.len()
                        && ids
                            .iter()
                            .cloned()
                            .collect::<std::collections::BTreeSet<_>>()
                            == expected,
                    "Order must contain each installed plugin once"
                );
                for (i, id) in ids.iter().enumerate() {
                    self.db.plugins.get_mut(id).unwrap().order = i as u32;
                }
                self.save()?;
                Ok(json!(true))
            }
            "plugin.uninstall" => {
                let installed = self.plugin(&id)?.clone();
                let payload = self.payload(&id)?;
                self.disable_dependents(&id, &p)?;
                self.stop_reason(&id, "uninstall");
                if let Err(error) = self.command_hook(
                    &installed.manifest,
                    &payload,
                    "onUninstall",
                    "uninstall",
                    None,
                    Value::Null,
                ) {
                    if p["force"].as_bool() != Some(true) {
                        self.db.plugins.get_mut(&id).unwrap().error =
                            Some(format!("卸载清理失败，可重试或强制卸载：{error}"));
                        self.state(&id, "failed", json!({"reason":error.to_string()}));
                        self.save()?;
                        return Err(error);
                    }
                }
                self.db.plugins.remove(&id);
                self.save()?;
                fs::remove_dir_all(self.root.join("plugins").join(&id))?;
                if p["purge"].as_bool() == Some(true) {
                    let data = self.root.join("data").join(&id);
                    if data.exists() {
                        fs::remove_dir_all(data)?;
                    }
                }
                self.runtime.remove(&id);
                Ok(json!(true))
            }
            "plugin.open" => {
                self.start(&id)?;
                Ok(json!(true))
            }
            "plugin.call" => {
                self.start(&id)?;
                let name = p["method"].as_str().context("Missing backend method")?;
                ensure!(
                    !name.starts_with("framely.lifecycle."),
                    "Lifecycle methods are reserved for the manager"
                );
                let r = self
                    .running
                    .get_mut(&id)
                    .context("Plugin has no backend")?
                    .call(name, p["params"].clone());
                if let Err(e) = &r {
                    self.db.plugins.get_mut(&id).unwrap().error = Some(e.to_string());
                    if self.running.get_mut(&id).is_some_and(|p| p.exited()) {
                        self.crashed(&id);
                    }
                }
                r
            }
            "window.open" => {
                let plugin = self.active(&id)?;
                let key = p["window"].as_str().context("Missing window")?;
                let window = plugin
                    .manifest
                    .ui
                    .windows
                    .get(key)
                    .context("Window not declared")?;
                let mut v = json!({"kind":"window.open","plugin":id,"window":key,"spec":window});
                if let Some(icon) = &plugin.manifest.icon {
                    v["spec"]["iconPath"] = json!(self.payload(&id)?.join(icon));
                }
                process::event(&self.events, v);
                Ok(json!(true))
            }
            "window.close" => {
                self.active(&id)?;
                let key = p["window"].as_str().context("Missing window")?;
                ensure!(
                    self.plugin(&id)?.manifest.ui.windows.contains_key(key),
                    "Window not declared"
                );
                process::event(
                    &self.events,
                    json!({"kind":"window.close","plugin":id,"window":key}),
                );
                Ok(json!(true))
            }
            "notification.send" => self.notify(&id, p["notification"].clone()),
            "notification.remove" => {
                self.active(&id)?;
                let key = p["id"].as_str().context("Missing notification ID")?;
                valid_id(key)?;
                self.notifications.remove(&format!("{id}:{key}"));
                process::event(&self.events, json!({"kind":"notification.changed"}));
                Ok(json!(true))
            }
            "notification.action" => {
                let key = p["id"].as_str().context("Missing ID")?;
                let action = p["action"].as_str().context("Missing action")?;
                let n = self
                    .notifications
                    .get(&format!("{id}:{key}"))
                    .context("Notification expired")?;
                ensure!(
                    n["expiresAt"].as_u64().unwrap_or(0) > now_ms(),
                    "Notification expired"
                );
                ensure!(
                    n["notification"]["actions"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v["id"] == action)),
                    "Action not declared"
                );
                process::event(
                    &self.events,
                    json!({"kind":"plugin.event","plugin":id,"event":"notification.action","data":{"id":key,"action":action}}),
                );
                if self.active(&id)?.manifest.backend.is_some() {
                    self.handle("plugin.call",json!({"plugin":id,"method":"notification.action","params":{"id":key,"action":action}}))
                } else {
                    Ok(json!(true))
                }
            }
            "events" => {
                let events: Vec<_> = self.events.lock().unwrap().drain(..).collect();
                let mut out = Vec::new();
                for e in events {
                    if e["kind"] == "plugin.event" {
                        let plugin = e["plugin"].as_str().unwrap_or("");
                        if e["event"] == "notification" {
                            let _ = self.notify(plugin, e["data"].clone());
                        } else {
                            out.push(e);
                        }
                    } else {
                        if e["kind"] == "plugin.error" {
                            if let Some(p) =
                                self.db.plugins.get_mut(e["plugin"].as_str().unwrap_or(""))
                            {
                                p.error = e["message"].as_str().map(str::to_owned);
                            }
                        }
                        out.push(e);
                    }
                }
                self.notifications
                    .retain(|_, v| v["expiresAt"].as_u64().unwrap_or(0) > now_ms());
                Ok(json!(out))
            }
            "system.source.save" => {
                let source: Option<UpdateSource> = serde_json::from_value(p["source"].clone())?;
                if let Some(source) = &source {
                    source.validate()?;
                }
                self.updater.ensure_idle()?;
                if let Some(source) = &source {
                    let path = self.root.join("update-source.json");
                    fs::write(&path, serde_json::to_vec(source)?)?;
                    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
                } else {
                    let _ = fs::remove_file(self.root.join("update-source.json"));
                }
                self.db.update_source = source;
                self.updater.clear();
                self.save()?;
                Ok(json!(true))
            }
            "system.channel.save" => {
                let channel: UpdateChannel = serde_json::from_value(p["channel"].clone())?;
                self.updater.ensure_idle()?;
                self.db.update_channel = channel;
                self.updater.clear();
                self.save()?;
                Ok(json!(true))
            }
            "system.check.start" => self.updater.check(
                self.db.update_source.clone().context("请先设置更新源")?,
                self.db.update_channel,
                self.db.proxy.clone(),
            ),
            "system.download.start" => self
                .updater
                .prepare(self.root.clone(), self.db.proxy.clone()),
            "system.job.status" => self
                .updater
                .jobs
                .status(p["job"].as_str().context("Missing job")?),
            "system.apply" => self.updater.apply(
                &self.root,
                self.manager,
                self.db.update_source.as_ref().context("请先配置更新源")?,
                self.db.update_channel,
                p,
            ),
            "system.rollback" => crate::update::Updater::rollback(&self.root, self.manager, p),
            "sources.save" => {
                let mut proposed = self.db.clone();
                crate::subscriptions::save_sources(
                    &mut proposed,
                    serde_json::from_value(p["sources"].clone())?,
                )?;
                self.db = proposed;
                self.save()?;
                Ok(json!(true))
            }
            "subscriptions.add" => {
                ensure!(p["approve"].as_bool() == Some(true), "请确认添加订阅");
                let mut proposed = self.db.clone();
                let id = crate::subscriptions::add(
                    &mut proposed,
                    p["url"].as_str().context("缺少订阅 URL")?,
                    serde_json::from_value(p["document"].clone())?,
                )?;
                self.db = proposed;
                self.save()?;
                Ok(json!({"id":id}))
            }
            "subscriptions.apply" => {
                let mut proposed = self.db.clone();
                let id = p["id"].as_str().context("缺少订阅 ID")?;
                let expected = p["snapshot"].as_str().context("缺少订阅快照")?;
                ensure!(
                    crate::package::digest(&serde_json::to_vec(
                        self.db
                            .subscriptions
                            .iter()
                            .find(|s| s.id == id)
                            .context("订阅已移除")?
                    )?) == expected,
                    "订阅已改变，请重新刷新"
                );
                crate::subscriptions::apply(
                    &mut proposed,
                    id,
                    serde_json::from_value(p["document"].clone())?,
                    serde_json::from_value(p["etag"].clone())?,
                    serde_json::from_value(p["lastModified"].clone())?,
                    serde_json::from_value(p["error"].clone())?,
                )?;
                self.db = proposed;
                self.save()?;
                Ok(json!(true))
            }
            "subscriptions.change" => {
                let mut proposed = self.db.clone();
                let accept = p["acceptUrls"].as_bool().unwrap_or(false);
                let remove = p["remove"].as_bool().unwrap_or(false);
                ensure!(
                    !(accept || remove) || p["approve"].as_bool() == Some(true),
                    "请确认订阅变更"
                );
                crate::subscriptions::change(
                    &mut proposed,
                    p["id"].as_str().context("缺少订阅 ID")?,
                    p["enabled"].as_bool(),
                    p["autoRefresh"].as_bool(),
                    accept,
                    remove,
                )?;
                self.db = proposed;
                self.save()?;
                Ok(json!(true))
            }
            "network.password.configured" => {
                Ok(json!(self.root.join("network-password.json").is_file()))
            }
            "network.password.verify" => self.verifier.verify(&self.root, &p),
            "proxy.save" => {
                let mut config: ProxySettings = serde_json::from_value(p)?;
                config.http = config.http.trim().to_owned();
                config.github = config.github.trim().trim_end_matches('/').to_owned();
                config.validate()?;
                self.db.proxy = config;
                self.save()?;
                Ok(json!(true))
            }
            "network.save" | "network.password.setup" => {
                let setup = method == "network.password.setup";
                let mut params = if setup {
                    ensure!(
                        !self.root.join("network-password.json").exists(),
                        "访问密码已设置，请登录"
                    );
                    let password = p["password"].as_str().context("请设置访问密码")?;
                    ensure!(
                        password.chars().count() >= 8 && password.len() <= 512,
                        "访问密码至少 8 个字符，最多 512 字节"
                    );
                    ensure!(
                        p["confirmPassword"].as_str() == Some(password),
                        "两次输入的密码不一致"
                    );
                    let mut config = self.db.network_panel.clone();
                    config.password_enabled = true;
                    // Persist authentication before creating credentials so an interrupted
                    // setup cannot leave a configured password with authentication disabled.
                    self.db.network_panel = config.clone();
                    self.save()?;
                    let mut params = serde_json::to_value(config)?;
                    params["password"] = json!(password);
                    params
                } else {
                    p.clone()
                };
                let password = params
                    .as_object_mut()
                    .context("Invalid settings")?
                    .remove("password");
                let config: NetworkPanel = serde_json::from_value(params)?;
                config.validate()?;
                if let Some(password) = password
                    .and_then(|p| p.as_str().map(str::to_owned))
                    .filter(|p| !p.is_empty())
                {
                    ensure!(password.len() <= 512, "密码过长");
                    let salt: [u8; 16] = rand::random();
                    let mut hash = [0u8; 32];
                    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(
                        password.as_bytes(),
                        &salt,
                        600_000,
                        &mut hash,
                    );
                    let file = self.root.join("network-password.json.new");
                    let mut options = fs::OpenOptions::new();
                    options.create(true).truncate(true).write(true);
                    use std::io::Write;
                    use std::os::unix::fs::OpenOptionsExt;
                    let mut out = options.mode(0o600).open(&file)?;
                    out.write_all(&serde_json::to_vec(
                        &json!({"salt":hex::encode(salt),"hash":hex::encode(hash)}),
                    )?)?;
                    out.sync_all()?;
                    fs::rename(file, self.root.join("network-password.json"))?;
                }
                ensure!(
                    !config.password_enabled || self.root.join("network-password.json").is_file(),
                    "请先设置访问密码"
                );
                self.db.network_panel = config;
                self.save()?;
                Ok(json!(true))
            }
            "safeMode" => {
                let enabled = p["enabled"].as_bool().context("Missing enabled")?;
                self.db.safe_mode = enabled;
                let ids: Vec<_> = self.running.keys().cloned().collect();
                for id in ids {
                    self.stop(&id);
                }
                self.save()?;
                if !enabled {
                    self.autostart();
                }
                Ok(json!(true))
            }
            "logs" => {
                if !id.is_empty() {
                    self.plugin(&id)?;
                }
                let mut result = String::new();
                let name = if id.is_empty() { "framely" } else { &id };
                for suffix in ["log", "lifecycle.log"] {
                    let path = self.root.join("logs").join(format!("{name}.{suffix}"));
                    if path.exists() {
                        let mut file = fs::File::open(path)?;
                        let start = file.metadata()?.len().saturating_sub(32768);
                        file.seek(SeekFrom::Start(start))?;
                        let mut data = Vec::new();
                        file.take(32768).read_to_end(&mut data)?;
                        result.push_str(&format!("--- {suffix} ---\n"));
                        result.push_str(&String::from_utf8_lossy(&data));
                        result.push('\n');
                    }
                }
                Ok(json!(result))
            }
            _ => bail!("Unknown service method"),
        }
    }
    fn notify(&mut self, id: &str, value: Value) -> Result<Value> {
        let plugin = self.active(id)?;
        let n: Notification = serde_json::from_value(value)?;
        n.validate()?;
        let name = plugin.manifest.name.clone();
        let rate = self.rates.entry(id.into()).or_default();
        let now = Instant::now();
        while rate
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(10))
        {
            rate.pop_front();
        }
        ensure!(rate.len() < 10, "Notification rate exceeded");
        rate.push_back(now);
        ensure!(
            self.notifications.len() < 64
                || self.notifications.contains_key(&format!("{id}:{}", n.id)),
            "Notification queue full"
        );
        let entry = json!({"plugin":id,"pluginName":name,"expiresAt":now_ms()+n.duration_ms,"notification":n});
        self.notifications.insert(format!("{id}:{}", n.id), entry);
        process::event(&self.events, json!({"kind":"notification.changed"}));
        Ok(json!(true))
    }
    fn install(&mut self, p: Value) -> Result<Value> {
        ensure!(
            p["approve"].as_bool() == Some(true),
            "Installation requires explicit approval"
        );
        let bytes = package::request_bytes(&p, &self.db.proxy, self.manager)?;
        let v = package::verify(&bytes)?;
        if let Some(hash) = p["inspectionHash"].as_str() {
            ensure!(
                package::digest(&bytes) == hash,
                "Package changed since review"
            );
        }
        let id = v.manifest.id.clone();
        if !self.batching {
            if let Some(expected) = p["fingerprint"].as_str() {
                ensure!(
                    crate::relations::fingerprint(&self.db)? == expected,
                    "插件状态已改变，请重新检查安装计划"
                );
            }
            let mut proposed = self.db.clone();
            let mut installed = proposed.plugins.get(&id).cloned().unwrap_or(Installed {
                manifest: v.manifest.clone(),
                enabled: true,
                favorite: false,
                order: proposed.plugins.len() as u32,
                source: None,
                error: None,
            });
            installed.manifest = v.manifest.clone();
            installed.source = p["source"].as_str().map(str::to_owned);
            proposed.plugins.insert(id.clone(), installed);
            crate::relations::order(&proposed, std::slice::from_ref(&id), false)?;
            crate::relations::validate_updates(&proposed, std::slice::from_ref(&id))?;
            crate::relations::check_enabled(&proposed)?;
        }
        if let Some(old) = self.db.plugins.get(&id) {
            let expanded = old.manifest.run_as() != v.manifest.run_as();
            ensure!(
                !expanded || p["approveRunAs"].as_bool() == Some(true),
                "Run user changed; confirmation required"
            );
        }
        let source = p["source"].as_str().map(str::to_owned);
        if let Some(s) = &source {
            ensure!(
                self.db.sources.iter().any(|q| q.id == *s),
                "Unknown installation source"
            );
        }
        let base = self.root.join("plugins").join(&id);
        fs::create_dir_all(base.join("versions"))?;
        let dest = base.join("versions").join(&v.manifest.version);
        let newly_created = !dest.exists();
        if newly_created {
            let stage = base.join(format!(".stage-{}", rand::random::<u64>()));
            if let Err(e) = package::unpack(&v, &stage) {
                let _ = fs::remove_dir_all(&stage);
                return Err(e);
            }
            fs::rename(stage, &dest)?;
        } else {
            let cached: Manifest = serde_json::from_slice(&fs::read(dest.join("manifest.json"))?)?;
            ensure!(
                serde_json::to_value(&cached)? == serde_json::to_value(&v.manifest)?,
                "同一插件版本的发布内容发生变化，拒绝替换"
            );
            for (file, expected) in &v.manifest.files {
                let path = dest.join(file);
                ensure!(
                    fs::symlink_metadata(&path)?.file_type().is_file()
                        && package::digest(&fs::read(path)?) == *expected,
                    "本机版本文件校验失败：{file}"
                );
            }
        }
        let was_running = self.running.contains_key(&id);
        let old = self.db.plugins.get(&id).cloned();
        self.stop_reason(&id, if old.is_some() { "update" } else { "install" });
        let phase = if old.is_some() {
            "onUpdate"
        } else {
            "onInstall"
        };
        if let Err(error) = self.command_hook(
            &v.manifest,
            &dest,
            phase,
            if old.is_some() { "update" } else { "install" },
            old.as_ref().map(|p| p.manifest.version.as_str()),
            Value::Null,
        ) {
            if newly_created {
                let _ = fs::remove_dir_all(&dest);
            }
            if let Some(previous) = &old {
                self.db.plugins.get_mut(&id).unwrap().error =
                    Some(format!("更新失败，保留旧版：{error}"));
                if was_running || (previous.enabled && plugin_autostart(&self.db, &id)) {
                    let _ = self.start_reason(&id, "update-rollback");
                }
            }
            self.state(&id, "failed", json!({"reason":error.to_string()}));
            self.save()?;
            return Err(error);
        }
        let installed = Installed {
            manifest: v.manifest,
            enabled: old.as_ref().map(|o| o.enabled).unwrap_or(true),
            favorite: old.as_ref().is_some_and(|o| o.favorite),
            order: old
                .as_ref()
                .map(|o| o.order)
                .unwrap_or(self.db.plugins.len() as u32),
            source: source.or_else(|| old.as_ref().and_then(|o| o.source.clone())),
            error: None,
        };
        let link = base.join("current.tmp");
        let _ = fs::remove_file(&link);
        symlink(
            Path::new("versions").join(&installed.manifest.version),
            &link,
        )?;
        fs::rename(link, base.join("current"))?;
        self.db.plugins.insert(id.clone(), installed);
        if let Err(e) = self.save() {
            if let Some(old) = old.clone() {
                let _ = fs::remove_file(base.join("current"));
                let _ = symlink(
                    Path::new("versions").join(&old.manifest.version),
                    base.join("current"),
                );
                self.db.plugins.insert(id, old);
            } else {
                self.db.plugins.remove(&id);
                let _ = fs::remove_file(base.join("current"));
            }
            if newly_created {
                let _ = fs::remove_dir_all(&dest);
            }
            return Err(e);
        }
        self.state(&id, "installed", Value::Null);
        if !self.batching
            && (was_running || plugin_autostart(&self.db, &id))
            && self.db.plugins[&id].enabled
            && !self.db.safe_mode
        {
            if let Err(error) = self.start_reason(&id, "install") {
                // A startup hook failure is an installation failure, not a successful activation.
                self.stop_reason(&id, "install-rollback");
                if let Some(previous) = old {
                    let _ = fs::remove_file(base.join("current"));
                    let _ = symlink(
                        Path::new("versions").join(&previous.manifest.version),
                        base.join("current"),
                    );
                    self.db.plugins.insert(id.clone(), previous);
                    if was_running || plugin_autostart(&self.db, &id) {
                        let _ = self.start_reason(&id, "update-rollback");
                    }
                } else {
                    self.db.plugins.remove(&id);
                    let _ = fs::remove_file(base.join("current"));
                }
                if newly_created {
                    let _ = fs::remove_dir_all(&dest);
                }
                self.save()?;
                return Err(error);
            }
        }
        Ok(json!(self.db.plugins[&id]))
    }
    fn disable_dependents(&mut self, id: &str, p: &Value) -> Result<()> {
        let ids: Vec<_> = crate::relations::dependents(&self.db, id)
            .into_iter()
            .filter(|id| self.db.plugins[id].enabled)
            .collect();
        ensure!(
            ids.is_empty()
                || (p["approveDependents"].as_bool() == Some(true)
                    && p["fingerprint"].as_str()
                        == Some(crate::relations::fingerprint(&self.db)?.as_str())),
            "此操作会停用依赖该插件的插件，请先确认"
        );
        for target in ids {
            self.stop_reason(&target, "dependency-disabled");
            self.db.plugins.get_mut(&target).unwrap().enabled = false;
            self.waiting.remove(&target);
        }
        Ok(())
    }
    fn dependency_status(&self, id: &str) -> Result<Value> {
        let m = &self.plugin(id)?.manifest;
        let mut items = vec![];
        for (target, dep) in m.dependencies.iter().chain(&m.optional_dependencies) {
            let p = self.db.plugins.get(target);
            let matches = p.is_some_and(|p| {
                dep.matches(&p.manifest.version).unwrap_or(false)
                    && crate::relations::source_matches(&self.db, dep, target).unwrap_or(false)
            });
            let available = matches
                && p.is_some_and(|p| p.enabled)
                && !self.db.safe_mode
                && p.is_some_and(|p| {
                    p.manifest.backend.is_none() || self.running.contains_key(target)
                });
            items.push(json!({"id":target,"required":m.dependencies.contains_key(target),"constraint":dep,"version":p.map(|p|&p.manifest.version),"enabled":p.map(|p|p.enabled).unwrap_or(false),"matches":matches,"available":available,"state":self.runtime.get(target)}));
        }
        Ok(json!(items))
    }
    fn install_batch(&mut self, p: Value) -> Result<Value> {
        ensure!(p["approve"].as_bool() == Some(true), "请确认批量安装");
        let plan: crate::planner::Plan = serde_json::from_value(p["plan"].clone())?;
        ensure!(
            plan.fingerprint == crate::relations::fingerprint(&self.db)?,
            "插件状态已改变，请重新检查安装计划"
        );
        let requests = p["packages"].as_array().context("缺少安装包快照")?;
        ensure!(
            !requests.is_empty() && requests.len() <= 32 && plan.items.len() <= 32,
            "无效批量安装数量"
        );
        let original = self.db.clone();
        let mut proposed = original.clone();
        for source in &plan.add_sources {
            source.validate()?;
            ensure!(
                !proposed.sources.iter().any(|s| s.id == source.id),
                "新增源 ID 已存在"
            );
            proposed.sources.push(source.clone());
        }
        crate::subscriptions::initialize(&mut proposed)?;
        let mut verified = BTreeMap::new();
        for request in requests {
            let bytes = package::request_bytes(request, &self.db.proxy, self.manager)?;
            let v = package::verify(&bytes)?;
            let source = request["source"].as_str().map(str::to_owned);
            let item = plan
                .items
                .iter()
                .find(|item| item.manifest.id == v.manifest.id)
                .context("安装包不在已审核计划中")?;
            ensure!(
                serde_json::to_value(&item.manifest)? == serde_json::to_value(&v.manifest)?
                    && item.source == source,
                "安装包与已审核计划不一致"
            );
            ensure!(
                source
                    .as_ref()
                    .is_none_or(|id| proposed.sources.iter().any(|s| &s.id == id)),
                "未知安装来源"
            );
            let mut installed =
                original
                    .plugins
                    .get(&v.manifest.id)
                    .cloned()
                    .unwrap_or(Installed {
                        manifest: v.manifest.clone(),
                        enabled: true,
                        favorite: false,
                        order: proposed.plugins.len() as u32,
                        source: None,
                        error: None,
                    });
            installed.manifest = v.manifest.clone();
            installed.source = source;
            proposed.plugins.insert(v.manifest.id.clone(), installed);
            ensure!(
                verified
                    .insert(v.manifest.id.clone(), request.clone())
                    .is_none(),
                "重复安装包"
            );
        }
        crate::relations::order(&proposed, std::slice::from_ref(&plan.root), false)?;
        crate::relations::validate_updates(
            &proposed,
            &verified.keys().cloned().collect::<Vec<_>>(),
        )?;
        let activation = if proposed
            .plugins
            .get(&plan.root)
            .context("缺少根插件")?
            .enabled
        {
            Some(crate::relations::activation(&proposed, &plan.root)?)
        } else {
            None
        };
        let (enable, disable) = activation
            .map(|p| (p.enable, p.disable))
            .unwrap_or_default();
        ensure!(
            enable == plan.enable && disable == plan.disable,
            "启用和冲突计划已改变"
        );
        for id in &disable {
            proposed
                .plugins
                .get_mut(id)
                .context("无效冲突插件")?
                .enabled = false;
        }
        for id in &enable {
            proposed
                .plugins
                .get_mut(id)
                .context("无效依赖插件")?
                .enabled = true;
        }
        crate::relations::check_enabled(&proposed)?;
        let targets: Vec<_> = verified.keys().cloned().collect();
        let mut impacted: BTreeSet<_> = targets.iter().cloned().collect();
        for id in &targets {
            impacted.extend(crate::relations::dependents(&original, id));
        }
        impacted.extend(disable.iter().cloned());
        let running: BTreeSet<_> = self
            .running
            .keys()
            .filter(|id| impacted.contains(*id))
            .cloned()
            .collect();
        let journal = json!({"database":original,"targets":targets,"running":running});
        let path = self.root.join("install-transaction.json");
        let temporary = self.root.join("install-transaction.tmp");
        fs::write(&temporary, serde_json::to_vec(&journal)?)?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        fs::File::open(&temporary)?.sync_all()?;
        fs::rename(temporary, &path)?;
        fs::File::open(&self.root)?.sync_all()?;
        for id in &impacted {
            self.stop_reason(id, "batch-install");
            self.waiting.remove(id);
        }
        self.db.sources = proposed.sources.clone();
        self.db.source_origins = proposed.source_origins.clone();
        self.db.source_urls = proposed.source_urls.clone();
        self.db.source_enabled = proposed.source_enabled.clone();
        self.batching = true;
        let result = (|| -> Result<Value> {
            for item in &plan.items {
                if let Some(request) = verified.get(&item.manifest.id) {
                    let mut request = request.clone();
                    request["approve"] = json!(true);
                    request["approveRunAs"] = p["approveRunAs"].clone();
                    self.install(request)?;
                }
            }
            for (id, plugin) in &proposed.plugins {
                if let Some(current) = self.db.plugins.get_mut(id) {
                    current.enabled = plugin.enabled;
                }
            }
            self.batching = false;
            self.save()?;
            let roots: Vec<_> = impacted
                .iter()
                .filter(|id| {
                    self.db.plugins.get(*id).is_some_and(|p| p.enabled)
                        && (running.contains(*id) || plugin_autostart(&self.db, id))
                })
                .cloned()
                .collect();
            for id in roots {
                self.start_reason(&id, "batch-install")?;
            }
            self.save()?;
            Ok(json!({"installed":targets,"disabled":disable}))
        })();
        self.batching = false;
        if let Err(error) = result {
            for id in &impacted {
                self.stop_reason(id, "transaction-rollback");
                self.waiting.remove(id);
            }
            restore_links(&self.root, &original, &targets)?;
            self.db = original;
            self.save()?;
            for id in &running {
                let _ = self.start_reason(id, "transaction-rollback");
            }
            self.save()?;
            fs::remove_file(path)?;
            return Err(error).context("批量安装失败，已恢复原版本和启用状态");
        }
        fs::remove_file(path)?;
        fs::File::open(&self.root)?.sync_all()?;
        for id in impacted {
            process::event(
                &self.events,
                json!({"kind":"plugin.dependency.changed","plugin":id}),
            );
        }
        result
    }
}
fn restore_links(root: &Path, original: &Database, targets: &[String]) -> Result<()> {
    for id in targets {
        valid_id(id)?;
        let base = root.join("plugins").join(id);
        if let Some(p) = original.plugins.get(id) {
            let path = Path::new("versions").join(&p.manifest.version);
            ensure!(
                base.join(&path).join("manifest.json").is_file(),
                "无法恢复旧版本：{id}"
            );
            let temporary = base.join("current.tmp");
            let _ = fs::remove_file(&temporary);
            symlink(path, &temporary)?;
            fs::rename(temporary, base.join("current"))?;
        } else if base.exists() {
            fs::remove_dir_all(base)?;
        }
    }
    Ok(())
}
fn plugin_restarts(db: &Database, id: &str) -> bool {
    db.plugins.get(id).is_some_and(|p| {
        p.enabled
            && p.manifest
                .backend
                .as_ref()
                .is_some_and(|b| b.restart == RestartPolicy::OnFailure)
    })
}
fn plugin_autostart(db: &Database, id: &str) -> bool {
    db.plugins
        .get(id)
        .is_some_and(|p| p.enabled && p.manifest.backend.as_ref().is_some_and(|b| b.autostart))
}
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn dispatch(core: &Arc<Mutex<Service>>, method: &str, params: Value) -> Result<Value> {
    if method == "network.password.verify" {
        let (root, verifier) = {
            let service = core.lock().unwrap();
            (service.root.clone(), service.verifier.clone())
        };
        return verifier.verify(&root, &params);
    }
    core.lock().unwrap().handle(method, params)
}

static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
extern "C" fn shutdown_signal(_: libc::c_int) {
    SHUTDOWN.store(true, std::sync::atomic::Ordering::Release);
}
pub fn serve(root: &Path, socket: &Path, manager: u32) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "Core service must run as root"
    );
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = shutdown_signal as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        ensure!(
            libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) == 0
                && libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()) == 0,
            "Cannot register shutdown signals"
        );
    }
    let mut core = Service::load(root, manager)?;
    fs::create_dir_all(socket.parent().context("Socket path needs parent")?)?;
    if socket.exists() {
        ensure!(
            UnixStream::connect(socket).is_err(),
            "Core service already running"
        );
        fs::remove_file(socket)?;
    }
    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o660))?;
    let group = Command::new("id")
        .args(["-g", &manager.to_string()])
        .output()?;
    ensure!(group.status.success(), "Cannot resolve Steam user group");
    let gid: libc::gid_t = std::str::from_utf8(&group.stdout)?.trim().parse()?;
    let name = std::ffi::CString::new(socket.as_os_str().as_encoded_bytes())?;
    ensure!(
        unsafe { libc::chown(name.as_ptr(), 0, gid) } == 0,
        "Cannot set socket owner"
    );
    listener.set_nonblocking(true)?;
    core.autostart();
    let core = Arc::new(Mutex::new(core));
    let monitor = core.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        if let Ok(mut core) = monitor.lock() {
            core.maintenance();
        }
    });
    while !SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(30));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let r = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut _,
                &mut len,
            )
        };
        if r != 0 || !(cred.uid == 0 || cred.uid == manager) {
            let _ = ipc::write(&mut stream, &json!({"error":"Unauthorized peer UID"}));
            continue;
        }
        stream.set_read_timeout(Some(Duration::from_secs(90)))?;
        let core = core.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Value> {
                let request = ipc::read(&mut stream)?;
                let method = request["method"].as_str().context("Missing method")?;
                ensure!(
                    method != "system.uninstall.prepare" || cred.uid == 0,
                    "Uninstall requires root"
                );
                ensure!(
                    !SHUTDOWN.load(std::sync::atomic::Ordering::Acquire),
                    "Manager is shutting down"
                );
                dispatch(&core, method, request["params"].clone())
            })();
            let response = match result {
                Ok(v) => json!({"result":v}),
                Err(e) => json!({"error":e.to_string()}),
            };
            let _ = ipc::write(&mut stream, &response);
        });
    }
    core.lock().unwrap().shutdown();
    let _ = fs::remove_file(socket);
    Ok(())
}

#[cfg(test)]
mod request_tests {
    use super::*;
    #[test]
    fn password_verification_does_not_hold_service_lock() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::load(root.path(), 1000).unwrap();
        let verifier = service.verifier.clone();
        fs::write(
            root.path().join("network-password.json"),
            serde_json::to_vec(&json!({"salt":"00".repeat(16),"hash":"00".repeat(32)})).unwrap(),
        )
        .unwrap();
        let core = Arc::new(Mutex::new(service));
        let worker_core = core.clone();
        let worker = std::thread::spawn(move || {
            dispatch(
                &worker_core,
                "network.password.verify",
                json!({"password":"wrong"}),
            )
            .unwrap()
        });
        for _ in 0..200 {
            if verifier.active() > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(verifier.active(), 1);
        let result = core
            .try_lock()
            .expect("Password verification blocked core")
            .handle("status", json!({}))
            .unwrap();
        assert!(result.get("database").is_some());
        assert_eq!(worker.join().unwrap(), json!(false));
    }
}
