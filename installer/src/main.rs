#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use framely_installer::{
    DEFAULT_REPO,
    discovery::{self, Device},
    maintenance::{self, Installation},
    progress::{Progress, Stage},
    release::{self, Release},
    remote::{self, Connection, Credentials, Probe},
};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        input::{Input, InputState},
        theme::{Theme, ThemeMode},
        *,
    },
    *,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Clone, Copy)]
enum FileKind {
    Archive,
    Checksums,
}
enum Event {
    Device(u64, Device),
    ScanDone(u64, Result<(), String>),
    Releases(Result<Vec<Release>, String>),
    Probe(Result<Probe, String>),
    ConnectionStage(String),
    Connected(Result<Connection, String>),
    File(FileKind, Option<PathBuf>),
    Log(String),
    LogsExported(Result<Option<PathBuf>, String>),
    Progress(Progress),
    Done(Result<(), String>),
}
struct Installer {
    page: usize,
    system_proxy: bool,
    http_proxy: Entity<InputState>,
    github_proxy: Entity<InputState>,
    chosen_action: String,
    device_state: Option<Installation>,
    preview: bool,
    cidr: Entity<InputState>,
    host: Entity<InputState>,
    port: Entity<InputState>,
    user: Entity<InputState>,
    password: Entity<InputState>,
    repo: Entity<InputState>,
    devices: Vec<Device>,
    releases: Vec<Release>,
    selected: Option<usize>,
    show_testing: bool,
    archive: Option<PathBuf>,
    checksums: Option<PathBuf>,
    local: bool,
    probe: Option<Probe>,
    connection: Option<Arc<Mutex<Connection>>>,
    connected_host: String,
    busy: bool,
    scanning: bool,
    scan_options: bool,
    scan_id: u64,
    cancelled: Arc<AtomicBool>,
    confirmation: Option<String>,
    prompts: VecDeque<dialogs::Prompt>,
    operation_result: Option<(String, Result<(), String>)>,
    progress: Option<Progress>,
    logs: Vec<String>,
    status: String,
    error: Option<String>,
    release_error: Option<String>,
    connection_stage: Option<String>,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
}
fn field(label: &'static str, state: &Entity<InputState>, disabled: bool) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_sm().text_color(rgb(ui::MUTED)).child(label))
        .child(Input::new(state).h(px(42.)).disabled(disabled))
        .into_any_element()
}
impl Installer {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let mut input = |value: &str, placeholder: &str, masked: bool| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value.to_owned())
                    .placeholder(placeholder.to_owned())
                    .masked(masked)
            })
        };
        let proxy_result = framely_installer::proxy::load();
        let proxy_error = proxy_result
            .as_ref()
            .err()
            .map(|_| "无法读取代理设置，请在设置页重新保存。".to_owned());
        let proxies = proxy_result.unwrap_or_default();
        let networks = discovery::networks();
        let mut view = Self {
            page: 0,
            system_proxy: proxies.system,
            http_proxy: input(&proxies.http, "http://127.0.0.1:7890", false),
            github_proxy: input(&proxies.github, "https://your-github-proxy.example", false),
            chosen_action: "install".into(),
            device_state: None,
            preview: cfg!(feature = "visual-test")
                && std::env::var_os("FRAMELY_INSTALLER_PREVIEW_DIR").is_some(),
            cidr: input(
                networks
                    .first()
                    .map(String::as_str)
                    .unwrap_or("192.168.1.0/24"),
                "扫描网段，如 192.168.1.0/24",
                false,
            ),
            host: input("", "设备 IP 或主机名", false),
            port: input("22", "SSH 端口", false),
            user: input("steamos", "用户名", false),
            password: input("", "设备登录密码", true),
            repo: input(DEFAULT_REPO, "owner/repo", false),
            devices: Vec::new(),
            releases: Vec::new(),
            selected: None,
            show_testing: false,
            archive: None,
            checksums: None,
            local: false,
            probe: None,
            connection: None,
            connected_host: String::new(),
            busy: false,
            scanning: false,
            scan_options: false,
            scan_id: 0,
            cancelled: Arc::new(AtomicBool::new(false)),
            confirmation: None,
            prompts: VecDeque::new(),
            operation_result: None,
            progress: None,
            logs: Vec::new(),
            status: "开启 Frame 开发者模式并设置密码，将电脑和 Frame 连接到同一网络。".into(),
            error: proxy_error,
            release_error: None,
            connection_stage: None,
            sender,
            receiver,
        };
        if view.preview {
            view.device_state = Some(Installation {
                present: true,
                current_version: Some("0.4.0".into()),
                previous_version: Some("0.3.9".into()),
            });
            view.chosen_action = "update".into();
            view.page = std::env::var("FRAMELY_INSTALLER_PREVIEW_PAGE")
                .ok()
                .and_then(|page| page.parse::<usize>().ok())
                .unwrap_or(0)
                .min(2);
            view.devices = vec![
                Device {
                    name: "frame".into(),
                    ip: "192.168.1.42".into(),
                    ssh: true,
                },
                Device {
                    name: "frame".into(),
                    ip: "192.168.1.68".into(),
                    ssh: true,
                },
            ];
            view.host
                .update(cx, |input, cx| input.set_value("192.168.1.42", window, cx));
            view.cidr.update(cx, |input, cx| {
                input.set_value("192.168.1.0/24", window, cx)
            });
            view.releases = ["v0.4.1", "v0.4.0", "v0.3.9"].into_iter().map(|tag| Release {
                tag_name: tag.into(), name: Some(format!("Framely {tag}")),
                published_at: Some("2026-10-03T08:00:00Z".into()),
                body: Some("Framely 更新\n\n改进插件管理和设备连接体验。\n支持安装、更新、修复、回滚与卸载。".into()),
                prerelease: false, draft: false, assets: vec![],
            }).collect();
            view.selected = Some(0);
            view.status = "界面预览：设备与版本均为模拟数据，尚未连接真实设备。".into();
        }
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if weak
                    .update_in(cx, |view: &mut Installer, window, cx| view.poll(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        view
    }
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut changed = false;
        while let Ok(event) = self.receiver.try_recv() {
            changed = true;
            match event {
                Event::Device(id, device)
                    if id == self.scan_id && discovery::is_frame_name(&device.name) =>
                {
                    if let Some(existing) = self.devices.iter_mut().find(|d| d.ip == device.ip) {
                        existing.name = device.name;
                        existing.ssh |= device.ssh;
                    } else {
                        self.devices.push(device);
                    }
                    self.devices.sort_by_key(|d| d.ip.clone());
                }
                Event::ScanDone(id, result) if id == self.scan_id => {
                    self.scanning = false;
                    match result {
                        Ok(()) => {
                            self.status = if self.devices.is_empty() {
                                "未发现 Frame，可手动输入设备 IP".into()
                            } else {
                                format!("发现 {} 台 Frame", self.devices.len())
                            }
                        }
                        Err(error) => {
                            self.status = error.clone();
                            self.notice("扫描失败", error.clone());
                            self.error = Some(error);
                        }
                    }
                }
                Event::Releases(result) => {
                    self.busy = false;
                    match result {
                        Ok(items) => {
                            self.release_error = None;
                            self.releases = items;
                            self.status = format!("已加载 {} 个可安装版本。", self.releases.len());
                        }
                        Err(error) => {
                            self.status = error.clone();
                            self.notice(
                                "无法加载在线版本",
                                format!("可刷新重试，或选择本地安装包。\n\n{error}"),
                            );
                            self.release_error =
                                Some(format!("无法加载在线版本，可重试或选择本地安装包。{error}"));
                        }
                    }
                }
                Event::Probe(result) => {
                    self.busy = false;
                    match result {
                        Ok(probe) => {
                            self.status = "SSH 握手成功。确认下方设备指纹后登录。".into();
                            self.probe = Some(probe);
                        }
                        Err(error) => {
                            self.status = error.clone();
                            self.notice("SSH 连接检查失败", error.clone());
                            self.error = Some(error);
                        }
                    }
                }
                Event::ConnectionStage(stage) => {
                    self.status = stage.clone();
                    self.connection_stage = Some(stage);
                }
                Event::Connected(result) => {
                    self.connection_stage = None;
                    self.busy = false;
                    match result {
                        Ok(connection) => {
                            self.status =
                                format!("已连接 {}：{}", connection.host, connection.version);
                            self.chosen_action = connection.installation.default_action().into();
                            let installed = connection.installation.present;
                            self.device_state = Some(connection.installation.clone());
                            self.confirmation = None;
                            self.connection = Some(Arc::new(Mutex::new(connection)));
                            self.error = None;
                            self.page = if installed { 2 } else { 1 };
                            if !installed && self.releases.is_empty() && !self.local {
                                self.releases(cx);
                            }
                        }
                        Err(error) => {
                            self.status = error.clone();
                            self.notice("设备登录失败", error.clone());
                            self.error = Some(error);
                        }
                    }
                }
                Event::File(kind, path) => {
                    self.busy = false;
                    if let Some(path) = path {
                        self.local = true;
                        self.confirmation = None;
                        match kind {
                            FileKind::Archive => self.archive = Some(path),
                            FileKind::Checksums => self.checksums = Some(path),
                        };
                    }
                }
                Event::Log(line) => {
                    self.logs.push(line);
                    if self.logs.len() > 500 {
                        self.logs.remove(0);
                    }
                }
                Event::Progress(progress) => {
                    self.status = progress.detail.clone();
                    self.progress = Some(progress);
                }
                Event::LogsExported(result) => {
                    self.busy = false;
                    match result {
                        Ok(Some(path)) => {
                            self.status = "日志已导出。".into();
                            self.notice("日志导出完成", format!("已保存到：{}", path.display()));
                        }
                        Ok(None) => self.status = "已取消导出日志。".into(),
                        Err(error) => {
                            self.status = "日志导出失败。".into();
                            self.notice("日志导出失败", error);
                        }
                    }
                }
                Event::Done(result) => {
                    self.busy = false;
                    self.error = None;
                    self.status = if result.is_ok() {
                        "操作已完成，请查看操作结果。".into()
                    } else {
                        "操作失败，请查看操作结果与日志。".into()
                    };
                    let first_install = result.is_ok()
                        && self.chosen_action == "install"
                        && self
                            .device_state
                            .as_ref()
                            .is_some_and(|state| !state.present);
                    if first_install && !self.preview {
                        if let Some(url) = self.manager_url() {
                            cx.open_url(&url);
                        }
                    }
                    self.queue_prompt(dialogs::Prompt::Outcome(
                        self.chosen_action.clone(),
                        result.clone(),
                    ));
                    self.operation_result = Some((self.chosen_action.clone(), result));
                    // Avoid presenting a stale version or reusing an interrupted SSH channel.
                    self.connection = None;
                    self.device_state = None;
                    self.confirmation = None;
                    self.probe = None;
                }
                _ => {}
            }
        }
        if changed {
            cx.notify();
        }
        self.present_prompt(window, cx);
    }
    fn manager_url(&self) -> Option<String> {
        let completed = self
            .operation_result
            .as_ref()
            .is_some_and(|(action, result)| result.is_ok() && action != "uninstall");
        if self.connected_host.is_empty() || (self.connection.is_none() && !completed) {
            return None;
        }
        let host = self
            .connected_host
            .rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or(&self.connected_host)
            .trim_matches(['[', ']']);
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        Some(format!("http://{host}:15915/manager"))
    }
    fn export_logs(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let password = Zeroizing::new(self.password.read(cx).value().to_string());
        let sender = self.sender.clone();
        self.busy = true;
        self.status = "正在导出日志…".into();
        let logs = self.logs.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<Option<PathBuf>> {
                let Some(path) = rfd::FileDialog::new()
                    .set_title("保存 Framely 日志包")
                    .set_file_name("framely-logs.zip")
                    .add_filter("ZIP 日志包", &["zip"])
                    .save_file()
                else {
                    return Ok(None);
                };
                let guard = connection
                    .lock()
                    .map_err(|_| anyhow::anyhow!("设备连接不可用"))?;
                remote::export_logs(&guard, &password, &path, &logs)?;
                Ok(Some(path))
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = sender.send(Event::LogsExported(result));
        });
        cx.notify();
    }
    fn scan(&mut self, cx: &mut Context<Self>) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.cancelled = Arc::new(AtomicBool::new(false));
        self.scan_id += 1;
        self.devices.clear();
        self.scanning = true;
        let cidr = self.cidr.read(cx).value().to_string();
        let cancelled = self.cancelled.clone();
        let sender = self.sender.clone();
        let id = self.scan_id;
        self.error = None;
        self.status = "正在扫描局域网…".into();
        std::thread::spawn(move || {
            let emit_sender = sender.clone();
            let result = discovery::scan(
                &cidr,
                cancelled,
                Arc::new(move |device| {
                    let _ = emit_sender.send(Event::Device(id, device));
                }),
            )
            .map_err(|e| format!("{e:#}"));
            let _ = sender.send(Event::ScanDone(id, result));
        });
        cx.notify();
    }
    fn probe(&mut self, cx: &mut Context<Self>) {
        let host = self.host.read(cx).value().to_string();
        let port = self.port.read(cx).value().parse::<u16>();
        let Ok(port) = port else {
            self.status = "SSH 端口无效".into();
            self.error = Some(self.status.clone());
            self.notice("无法检查连接", self.status.clone());
            cx.notify();
            return;
        };
        self.busy = true;
        self.error = None;
        self.connection = None;
        self.device_state = None;
        self.confirmation = None;
        self.operation_result = None;
        self.progress = None;
        self.status = "正在检查 SSH 连接…".into();
        self.probe = None;
        self.connected_host = format!("{host}:{port}");
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Event::Probe(
                remote::probe(&host, port).map_err(|e| format!("{e:#}")),
            ));
        });
        cx.notify();
    }
    fn connect(&mut self, cx: &mut Context<Self>) {
        let Some(probe) = self.probe.clone() else {
            return;
        };
        let host = self.host.read(cx).value().to_string();
        let port = self.port.read(cx).value().parse::<u16>().unwrap_or(22);
        if format!("{host}:{port}") != self.connected_host {
            self.probe = None;
            self.status = "地址已变化，请重新检查 SSH。".into();
            self.error = Some(self.status.clone());
            self.notice("设备地址已变化", self.status.clone());
            cx.notify();
            return;
        }
        if self.password.read(cx).value().is_empty() {
            self.error = Some("请输入设备登录密码".into());
            self.notice("无法登录设备", "请输入设备登录密码。");
            cx.notify();
            return;
        }
        let credentials = Credentials {
            host,
            port,
            user: self.user.read(cx).value().to_string(),
            password: Zeroizing::new(self.password.read(cx).value().to_string()),
        };
        self.busy = true;
        self.error = None;
        self.status = "正在建立 SSH 连接…".into();
        self.connection_stage = Some(self.status.clone());
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = remote::connect(credentials, &probe.fingerprint, &mut |stage| {
                let _ = sender.send(Event::ConnectionStage(stage));
            })
            .map_err(|e| format!("{e:#}"));
            let _ = sender.send(Event::Connected(result));
        });
        cx.notify();
    }
    fn releases(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.selected = None;
        self.confirmation = None;
        self.error = None;
        self.release_error = None;
        self.status = "正在加载可安装版本…".into();
        let repo = self.repo.read(cx).value().to_string();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Event::Releases(
                release::list(&repo).map_err(|e| format!("{e:#}")),
            ));
        });
        cx.notify();
    }
    fn selection_ready(&self) -> bool {
        if self.local {
            self.archive.is_some() && self.checksums.is_some()
        } else {
            self.selected
                .and_then(|i| self.releases.get(i))
                .is_some_and(|r| release::package_for(r, &self.chosen_action).is_ok())
        }
    }
    fn action_available(&self, action: &str) -> bool {
        self.device_state
            .as_ref()
            .is_some_and(|state| state.allows(action))
    }
    fn same_update(&self) -> bool {
        let name = if self.local {
            self.archive
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
        } else {
            self.selected
                .and_then(|i| self.releases.get(i))
                .and_then(|r| release::package_for(r, "update").ok())
                .map(|(package, _)| package.name.as_str())
        };
        self.chosen_action == "update"
            && name.is_some_and(|name| {
                self.device_state
                    .as_ref()
                    .is_some_and(|state| state.same_package(name))
            })
    }
    fn request_operation(&mut self, cx: &mut Context<Self>) {
        if self.busy
            || self.confirmation.is_some()
            || self.connection.is_none()
            || !self.action_available(&self.chosen_action)
            || self.same_update()
        {
            return;
        }
        if maintenance::requires_package(&self.chosen_action) && !self.selection_ready() {
            self.navigate(1, cx);
            return;
        }
        self.confirmation = Some(self.chosen_action.clone());
        self.queue_prompt(dialogs::Prompt::Operation(self.chosen_action.clone()));
        cx.notify();
    }
    fn save_proxy(&mut self, cx: &mut Context<Self>) {
        let settings = framely_installer::proxy::DownloadSettings {
            system: self.system_proxy,
            http: self.http_proxy.read(cx).value().trim().to_owned(),
            github: self
                .github_proxy
                .read(cx)
                .value()
                .trim()
                .trim_end_matches('/')
                .to_owned(),
        };
        match framely_installer::proxy::save(settings) {
            Ok(()) => {
                self.error = None;
                self.release_error = None;
                self.releases.clear();
                self.selected = None;
                self.status = "代理设置已保存，下次下载及检查版本时生效。".into();
            }
            Err(error) => self.error = Some(format!("保存代理设置失败：{error}")),
        }
        cx.notify();
    }
    fn navigate(&mut self, page: usize, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if page == 2 && self.connection.is_none() && !self.preview {
            self.page = 0;
            self.error = Some("请先连接 Frame，再进行安装或维护".into());
            self.notice("请先连接设备", "请先连接 Frame，再进行安装或维护。");
            cx.notify();
            return;
        }
        self.page = page;
        self.confirmation = None;
        if page == 1 && !self.local && self.releases.is_empty() && !self.busy && !self.preview {
            self.releases(cx);
        }
        cx.notify();
    }
    fn file(&mut self, kind: FileKind, cx: &mut Context<Self>) {
        self.error = None;
        self.busy = true;
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new().set_title(match kind {
                FileKind::Archive => "选择 Framely ARM64 压缩包",
                FileKind::Checksums => "选择压缩包外部的 SHA256SUMS",
            });
            let _ = sender.send(Event::File(kind, dialog.pick_file()));
        });
        cx.notify();
    }
    fn operate(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(action) = self.confirmation.take() else {
            return;
        };
        if action != self.chosen_action
            || !self.action_available(&action)
            || self.same_update()
            || (maintenance::requires_package(&action) && !self.selection_ready())
        {
            self.error = Some("操作条件已变化，请重新选择操作或版本".into());
            self.notice("无法执行操作", "操作条件已变化，请重新选择操作或版本。");
            cx.notify();
            return;
        }
        let Some(connection) = self.connection.clone() else {
            self.status = "请先连接设备".into();
            self.notice("请先连接设备", "连接后重新选择操作。");
            cx.notify();
            return;
        };
        let local = self.local;
        let archive = self.archive.clone();
        let sums = self.checksums.clone();
        let selected = self.selected.and_then(|i| self.releases.get(i)).cloned();
        let repo = self.repo.read(cx).value().to_string();
        let password = Zeroizing::new(self.password.read(cx).value().to_string());
        self.busy = true;
        self.error = None;
        self.logs.clear();
        self.operation_result = None;
        self.progress = Some(Progress::new(Stage::Prepare, "正在准备操作…"));
        self.status = format!("正在执行 {action}…");
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<()> {
                let folder = tempfile::tempdir()?;
                let mut log = |line| {
                    let _ = sender.send(Event::Log(line));
                };
                let mut progress = |value| {
                    let _ = sender.send(Event::Progress(value));
                };
                let (archive, sums) = if matches!(action.as_str(), "install" | "update") {
                    if local {
                        (
                            Some(archive.ok_or_else(|| anyhow::anyhow!("请选择本地压缩包"))?),
                            Some(sums.ok_or_else(|| anyhow::anyhow!("请选择外部 SHA256SUMS"))?),
                        )
                    } else {
                        let selected =
                            selected.ok_or_else(|| anyhow::anyhow!("请选择 Release 版本"))?;
                        let (package, checksum) = release::package_for(&selected, &action)?;
                        let archive = folder.path().join(&package.name);
                        let sums = folder.path().join("SHA256SUMS");
                        let total = checksum
                            .size
                            .checked_add(package.size)
                            .ok_or_else(|| anyhow::anyhow!("下载长度无效"))?;
                        log(format!("下载 {}", package.name));
                        for (asset, path, offset) in
                            [(checksum, &sums, 0), (package, &archive, checksum.size)]
                        {
                            release::download(asset, path, &mut |mut value| {
                                value.completed += offset;
                                value.total = Some(total);
                                progress(value);
                            })?;
                        }
                        (Some(archive), Some(sums))
                    }
                } else {
                    (None, None)
                };
                if let (Some(archive), Some(sums)) = (&archive, &sums) {
                    release::verify_with_progress(archive, sums, &mut progress)?;
                    log("电脑端 SHA256 校验通过，开始传输；设备端将再次校验。".into());
                }
                remote::operate(
                    &connection.lock().unwrap(),
                    password,
                    remote::Operation {
                        action: &action,
                        archive: archive.as_deref(),
                        sums: sums.as_deref(),
                        repo: &repo,
                    },
                    &mut log,
                    &mut progress,
                )
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = sender.send(Event::Done(result));
        });
        cx.notify();
    }
}
impl Drop for Installer {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
mod dialogs;
mod ui;

fn transparent_window_root(window: &mut Window, cx: &mut App) {
    if let Some(Some(root)) = window.root::<gpui_kit::base::Root>() {
        root.update(cx, |root, cx| {
            root.style().background = Some(transparent_black().into());
            cx.notify();
        });
    }
}

fn initialize_theme(cx: &mut App) {
    gpui_kit::init(cx);
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.colors.background = rgb(ui::BG).into();
        theme.colors.foreground = rgb(ui::TEXT).into();
        theme.colors.border = rgb(ui::EDGE).into();
        theme.colors.input = rgb(0x494d54).into();
        theme.colors.primary = rgb(ui::BLUE).into();
        theme.colors.primary_foreground = rgb(0x192129).into();
        theme.colors.primary_hover = rgb(0xb1d5f1).into();
        theme.colors.primary_active = rgb(0x93c5ed).into();
        theme.colors.secondary = rgb(ui::PANEL).into();
        theme.colors.secondary_foreground = rgb(ui::TEXT).into();
        theme.colors.secondary_hover = rgb(0x363a41).into();
        theme.colors.button_primary = rgb(ui::BLUE).into();
        theme.colors.button_primary_foreground = rgb(0x192129).into();
        theme.colors.button_primary_hover = rgb(0xb1d5f1).into();
        theme.colors.button_primary_active = rgb(0x81b5dd).into();
        theme.colors.muted_foreground = rgb(ui::MUTED).into();
        theme.colors.ring = rgb(ui::BLUE).into();
        theme.radius = px(6.);
    });
}

