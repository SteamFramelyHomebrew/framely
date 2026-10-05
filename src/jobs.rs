use crate::{ipc, package};
use anyhow::{ensure, Context, Result};
#[cfg(test)]
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Default)]
pub struct Cancellation {
    flag: Arc<AtomicBool>,
    commit: Arc<Mutex<()>>,
    parents: Vec<Arc<AtomicBool>>,
}
impl Cancellation {
    pub fn check(&self) -> Result<()> {
        ensure!(
            !self.flag.load(Ordering::Acquire)
                && !self.parents.iter().any(|p| p.load(Ordering::Acquire)),
            "任务已取消"
        );
        Ok(())
    }
    pub(crate) fn child(&self) -> Self {
        let mut parents = self.parents.clone();
        parents.push(self.flag.clone());
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            commit: self.commit.clone(),
            parents,
        }
    }
    pub(crate) fn stop(&self) {
        self.flag.store(true, Ordering::Release);
    }
    pub fn commit<T>(&self, action: impl FnOnce() -> Result<T>) -> Result<T> {
        let _guard = self.commit.lock().unwrap();
        self.check()?;
        action()
    }
}
#[derive(Clone)]
pub struct Jobs {
    pub uploads: crate::uploads::Uploads,
    state: Arc<Mutex<State>>,
}
#[derive(Default)]
struct State {
    jobs: BTreeMap<String, Job>,
}
struct Job {
    public: Value,
    cancel: Cancellation,
    active: bool,
    bytes: Option<Arc<package::Staged>>,
    batch: Option<Arc<crate::planner::Prepared>>,
    retained_at: Option<std::time::Instant>,
}
impl Default for Jobs {
    fn default() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let uploads = crate::uploads::Uploads::default();
        let weak = Arc::downgrade(&state);
        let cleaner = uploads.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            let Some(state) = weak.upgrade() else {
                break;
            };
            Self::expire(&mut state.lock().unwrap());
            cleaner.cleanup();
        });
        Self { state, uploads }
    }
}
impl Jobs {
    fn expire(state: &mut State) {
        for job in state.jobs.values_mut() {
            if !job.active
                && job
                    .retained_at
                    .is_some_and(|at| at.elapsed() > std::time::Duration::from_secs(900))
            {
                job.bytes = None;
                job.batch = None;
                job.retained_at = None;
            }
        }
    }