#[cfg(feature = "visual-test")]
fn preview_pages(dir: PathBuf) -> anyhow::Result<()> {
    use gpui_kit::{HeadlessAppContext, test::TestWindowExt};
    std::fs::create_dir_all(&dir)?;
    let mut context = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(gpui_kit::assets::AllAssets),
        gpui_kit::platform::current_headless_renderer,
    );
    context.update(initialize_theme);
    context.update(|cx| cx.set_reduce_motion(true));
    let (handle, view) = context.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Default::default(),
                    size: size(
                        px(std::env::var("FRAMELY_PREVIEW_WIDTH")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(1080.)),
                        px(std::env::var("FRAMELY_PREVIEW_HEIGHT")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(800.)),
                    ),
                })),
                focus: false,
                show: false,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| Installer::new(window, cx)),
        )
    })?;
    context.update_window(handle, |_, window, cx| transparent_window_root(window, cx))?;
    context.allow_parking();
    for (page, name) in [
        (0, "devices"),
        (1, "versions"),
        (2, "maintenance"),
        (3, "settings"),
        (1, "local"),
        (0, "empty"),
        (0, "connecting"),
        (0, "connection-error"),
        (1, "connected"),
        (1, "release-error"),
        (2, "installed-no-package"),
        (2, "uninstalled"),
        (2, "same-version"),
        (2, "update-complete"),
        (2, "first-install-complete"),
        (2, "operation-failed"),
    ] {
        context.update_window(handle, |_, window, cx| {
            window.close_all_dialogs(cx);
            view.update(cx, |view, cx| {
                view.prompts.clear();
                view.confirmation = None;
                view.error = None;
                view.release_error = None;
                view.operation_result = None;
                view.progress = None;
                view.page = page;
                view.local = name == "local";
                if name == "empty" {
                    view.devices.clear();
                } else if name == "connecting" {
                    view.probe = Some(Probe {
                        fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
                        known: false,
                    });
                    view.busy = true;
                    view.sender
                        .send(Event::ConnectionStage("正在验证用户名和密码…".into()))
                        .unwrap();
                    view.poll(window, cx);
                    assert!(view.busy && view.connection_stage.is_some());
                } else if name == "connection-error" {
                    view.sender
                        .send(Event::Connected(Err(
                            "SSH 登录失败：请检查用户名和密码，并确认已开启开发者模式".into(),
                        )))
                        .unwrap();
                    view.poll(window, cx);
                    assert!(!view.busy && view.connection_stage.is_none() && view.error.is_some());
                } else if name == "connected" {
                    view.connected_host = "192.168.1.42:22".into();
                    view.sender
                        .send(Event::Connected(Ok(Connection {
                            session: ssh2::Session::new().unwrap(),
                            host: "192.168.1.42".into(),
                            user: "steamos".into(),
                            version: "frame · 0.4.0".into(),
                            installation: Installation {
                                present: true,
                                current_version: Some("0.4.0".into()),
                                previous_version: None,
                            },
                        })))
                        .unwrap();
                    view.poll(window, cx);
                    assert!(
                        view.connection.is_some()
                            && !view.busy
                            && view.error.is_none()
                            && view.page == 2
                            && view.chosen_action == "update"
                    );
                } else if name == "release-error" {
                    view.releases.clear();
                    view.selected = None;
                    view.sender
                        .send(Event::Releases(Err("GitHub API: status code 404".into())))
                        .unwrap();
                    view.poll(window, cx);
                    assert!(!view.busy && view.error.is_none() && view.release_error.is_some());
                }
                if name == "installed-no-package" {
                    view.preview = true;
                    view.device_state = Some(Installation {
                        present: true,
                        current_version: Some("0.4.0".into()),
                        previous_version: None,
                    });
                    view.selected = None;
                    view.chosen_action = "repair".into();
                    assert!(
                        !view.action_available("install")
                            && !view.action_available("rollback")
                            && view.action_available("repair")
                    );
                } else if name == "uninstalled" {
                    view.device_state = Some(Installation::default());
                    view.chosen_action = "install".into();
                    assert!(view.action_available("install") && !view.action_available("update"));
                } else if name == "same-version" {
                    view.device_state = Some(Installation {
                        present: true,
                        current_version: Some("0.4.0-build".into()),
                        previous_version: None,
                    });
                    view.chosen_action = "update".into();
                    view.releases = vec![Release {
                        tag_name: "v0.4.0".into(),
                        name: None,
                        published_at: None,
                        body: None,
                        prerelease: false,
                        draft: false,
                        assets: vec![
                            release::Asset {
                                name: "framely-0.4.0-build-linux-arm64.tar.gz".into(),
                                browser_download_url: "https://example.org/package".into(),
                                size: 1,
                            },
                            release::Asset {
                                name: "SHA256SUMS".into(),
                                browser_download_url: "https://example.org/sums".into(),
                                size: 1,
                            },
                        ],
                    }];
                    view.selected = Some(0);
                    view.confirmation = None;
                    assert!(view.same_update() && view.selection_ready());
                    view.request_operation(cx);
                    assert!(view.confirmation.is_none());
                } else if matches!(
                    name,
                    "update-complete" | "first-install-complete" | "operation-failed"
                ) {
                    view.chosen_action = if name == "first-install-complete" {
                        "install"
                    } else {
                        "update"
                    }
                    .into();
                    view.connected_host = "192.168.1.42:22".into();
                    if name == "first-install-complete" {
                        view.device_state = Some(Installation::default());
                    }

                    view.sender
                        .send(Event::Done(if name != "operation-failed" {
                            Ok(())
                        } else {
                            Err("网络连接中断，请重新连接设备后重试。".into())
                        }))
                        .unwrap();
                    view.poll(window, cx);
                    assert!(
                        view.operation_result.is_some()
                            && view.connection.is_none()
                            && view.confirmation.is_none()
                            && !view.busy
                    );
                    assert_eq!(view.manager_url().is_some(), name != "operation-failed");
                    if name != "operation-failed" {
                        assert_eq!(
                            view.manager_url().as_deref(),
                            Some("http://192.168.1.42:15915/manager")
                        );
                        view.connected_host = "[fd00::42]:22".into();
                        assert_eq!(
                            view.manager_url().as_deref(),
                            Some("http://[fd00::42]:15915/manager")
                        );
                        view.connected_host = "192.168.1.42:22".into();
                    }
                }
                cx.notify();
            });
            window.render_frame(cx);
        })?;
        context.run_until_parked();
        context.update_window(handle, |_, window, cx| window.render_frame(cx))?;
        context
            .capture_screenshot(handle)?
            .save(dir.join(format!("installer-{name}.png")))?;
    }
    // Exercise actual modal hit testing, cancellation and stale confirmation.
    for action in ["install", "update", "repair", "rollback", "uninstall"] {
        context.update_window(handle, |_, window, cx| {
            window.close_all_dialogs(cx);
            view.update(cx, |view, cx| {
                view.page = 2;
                view.busy = false;
                view.operation_result = None;
                view.progress = None;
                view.prompts.clear();
                view.confirmation = None;
                view.error = None;
                view.release_error = None;
                view.status = "已连接 192.168.1.42：frame · 0.4.0".into();
                view.local = false;
                view.selected = Some(0);
                view.releases[0].tag_name = "v0.4.1".into();
                view.device_state = Some(Installation {
                    present: action != "install",
                    current_version: (action != "install").then(|| "0.4.0".into()),
                    previous_version: Some("0.3.9".into()),
                });
                view.connection = Some(Arc::new(Mutex::new(Connection {
                    session: ssh2::Session::new().unwrap(),
                    host: "192.168.1.42".into(),
                    user: "steamos".into(),
                    version: "frame · 0.4.0".into(),
                    installation: view.device_state.clone().unwrap(),
                })));
                view.chosen_action = action.into();
                view.request_operation(cx);
                view.request_operation(cx);
                assert_eq!(
                    view.prompts.len(),
                    1,
                    "double activation must not queue two confirmations"
                );
                view.poll(window, cx);
                assert!(view.confirmation.is_some());
            });
            window.render_frame(cx);
            assert!(window.has_active_dialog(cx));
            assert!(window.find("dialog").visible());
            window.click("step-0", cx);
            assert_eq!(
                view.read(cx).page,
                2,
                "modal must block background navigation"
            );
        })?;
        context.run_until_parked();
        context.update_window(handle, |_, window, cx| window.render_frame(cx))?;
        context
            .capture_screenshot(handle)?
            .save(dir.join(format!("installer-confirm-{action}.png")))?;
        context.update_window(handle, |_, window, cx| {
            window.within("dialog").click("cancel", cx);
            window.render_frame(cx);
            assert!(!window.has_active_dialog(cx));
            assert!(view.read(cx).confirmation.is_none() && !view.read(cx).busy);
            view.update(cx, |view, cx| {
                view.request_operation(cx);
                view.poll(window, cx);
            });
            window.render_frame(cx);
            window.press("escape", cx);
            window.render_frame(cx);
            assert!(!window.has_active_dialog(cx));
            assert!(view.read(cx).confirmation.is_none() && !view.read(cx).busy);
            view.update(cx, |view, cx| {
                view.request_operation(cx);
                view.poll(window, cx);
            });
            window.render_frame(cx);
            window.within("dialog").click("close", cx);
            window.render_frame(cx);
            assert!(!window.has_active_dialog(cx));
            assert!(view.read(cx).confirmation.is_none() && !view.read(cx).busy);
            view.update(cx, |view, cx| {
                view.request_operation(cx);
                view.poll(window, cx);
            });
            window.render_frame(cx);
            view.update(cx, |view, _| {
                view.device_state = None;
            });
            window.within("dialog").click("ok", cx);
            assert!(
                !view.read(cx).busy,
                "stale confirmation must not start SSH operations"
            );
            view.update(cx, |view, cx| view.poll(window, cx));
            window.render_frame(cx);
            assert!(
                window.has_active_dialog(cx),
                "stale confirmation must show an error dialog"
            );
            window.within("dialog").click("ok", cx);
            window.render_frame(cx);
            assert!(!window.has_active_dialog(cx));
        })?;
    }
    for (stage, name) in [
        (Stage::Download, "download"),
        (Stage::Verify, "verify"),
        (Stage::Transfer, "transfer"),
        (Stage::Install, "install"),
    ] {
        context.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.chosen_action = "install".into();
                view.error = None;
                view.release_error = None;
                view.busy = true;
                view.local = false;
                view.logs = vec!["电脑端 SHA256 校验通过。".into()];
                let mut progress = Progress::new(
                    stage,
                    match stage {
                        Stage::Download => "framely-0.4.1-linux-arm64.tar.gz",
                        Stage::Verify => "校验本地安装包 SHA256",
                        Stage::Transfer => "framely-0.4.1-linux-arm64.tar.gz",
                        _ => "解压并校验发行文件",
                    },
                );
                if stage != Stage::Verify {
                    progress.completed = 25 * 1048576;
                    progress.total = Some(100 * 1048576);
                }
                if stage == Stage::Install {
                    progress.step = Some((3, 5));
                }
                view.sender.send(Event::Progress(progress)).unwrap();
                view.poll(window, cx);
                assert_eq!(view.progress.as_ref().unwrap().stage, stage);
            });
            window.render_frame(cx);
            assert!(window.find("operation-progress").visible());
        })?;
        context.run_until_parked();
        context.update_window(handle, |_, window, cx| window.render_frame(cx))?;
        context
            .capture_screenshot(handle)?
            .save(dir.join(format!("installer-progress-{name}.png")))?;
    }
    eprintln!("Installer modal and progress checks passed.");
    Ok(())
}