    pub fn ensure_idle(&self) -> Result<()> {
        ensure!(
            !self
                .state
                .lock()
                .unwrap()
                .jobs
                .values()
                .any(|job| job.active),
            "请等待下载和安装任务结束后再卸载 Framely"
        );
        Ok(())
    }
    fn create(&self, kind: &str) -> Result<(String, Cancellation, bool)> {
        let mut state = self.state.lock().unwrap();
        Self::expire(&mut state);
        if kind.starts_with("catalog:") || kind == "apk:list" {
            if let Some((id, job)) = state.jobs.iter().find(|(_, j)| {
                j.active && j.public["kind"] == kind && !j.cancel.flag.load(Ordering::Acquire)
            }) {
                return Ok((id.clone(), job.cancel.clone(), false));
            }
        }
        if kind.starts_with("subscription:") {
            ensure!(
                !state
                    .jobs
                    .values()
                    .any(|j| j.public["kind"] == kind && j.active),
                "该订阅正在刷新"
            );
        }
        ensure!(
            state.jobs.values().filter(|j| j.active).count() < 2,
            "已有两个任务正在运行"
        );
        if state.jobs.len() >= 12 {
            let remove = state
                .jobs
                .iter()
                .filter(|(_, j)| !j.active)
                .min_by_key(|(_, j)| j.public["createdAt"].as_u64())
                .map(|(id, _)| id.clone());
            if let Some(id) = remove {
                state.jobs.remove(&id);
            }
        }
        let id = format!("{:032x}", rand::random::<u128>());
        let cancel = Cancellation::default();
        state.jobs.insert(id.clone(), Job { public: json!({"id":id,"kind":kind,"phase":"queued","received":0,"total":null,"createdAt":crate::service::now_ms()}), cancel:cancel.clone(), active:true, bytes:None, batch:None, retained_at:None });
        Ok((id, cancel, true))
    }
    pub fn status(&self, id: &str) -> Result<Value> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .jobs
            .get(id)
            .context("任务已过期，请重新尝试")?
            .public
            .clone())
    }
    pub fn active_apk(&self) -> Value {
        let state = self.state.lock().unwrap();
        state
            .jobs
            .iter()
            .find(|(_, job)| {
                job.public["kind"] == "apk:operation"
                    && !matches!(
                        job.public["phase"].as_str(),
                        Some("done" | "failed" | "cancelled")
                    )
            })
            .map(|(id, _)| json!({"job":id}))
            .unwrap_or(json!({"job":null}))
    }
    pub fn cancel(&self, id: &str) -> Result<Value> {
        let cancel = {
            let state = self.state.lock().unwrap();
            let j = state.jobs.get(id).context("任务不存在")?;
            ensure!(
                j.public["kind"] != "install" && j.public["cancellable"] != false,
                "安装已经开始，不能中途取消"
            );
            j.cancel.clone()
        };
        // Serialize cancellation with state-changing commits, never with the jobs lock held.
        let _guard = cancel.commit.lock().unwrap();
        cancel.flag.store(true, Ordering::Release);
        let mut state = self.state.lock().unwrap();
        let j = state.jobs.get_mut(id).context("任务不存在")?;
        j.public["phase"] = json!("cancelled");
        j.bytes = None;
        j.batch = None;
        j.retained_at = None;
        Ok(json!(true))
    }
    fn patch(&self, id: &str, patch: Value) {
        if let Some(j) = self.state.lock().unwrap().jobs.get_mut(id) {
            if j.cancel.flag.load(Ordering::Acquire) {
                return;
            }
            for (k, v) in patch.as_object().unwrap() {
                j.public[k] = v.clone();
            }
        }
    }
    fn finish(&self, id: &str, result: Result<Value>) {
        if let Some(job) = self.state.lock().unwrap().jobs.get_mut(id) {
            job.active = false;
        }
        match result {
            Ok(v) => self.patch(id, json!({"phase":"done","result":v})),
            Err(e) => self.patch(id, json!({"phase":"failed","error":format!("{e:#}")})),
        }
    }
    pub fn task<F>(&self, kind: &str, task: F) -> Result<Value>
    where
        F: FnOnce(Cancellation) -> Result<Value> + Send + 'static,
    {
        self.task_progress(kind, move |cancel, _| task(cancel))
    }
    pub fn task_progress<F>(&self, kind: &str, task: F) -> Result<Value>
    where
        F: FnOnce(Cancellation, Arc<dyn Fn(Value) + Send + Sync>) -> Result<Value> + Send + 'static,
    {
        let (id, cancel, created) = self.create(kind)?;
        if !created {
            return Ok(json!({"job":id}));
        }
        let jobs = self.clone();
        let key = id.clone();
        std::thread::spawn(move || {
            jobs.patch(&key, json!({"phase":"loading"}));
            let progress_jobs = jobs.clone();
            let progress_key = key.clone();
            let progress = Arc::new(move |value| progress_jobs.patch(&progress_key, value));
            jobs.finish(&key, cancel.check().and_then(|_| task(cancel, progress)));
        });
        Ok(json!({"job":id}))
    }
    pub fn inspect(&self, socket: PathBuf, request: Value) -> Result<Value> {
        ensure!(
            request.get("packagePath").is_none(),
            "File paths are not accepted from the UI"
        );
        self.inspect_staged(socket, request, None)
    }
    fn inspect_staged(
        &self,
        socket: PathBuf,
        request: Value,
        staged: Option<Arc<package::Staged>>,
    ) -> Result<Value> {
        let (id, cancel, _) = self.create("inspect")?;
        let jobs = self.clone();
        let key = id.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Value> {
                jobs.patch(&key, json!({"phase":"downloading"}));
                let bytes = if let Some(staged) = staged {
                    staged
                } else if let Some(upload) = request["upload"].as_str() {
                    cancel.check()?;
                    jobs.uploads.take(upload)?
                } else {
                    let proxy = if request.get("package").is_some() {
                        Default::default()
                    } else {
                        serde_json::from_value(
                            ipc::call(&socket, "status", json!({}))?["database"]["proxy"].clone(),
                        )?
                    };
                    let download_request = request.clone();
                    let download_jobs = jobs.clone();
                    let download_key = key.clone();
                    let bytes = crate::http::interruptible(
                        &cancel,
                        std::time::Duration::from_secs(180),
                        move |token| {
                            package::stage_request_proxy(
                                &download_request,
                                &proxy,
                                |received, total| {
                                    token.check()?;
                                    download_jobs.patch(
                                        &download_key,
                                        json!({"received":received,"total":total}),
                                    );
                                    Ok(())
                                },
                            )
                        },
                    )?;
                    bytes
                };
                ensure!(cancel.check().is_ok(), "下载已取消");
                jobs.patch(&key, json!({"phase":"verifying"}));
                let manifest = bytes.manifest()?;
                if let Some(expected) = request["pluginId"].as_str() {
                    ensure!(manifest.id == expected, "插件包 ID 与目录不一致");
                }
                if let Some(expected) = request["version"].as_str() {
                    ensure!(manifest.version == expected, "插件包版本与目录不一致");
                }
                let mut info = ipc::call(&socket, "inspect", bytes.request())?;
                info["source"] = request["source"].clone();
                let database: crate::model::Database =
                    serde_json::from_value(info["database"].take())?;
                info.as_object_mut().unwrap().remove("database");
                info["installedVersion"] = database
                    .plugins
                    .get(&manifest.id)
                    .map(|plugin| json!(plugin.manifest.version))
                    .unwrap_or(Value::Null);
                let choices = serde_json::from_value(
                    request
                        .get("dependencySources")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                )?;
                info["dependencySources"] = request
                    .get("dependencySources")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                jobs.patch(&key, json!({"phase":"resolving"}));
                let batch = match crate::planner::prepare_staged(
                    &database,
                    bytes.clone(),
                    request["source"].as_str().map(str::to_owned),
                    choices,
                    cancel.clone(),
                ) {
                    Ok(batch) => {
                        info["plan"] = json!(batch.plan);
                        info["runAsChanged"] =
                            json!(batch.plan.items.iter().any(|item| item.run_as_changed));
                        Some(Arc::new(batch))
                    }
                    Err(error) => {
                        if let Some(choice) = error.downcast_ref::<crate::planner::ChoiceRequired>()
                        {
                            info["choiceRequired"] = json!(choice);
                            None
                        } else {
                            return Err(error);
                        }
                    }
                };

                let mut state = jobs.state.lock().unwrap();
                ensure!(cancel.check().is_ok(), "校验已取消");
                // Keep at most two immutable package snapshots for install approval.
                let oldest = state
                    .jobs
                    .iter()
                    .filter(|(k, j)| **k != key && j.bytes.is_some())
                    .min_by_key(|(_, j)| j.public["createdAt"].as_u64())
                    .map(|(k, _)| k.clone());
                if state.jobs.values().filter(|j| j.bytes.is_some()).count() >= 2 {
                    if let Some(k) = oldest {
                        let old = state.jobs.get_mut(&k).unwrap();
                        old.bytes = None;
                        old.batch = None;
                    }
                }
                let current = state.jobs.get_mut(&key).context("任务已过期")?;
                current.bytes = Some(bytes);
                current.retained_at = Some(std::time::Instant::now());
                current.batch = batch;
                Ok(info)
            })();
            jobs.finish(&key, result);
        });
        Ok(json!({"job":id}))
    }
    pub fn resolve(&self, socket: PathBuf, request: Value) -> Result<Value> {
        let ticket = request["ticket"].as_str().context("缺少已检查的安装包")?;
        let (bytes, info) = {
            let mut state = self.state.lock().unwrap();
            Self::expire(&mut state);
            let job = state.jobs.get(ticket).context("安装包已过期")?;
            ensure!(
                job.public["kind"] == "inspect"
                    && job.public["phase"] == "done"
                    && !job.cancel.flag.load(Ordering::Acquire),
                "检查尚未完成"
            );
            (
                job.bytes.clone().context("安装包已过期")?,
                job.public["result"].clone(),
            )
        };
        let mut choices = info["dependencySources"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for (id, source) in request["dependencySources"]
            .as_object()
            .context("缺少依赖来源选择")?
        {
            choices.insert(id.clone(), source.clone());
        }
        let result = self.inspect_staged(
            socket,
            json!({"source":info["source"],"dependencySources":choices}),
            Some(bytes),
        )?;
        let mut state = self.state.lock().unwrap();
        if let Some(old) = state.jobs.get_mut(ticket) {
            old.bytes = None;
            old.batch = None;
            old.retained_at = None;
        }
        Ok(result)
    }
    pub fn install(&self, socket: PathBuf, request: Value) -> Result<Value> {
        ensure!(request["approve"].as_bool() == Some(true), "请确认安装");
        let ticket = request["ticket"].as_str().context("缺少已校验安装包")?;
        let (batch, info) = {
            let mut state = self.state.lock().unwrap();
            Self::expire(&mut state);
            let j = state.jobs.get(ticket).context("安装包已过期，请重新检查")?;
            ensure!(
                j.public["phase"] == "done"
                    && j.public["kind"] == "inspect"
                    && !j.cancel.flag.load(Ordering::Acquire),
                "安装包未完成校验"
            );
            (
                j.batch
                    .clone()
                    .context("依赖计划未完成，请选择来源或重新检查")?,
                j.public["result"].clone(),
            )
        };
        let (id, _, _) = self.create("install")?;
        {
            let mut state = self.state.lock().unwrap();
            let reviewed = state.jobs.get_mut(ticket).context("安装包已过期")?;
            reviewed.bytes = None;
            reviewed.batch = None;
            reviewed.retained_at = None;
        }
        let jobs = self.clone();
        let key = id.clone();
        std::thread::spawn(move || {
            jobs.patch(&key, json!({"phase":"installing"}));
            let packages: Vec<_> = batch
                .packages
                .iter()
                .map(|(bytes, source)| {
                    let mut request = bytes.request();
                    request["source"] = json!(source);
                    request
                })
                .collect();
            let p = json!({"plan":info["plan"],"packages":packages,"approve":true,"approveRunAs":request["approveRunAs"]});
            let result = ipc::call_timeout(
                &socket,
                "install.batch",
                p,
                std::time::Duration::from_secs(1800),
            );
            drop(batch);
            jobs.finish(&key, result);
        });
        Ok(json!({"job":id}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_workers_keep_slots_until_exit_and_cannot_commit() {
        let jobs = Jobs::default();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let written = Arc::new(AtomicBool::new(false));
        let output = written.clone();
        let a = jobs
            .task("subscription:a", move |cancel| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                cancel.commit(|| {
                    output.store(true, Ordering::Release);
                    Ok(json!(true))
                })
            })
            .unwrap();
        started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let (b, _, _) = jobs.create("inspect").unwrap();
        jobs.cancel(a["job"].as_str().unwrap()).unwrap();
        assert!(jobs.create("catalog").is_err());
        release_tx.send(()).unwrap();
        for _ in 0..200 {
            if !jobs.state.lock().unwrap().jobs[a["job"].as_str().unwrap()].active {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!written.load(Ordering::Acquire));
        assert_eq!(
            jobs.status(a["job"].as_str().unwrap()).unwrap()["phase"],
            "cancelled"
        );
        let (c, _, _) = jobs.create("catalog").unwrap();
        jobs.finish(&b, Ok(json!(true)));
        jobs.finish(&c, Ok(json!(true)));
    }
    #[test]
    fn repeated_catalog_loads_share_one_worker_and_progress() {
        let jobs = Jobs::default();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let a=jobs.task_progress("catalog:same",move |_,progress| {progress(json!({"completedSources":1,"totalSources":2,"partial":[{"source":{"id":"fast"}}]}));release_rx.recv().unwrap();Ok(json!([]))}).unwrap();
        let b = jobs
            .task("catalog:same", |_| {
                panic!("duplicate catalog worker started")
            })
            .unwrap();
        assert_eq!(a["job"], b["job"]);
        for _ in 0..100 {
            if jobs.status(a["job"].as_str().unwrap()).unwrap()["completedSources"] == 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            jobs.status(a["job"].as_str().unwrap()).unwrap()["partial"][0]["source"]["id"],
            "fast"
        );
        release_tx.send(()).unwrap();
    }
    #[test]
    fn install_uses_reviewed_snapshot_and_ignores_replacement_request() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("core.sock");
        let bytes = crate::tests::fixture(root.path(), "test.snapshot", "1", 5, None);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let state = root.path().join("state");
        let thread = std::thread::spawn(move || {
            let mut core =
                crate::tests::accepted_service(&state, unsafe { libc::geteuid() }).unwrap();
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let r = ipc::read(&mut stream).unwrap();
                let result = core
                    .handle(r["method"].as_str().unwrap(), r["params"].clone())
                    .unwrap();
                ipc::write(&mut stream, &json!({"result":result})).unwrap();
            }
            core.db.plugins["test.snapshot"].manifest.version.clone()
        });
        let jobs = Jobs::default();
        let started = jobs
            .inspect(socket.clone(), json!({"package":STANDARD.encode(bytes)}))
            .unwrap();
        let ticket = started["job"].as_str().unwrap();
        wait_done(&jobs, ticket);
        let path = jobs.state.lock().unwrap().jobs[ticket]
            .bytes
            .as_ref()
            .unwrap()
            .path
            .clone();
        assert!(path.exists());
        let started = jobs
            .install(
                socket,
                json!({"ticket":ticket,"approve":true,"package":"unreviewed replacement"}),
            )
            .unwrap();
        let id = started["job"].as_str().unwrap();
        assert!(jobs.cancel(id).is_err());
        wait_done(&jobs, id);
        assert_eq!(thread.join().unwrap(), "1");
        assert!(!path.exists());
    }
    #[test]
    fn cancelled_and_expired_reviews_delete_staged_files() {
        let jobs = Jobs::default();
        for expired in [false, true] {
            let package = package::Staged::from_bytes(b"temporary package").unwrap();
            let path = package.path.clone();
            let (id, _, _) = jobs.create("inspect").unwrap();
            {
                let mut state = jobs.state.lock().unwrap();
                let job = state.jobs.get_mut(&id).unwrap();
                job.active = false;
                job.bytes = Some(package);
                job.retained_at = Some(
                    std::time::Instant::now()
                        - std::time::Duration::from_secs(if expired { 901 } else { 0 }),
                );
            }
            assert!(path.exists());
            if expired {
                Jobs::expire(&mut jobs.state.lock().unwrap());
            } else {
                jobs.cancel(&id).unwrap();
            }
            assert!(!path.exists());
        }
    }
    #[test]
    fn inspection_failure_releases_staged_package() {
        let root = tempfile::tempdir().unwrap();
        let package = package::Staged::from_bytes(b"invalid archive").unwrap();
        let path = package.path.clone();
        let jobs = Jobs::default();
        let started = jobs
            .inspect_staged(root.path().join("missing.sock"), json!({}), Some(package))
            .unwrap();
        let id = started["job"].as_str().unwrap();
        for _ in 0..200 {
            if jobs.status(id).unwrap()["phase"] == "failed" {
                assert!(!path.exists());
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("job timeout");
    }
    fn wait_done(jobs: &Jobs, id: &str) {
        for _ in 0..200 {
            let status = jobs.status(id).unwrap();
            if status["phase"] == "done" {
                return;
            }
            assert_ne!(status["phase"], "failed", "{status}");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("job timeout");
    }
    #[test]
    fn submitted_apk_operations_reject_cancellation_without_waiting_for_worker() {
        let jobs = Jobs::default();
        let (id, _, _) = jobs.create("apk:operation").unwrap();
        assert_eq!(jobs.active_apk()["job"], id);
        jobs.patch(&id, json!({"cancellable":false,"phase":"installing"}));
        assert!(jobs.cancel(&id).is_err());
        assert_eq!(jobs.status(&id).unwrap()["phase"], "installing");
        jobs.patch(&id, json!({"phase":"done"}));
        assert_eq!(jobs.active_apk()["job"], Value::Null);
    }
    #[test]
    fn cancellation_and_limits_do_not_allow_unreviewed_install() {
        let jobs = Jobs::default();
        let (a, _, _) = jobs.create("inspect").unwrap();
        let _ = jobs.create("inspect").unwrap();
        assert!(jobs.create("inspect").is_err());
        jobs.cancel(&a).unwrap();
        jobs.patch(&a, json!({"phase":"done"}));
        assert_eq!(jobs.status(&a).unwrap()["phase"], "cancelled");
        assert!(jobs
            .install(PathBuf::new(), json!({"ticket":a,"approve":true}))
            .is_err());
    }
}