fn main() {
    #[cfg(feature = "visual-test")]
    if let Some(dir) = std::env::var_os("FRAMELY_INSTALLER_PREVIEW_DIR") {
        preview_pages(PathBuf::from(dir)).expect("安装器预览渲染失败");
        return;
    }
    let _instance = match framely_installer::instance::InstanceGuard::acquire() {
        Ok(Some(instance)) => instance,
        Ok(None) => {
            eprintln!("Framely 安装器已经运行。");
            return;
        }
        Err(error) => {
            eprintln!("无法启动 Framely 安装器：{error:#}");
            std::process::exit(1);
        }
    };
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            cx.set_app_identity("org.framely.installer", "Framely Installer");
            initialize_theme(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let options = WindowOptions {
                app_id: Some("org.framely.installer".into()),
                window_min_size: Some(size(px(980.), px(720.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Framely Installer".into()),
                    ..TitleBar::title_bar_options()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1080.), px(800.)),
                    cx,
                ))),
                window_decorations: Some(WindowDecorations::Client),
                window_background: WindowBackgroundAppearance::Transparent,
                ..TitleBar::window_options()
            };
            let (handle, _) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Installer::new(window, cx))
            })
            .expect("无法打开 Framely 安装器窗口");
            handle
                .update(cx, |_, window, cx| transparent_window_root(window, cx))
                .expect("无法设置窗口背景");
            cx.activate(true);
        });
}
