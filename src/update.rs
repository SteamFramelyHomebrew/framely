use crate::{
    jobs::Jobs,
    model::{UpdateChannel, UpdateSource, API_VERSION},
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const MAX_RELEASE: u64 = 512 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Release {
    pub schema_version: u32,
    pub version: String,
    pub api_version: u32,
    pub arch: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub changelog: String,
}
pub fn verify_descriptor(bytes: &[u8]) -> Result<Release> {
    ensure!(bytes.len() <= 256 * 1024, "更新清单过大");
    let release: Release = serde_json::from_slice(bytes)?;
    ensure!(
        release.schema_version == 1
            && release.api_version <= API_VERSION
            && release.arch == "aarch64",
        "不兼容的发行包"
    );
    ensure!(
        !release.version.is_empty()
            && release.version.len() <= 80
            && release
                .version
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".+-".contains(&c)),
        "无效版本号"
    );
    crate::model::validate_url(&release.url, false)?;
    ensure!(
        hex::decode(&release.sha256)?.len() == 32
            && release.size > 0
            && release.size <= MAX_RELEASE
            && release.changelog.len() <= 32768,
        "无效发行包信息"
    );
    Ok(release)
}
pub fn create_descriptor(archive: &Path, url: &str, output: &Path, changelog: &str) -> Result<()> {
    crate::model::validate_url(url, false)?;
    let filename = archive
        .file_name()
        .and_then(|p| p.to_str())
        .context("发行包文件名无效")?;
    let version = filename
        .strip_prefix("framely-")
        .and_then(|n| n.strip_suffix("-linux-arm64.tar.gz"))
        .context("必须使用 Framely 发行包命名")?;
    ensure!(
        !version.ends_with("-offline"),
        "更新清单必须使用不含 CEF 的本体包"
    );
    let release = Release {
        schema_version: 1,
        version: version.into(),
        api_version: API_VERSION,
        arch: "aarch64".into(),
        url: url.into(),
        sha256: file_digest(archive)?,
        size: fs::metadata(archive)?.len(),
        changelog: changelog.into(),
    };
    let bytes = serde_json::to_vec_pretty(&release)?;
    verify_descriptor(&bytes)?;
    fs::write(output, bytes)?;
    Ok(())
}
fn file_digest(path: &Path) -> Result<String> {
    file_digest_progress(path, |_| Ok(()))
}
fn file_digest_progress(
    path: &Path,
    mut progress: impl FnMut(u64) -> Result<()>,
) -> Result<String> {
    let mut f = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut b = [0u8; 65536];
    let mut verified = 0;
    progress(verified)?;
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        hash.update(&b[..n]);
        verified += n as u64;
        progress(verified)?;
    }
    Ok(hex::encode(hash.finalize()))
}
fn verify_archive(
    archive: &Path,
    release: &Release,
    cancel: &crate::jobs::Cancellation,
    mut progress: impl FnMut(Value) -> Result<()>,
) -> Result<()> {
    let mut last_update = std::time::Instant::now();
    let digest = file_digest_progress(archive, |verified| {
        cancel.check()?;
        ensure!(verified <= release.size, "发行包超过声明长度");
        if verified == 0
            || verified == release.size
            || last_update.elapsed() >= Duration::from_millis(250)
        {
            progress(
                json!({"phase":"verifying","verified":verified,"total":release.size,"version":release.version}),
            )?;
            last_update = std::time::Instant::now();
        }
        Ok(())
    })?;
    cancel.check()?;
    ensure!(fs::metadata(archive)?.len() == release.size, "下载未完成");
    ensure!(digest == release.sha256, "发行包 SHA256 不匹配");
    Ok(())
}
#[derive(Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}
#[derive(Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    prerelease: bool,
    draft: bool,
    assets: Vec<GitHubAsset>,
}
fn descriptor_version(version: &str) -> Result<semver::Version> {
    // Runtime VERSION appends a twelve-digit content hash to the Cargo version.
    let version = version
        .rsplit_once('-')
        .filter(|(_, hash)| hash.len() == 12 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
        .map_or(version, |(version, _)| version);
    Ok(semver::Version::parse(version)?)
}
type Resolved = (Release, Vec<u8>, String);
fn resolve_release(
    source: &UpdateSource,
    channel: UpdateChannel,
    cancel: &crate::jobs::Cancellation,
    fetch: &mut impl FnMut(&str, usize) -> Result<Vec<u8>>,
) -> Result<Option<Resolved>> {
    if let Some(repo) = source.github_repository() {
        let mut candidates = Vec::new();
        for page in 1..=10 {
            cancel.check()?;
            let bytes = fetch(
                &format!("https://api.github.com/repos/{repo}/releases?per_page=100&page={page}"),
                4 * 1024 * 1024,
            )?;
            let releases: Vec<GitHubRelease> = serde_json::from_slice(&bytes)?;
            let finished = releases.len() < 100;
            for release in releases {
                let Some(tag) = release.tag_name.strip_prefix('v') else {
                    continue;
                };
                let Ok(version) = semver::Version::parse(tag) else {
                    continue;
                };
                let testing = channel == UpdateChannel::Testing;
                if release.draft
                    || release.prerelease != testing
                    || version.pre.is_empty() == testing
                {
                    continue;
                }
                let descriptors: Vec<_> = release
                    .assets
                    .iter()
                    .filter(|a| a.name == "framely-release.json")
                    .collect();
                let archives: Vec<_> = release
                    .assets
                    .iter()
                    .filter(|a| {
                        a.name
                            .strip_prefix("framely-")
                            .and_then(|n| n.strip_suffix("-linux-arm64.tar.gz"))
                            .is_some_and(|n| descriptor_version(n).is_ok_and(|v| v == version))
                    })
                    .collect();
                if descriptors.len() == 1 && archives.len() == 1 {
                    candidates.push((version, release));
                }
            }
            if finished {
                break;
            }
            ensure!(page < 10, "发行列表过长，请配置明确的更新清单地址");
        }
        candidates.sort_by(|a, b| b.0.cmp_precedence(&a.0));
        let Some((version, selected)) = candidates.into_iter().next() else {
            return Ok(None);
        };
        let descriptor = selected
            .assets
            .iter()
            .find(|a| a.name == "framely-release.json")
            .unwrap();
        let bytes = fetch(&descriptor.browser_download_url, 256 * 1024)?;
        let release = verify_descriptor(&bytes)?;
        ensure!(
            descriptor_version(&release.version)? == version,
            "更新清单与 Release 版本不一致"
        );
        let name = format!("framely-{}-linux-arm64.tar.gz", release.version);
        ensure!(
            selected.assets.iter().any(|a| a.name == name
                && a.browser_download_url == release.url
                && a.size == release.size),
            "更新清单与 Release 附件不一致"
        );
        Ok(Some((release, bytes, version.to_string())))
    } else {
        cancel.check()?;
        let bytes = fetch(&source.url, 256 * 1024)?;
        let release = verify_descriptor(&bytes)?;
        let version = descriptor_version(&release.version)?;
        ensure!(
            version.pre.is_empty() == (channel == UpdateChannel::Stable),
            "更新清单不属于所选渠道，请配置该渠道的清单地址"
        );
        Ok(Some((release, bytes, version.to_string())))
    }
}
type CheckedSource = (UpdateSource, UpdateChannel);
type Candidate = (Release, Vec<u8>, CheckedSource);
type Prepared = (PathBuf, Release, CheckedSource);
#[derive(Clone, Default)]
pub struct Updater {
    pub jobs: Jobs,
    candidate: Arc<Mutex<Option<Candidate>>>,
    prepared: Arc<Mutex<Option<Prepared>>>,
    busy: Arc<AtomicBool>,
}
struct Busy(Arc<AtomicBool>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
impl Updater {
    fn acquire(&self) -> Result<Busy> {
        ensure!(
            !self.busy.swap(true, Ordering::SeqCst),
            "已有更新任务正在运行"
        );
        Ok(Busy(self.busy.clone()))
    }
    pub fn ensure_idle(&self) -> Result<()> {
        ensure!(!self.busy.load(Ordering::SeqCst), "请等待更新任务完成");
        Ok(())
    }
    pub fn check(
        &self,
        source: UpdateSource,
        channel: UpdateChannel,
        proxy: crate::model::ProxySettings,
    ) -> Result<Value> {
        source.validate()?;
        let busy = self.acquire()?;
        self.clear();
        let updater = self.clone();
        self.jobs.task("system.check", move |cancel| {
            let _busy = busy;
            cancel.check()?;
            let result = resolve_release(&source, channel, &cancel, &mut |url, limit| {
                crate::model::validate_url(url, false)?;
                let response = crate::http::get_with_proxy_cancel(
                    url,
                    false,
                    Duration::from_secs(15),
                    &[
                        ("User-Agent", "Framely"),
                        ("Accept", "application/vnd.github+json"),
                    ],
                    &proxy,
                    &cancel,
                )?;
                crate::http::read_cancel(response, limit, &cancel)
            })?;
            if let Some((release, bytes, version)) = result {
                let info = json!({"release":release,"version":version,"channel":channel});
                cancel.commit(|| {
                    *updater.candidate.lock().unwrap() = Some((release, bytes, (source, channel)));
                    Ok(())
                })?;
                Ok(info)
            } else {
                Ok(json!({"release":null,"channel":channel}))
            }
        })
    }
    pub fn clear(&self) {
        *self.candidate.lock().unwrap() = None;
        if let Some((path, _, _)) = self.prepared.lock().unwrap().take() {
            let _ = fs::remove_dir_all(path);
        }
    }
    pub fn prepare(&self, root: PathBuf, proxy: crate::model::ProxySettings) -> Result<Value> {
        let busy = self.acquire()?;
        let (release, descriptor, source_url) = self
            .candidate
            .lock()
            .unwrap()
            .clone()
            .context("请先检查更新")?;
        let updater = self.clone();
        let state_path = root.join("update-status.json");
        self.jobs.task_progress("system.download", move |cancel, progress| {
            let _busy = busy;
            cancel.check()?;
            let cache = PathBuf::from("/home/.framely/update-cache");
            fs::create_dir_all(&cache)?;
            let metadata = fs::symlink_metadata(&cache)?;
            ensure!(metadata.is_dir() && !metadata.file_type().is_symlink() && metadata.uid() == 0, "更新缓存权限无效");
            fs::set_permissions(&cache, fs::Permissions::from_mode(0o700))?;
            let stage = cache.join(format!("{:032x}", rand::random::<u128>()));
            fs::create_dir(&stage)?;
            fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))?;
            let report = |value: Value| -> Result<()> {
                write_status(&state_path, value.clone())?;
                progress(value);
                Ok(())
            };
            let result = (|| -> Result<Value> {
                report(json!({"phase":"downloading","received":0,"total":release.size,"version":release.version}))?;
                let response = crate::http::get_with_proxy(&release.url, false, Duration::from_secs(120), &[], &proxy)?;
                if let Some(length) = response.header("Content-Length") {
                    ensure!(length.parse::<u64>()? == release.size, "下载长度与更新清单不一致");
                }
                let mut input = response.into_reader();
                let archive = stage.join("release.tar.gz");
                let mut output = fs::File::create(&archive)?;
                let mut buffer = [0u8; 262144];
                let mut received = 0u64;
                let mut update_at = std::time::Instant::now();
                loop {
                    cancel.check()?;
                    let n = input.read(&mut buffer)?;
                    if n == 0 { break; }
                    received += n as u64;
                    ensure!(received <= release.size, "发行包超过声明长度");
                    output.write_all(&buffer[..n])?;
                    if received == release.size || update_at.elapsed() >= Duration::from_millis(250) {
                        report(json!({"phase":"downloading","received":received,"total":release.size,"version":release.version}))?;
                        update_at = std::time::Instant::now();
                    }
                }
                output.sync_all()?;
                drop(output);
                ensure!(received == release.size, "下载未完成");
                verify_archive(&archive, &release, &cancel, report)?;
                fs::write(stage.join("descriptor.json"), descriptor)?;
                cancel.commit(|| {
                    report(json!({"phase":"ready","version":release.version,"received":received,"verified":received,"total":release.size}))?;
                    *updater.prepared.lock().unwrap() = Some((stage.clone(), release.clone(), source_url));
                    Ok(())
                })?;
                Ok(json!(release))
            })();
            if let Err(e) = &result {
                let _ = fs::remove_dir_all(stage);
                let _ = write_status(&state_path, json!({"phase":"failed","error":e.to_string()}));
            }
            result
        })
    }

    pub fn apply(
        &self,
        root: &Path,
        manager: u32,
        source: &UpdateSource,
        channel: UpdateChannel,
        request: Value,
    ) -> Result<Value> {
        let _busy = self.acquire()?;
        ensure!(
            request["approve"].as_bool() == Some(true),
            "请确认升级，Framely 界面会暂时关闭"
        );
        let (stage, release, source_url) = self
            .prepared
            .lock()
            .unwrap()
            .clone()
            .context("请先下载并校验发行包")?;
        ensure!(
            source_url == (source.clone(), channel),
            "更新源或渠道已变化，请重新检查并下载"
        );
        ensure!(
            request["version"].as_str() == Some(&release.version),
            "待安装版本已变，请重新确认"
        );
        launch_helper(root, manager, Some(&stage))?;
        *self.prepared.lock().unwrap() = None;
        write_status(
            &root.join("update-status.json"),
            json!({"phase":"installing","version":release.version}),
        )?;
        Ok(json!(true))
    }
    pub fn rollback(root: &Path, manager: u32, request: Value) -> Result<Value> {
        ensure!(request["approve"].as_bool() == Some(true), "请确认回滚");
        let previous = fs::read_to_string(root.join("previous-release"))?;
        ensure!(
            previous.trim().starts_with("releases/")
                && !previous.contains("..")
                && root.join(previous.trim()).is_dir(),
            "没有可用的上一版本"
        );
        launch_helper(root, manager, None)?;
        write_status(
            &root.join("update-status.json"),
            json!({"phase":"installing","rollback":true}),
        )?;
        Ok(json!(true))
    }
}
fn launch_helper(root: &Path, manager: u32, stage: Option<&Path>) -> Result<()> {
    let mut command = Command::new("systemd-run");
    command.args([
        "--quiet",
        "--collect",
        "--no-block",
        "--property=Type=exec",
        "--property=TimeoutStartSec=15min",
    ]);
    command.arg(format!(
        "--unit=framely-update-{:016x}",
        rand::random::<u64>()
    ));
    command
        .arg(root.join("current/bin/framely"))
        .args(["apply-update", "--state"])
        .arg(root)
        .arg("--manager-uid")
        .arg(manager.to_string());
    if let Some(stage) = stage {
        command.arg("--stage").arg(stage);
    } else {
        command.arg("--rollback");
    }
    ensure!(command.status()?.success(), "无法启动更新服务");
    Ok(())
}
fn write_status(path: &Path, value: Value) -> Result<()> {
    let tmp = path.with_extension(format!("json.{:032x}.tmp", rand::random::<u128>()));
    fs::write(&tmp, serde_json::to_vec(&value)?)?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644))?;
    fs::rename(tmp, path)?;
    Ok(())
}
pub fn apply(root: &Path, manager: u32, stage: Option<PathBuf>, rollback: bool) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "更新助手必须由核心服务启动"
    );
    std::thread::sleep(Duration::from_secs(2));
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("logs/update.log"))?;
    let result = (|| -> Result<()> {
        let status = if rollback {
            Command::new("bash")
                .arg(root.join("current/rollback.sh"))
                .stdout(log.try_clone()?)
                .stderr(log.try_clone()?)
                .status()?
        } else {
            let stage = stage.context("缺少发行包路径")?;
            ensure!(
                stage.parent() == Some(Path::new("/home/.framely/update-cache")),
                "无效更新目录"
            );
            let metadata = fs::symlink_metadata(&stage)?;
            ensure!(
                metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && metadata.uid() == 0
                    && metadata.mode() & 0o022 == 0,
                "更新目录权限无效"
            );
            let release = verify_descriptor(&fs::read(stage.join("descriptor.json"))?)?;
            ensure!(
                file_digest(&stage.join("release.tar.gz"))? == release.sha256,
                "发行包在校验后发生变化"
            );
            let extract = root.join("current/tools/extract-release.py");
            ensure!(
                Command::new("python3")
                    .arg(extract)
                    .arg(stage.join("release.tar.gz"))
                    .arg(stage.join("unpacked"))
                    .arg(&release.version)
                    .stdout(log.try_clone()?)
                    .stderr(log.try_clone()?)
                    .status()?
                    .success(),
                "发行包解压检查失败"
            );
            let name = Command::new("id")
                .args(["-nu", &manager.to_string()])
                .output()?;
            ensure!(name.status.success(), "Steam 用户不存在");
            let base = stage
                .join("unpacked")
                .join(format!("framely-{}", release.version));
            ensure!(
                fs::read_to_string(base.join("VERSION"))?.trim() == release.version,
                "发行包版本不一致"
            );
            Command::new("bash")
                .arg(base.join("install.sh"))
                .arg(std::str::from_utf8(&name.stdout)?.trim())
                .stdout(log.try_clone()?)
                .stderr(log.try_clone()?)
                .status()?
        };
        ensure!(
            status.success(),
            "升级失败；安装程序已尝试恢复原版本，请查看更新日志"
        );
        Ok(())
    })();
    write_status(
        &root.join("update-status.json"),
        match &result {
            Ok(()) => json!({"phase":"done"}),
            Err(e) => json!({"phase":"failed","error":e.to_string()}),
        },
    )?;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_verification_reports_bytes_and_rejects_tampering_or_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("release.tar.gz");
        let payload = vec![42u8; 150_000];
        fs::write(&archive, &payload).unwrap();
        let release = Release {
            schema_version: 1,
            version: "0.4.2".into(),
            api_version: API_VERSION,
            arch: "aarch64".into(),
            url: "https://example.org/release.tar.gz".into(),
            sha256: hex::encode(Sha256::digest(&payload)),
            size: payload.len() as u64,
            changelog: String::new(),
        };
        let mut events = Vec::new();
        verify_archive(&archive, &release, &Default::default(), |event| {
            events.push(event);
            Ok(())
        })
        .unwrap();
        assert_eq!(events.first().unwrap()["verified"], 0);
        assert_eq!(events.last().unwrap()["verified"], release.size);
        assert!(events
            .iter()
            .all(|event| event["phase"] == "verifying" && event["total"] == release.size));
        let cancel = crate::jobs::Cancellation::default();
        assert!(verify_archive(&archive, &release, &cancel, |event| {
            if event["verified"] == release.size {
                cancel.stop();
            }
            Ok(())
        })
        .unwrap_err()
        .to_string()
        .contains("取消"));
        fs::write(&archive, vec![43u8; payload.len()]).unwrap();
        assert!(
            verify_archive(&archive, &release, &Default::default(), |_| Ok(()))
                .unwrap_err()
                .to_string()
                .contains("SHA256")
        );
        fs::write(&archive, b"truncated").unwrap();
        assert!(
            verify_archive(&archive, &release, &Default::default(), |_| Ok(()))
                .unwrap_err()
                .to_string()
                .contains("下载未完成")
        );
    }
    fn source() -> UpdateSource {
        UpdateSource {
            url: "https://github.com/example/framely/releases/latest/download/framely-release.json"
                .into(),
        }
    }
    fn fixture(version: &str, draft: bool) -> (Value, Vec<u8>) {
        let build = format!("{version}-012345abcdef");
        let base = format!("https://github.com/example/framely/releases/download/v{version}");
        let archive = format!("framely-{build}-linux-arm64.tar.gz");
        let descriptor = serde_json::to_vec(&json!({"schemaVersion":1,"version":build,"apiVersion":1,"arch":"aarch64","url":format!("{base}/{archive}"),"sha256":"a".repeat(64),"size":123,"changelog":"Notes"})).unwrap();
        (
            json!({"tag_name":format!("v{version}"),"draft":draft,"prerelease":!semver::Version::parse(version).unwrap().pre.is_empty(),"assets":[{"name":"framely-release.json","browser_download_url":format!("{base}/framely-release.json"),"size":descriptor.len()},{"name":archive,"browser_download_url":format!("{base}/{archive}"),"size":123}]}),
            descriptor,
        )
    }
    fn resolve_fixture(
        channel: UpdateChannel,
        releases: Vec<(Value, Vec<u8>)>,
    ) -> Result<Option<Resolved>> {
        let list = serde_json::to_vec(&releases.iter().map(|r| &r.0).collect::<Vec<_>>()).unwrap();
        resolve_release(&source(), channel, &Default::default(), &mut |url, _| {
            if url.contains("/releases?per_page=100&page=1") {
                return Ok(list.clone());
            }
            releases
                .iter()
                .find(|r| r.0["assets"][0]["browser_download_url"] == url)
                .map(|r| r.1.clone())
                .context("unexpected request")
        })
    }
    #[test]
    fn github_channels_select_semver_and_skip_other_products_and_incomplete_releases() {
        let mut installer = fixture("9.0.0", false);
        installer.0["tag_name"] = json!("installer-v9.0.0");
        let mut incomplete = fixture("8.0.0", false);
        incomplete.0["assets"] = json!([]);
        let releases = vec![
            fixture("0.4.2-preview.9", false),
            fixture("0.4.2-preview.10", false),
            fixture("0.4.1", false),
            fixture("0.5.0", true),
            installer,
            incomplete,
            fixture("0.4.2", false),
        ];
        let stable = resolve_fixture(UpdateChannel::Stable, releases.clone())
            .unwrap()
            .unwrap();
        assert_eq!(stable.2, "0.4.2");
        assert_eq!(stable.0.version, "0.4.2-012345abcdef");
        let testing = resolve_fixture(UpdateChannel::Testing, releases)
            .unwrap()
            .unwrap();
        assert_eq!(testing.2, "0.4.2-preview.10");
        assert!(resolve_fixture(
            UpdateChannel::Stable,
            vec![fixture("0.4.2-preview.1", false)]
        )
        .unwrap()
        .is_none());
        assert!(
            resolve_fixture(UpdateChannel::Testing, vec![fixture("0.4.2", false)])
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn github_channel_lookup_paginates_and_handles_cancellation() {
        let (release, bytes) = fixture("0.4.2-rc.1", false);
        let ignored =
            json!({"tag_name":"installer-v1.0.0","draft":false,"prerelease":false,"assets":[]});
        let mut requests = Vec::new();
        let result = resolve_release(
            &source(),
            UpdateChannel::Testing,
            &Default::default(),
            &mut |url, _| {
                requests.push(url.to_owned());
                if url.ends_with("page=1") {
                    Ok(serde_json::to_vec(&vec![ignored.clone(); 100]).unwrap())
                } else if url.ends_with("page=2") {
                    Ok(serde_json::to_vec(&vec![release.clone()]).unwrap())
                } else {
                    Ok(bytes.clone())
                }
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.2, "0.4.2-rc.1");
        assert_eq!(requests.len(), 3);
        let cancel = crate::jobs::Cancellation::default();
        cancel.stop();
        assert!(resolve_release(
            &source(),
            UpdateChannel::Testing,
            &cancel,
            &mut |_, _| panic!("cancelled request")
        )
        .is_err());
    }
    #[test]
    fn github_descriptor_must_match_selected_tag_archive_and_size() {
        for field in ["version", "url", "size"] {
            let (release, bytes) = fixture("0.4.2-preview.1", false);
            let mut descriptor: Value = serde_json::from_slice(&bytes).unwrap();
            descriptor[field] = match field {
                "version" => json!("0.4.2-preview.2-012345abcdef"),
                "url" => json!("https://example.org/unrelated.tar.gz"),
                _ => json!(124),
            };
            assert!(
                resolve_fixture(
                    UpdateChannel::Testing,
                    vec![(release, serde_json::to_vec(&descriptor).unwrap())]
                )
                .is_err(),
                "{field}"
            );
        }
    }
    #[test]
    fn split_release_always_resolves_the_core_archive() {
        let mut item = fixture("0.4.2-preview.1", false);
        item.0["assets"].as_array_mut().unwrap().extend([
            json!({"name":"framely-0.4.2-preview.1-012345abcdef-offline-linux-arm64.tar.gz","browser_download_url":"https://example.org/offline","size":900}),
            json!({"name":"framely-cef-154-build-linux-arm64.tar.gz","browser_download_url":"https://example.org/cef","size":800}),
        ]);
        let resolved = resolve_fixture(UpdateChannel::Testing, vec![item])
            .unwrap()
            .unwrap();
        assert!(resolved
            .0
            .url
            .ends_with("framely-0.4.2-preview.1-012345abcdef-linux-arm64.tar.gz"));
    }
    #[test]
    fn custom_descriptors_distinguish_channels_and_ignore_packaging_hashes() {
        for (version, channel) in [
            ("0.4.2", UpdateChannel::Stable),
            ("0.4.2-beta.1", UpdateChannel::Testing),
        ] {
            let (_, bytes) = fixture(version, false);
            let mut source = source();
            source.url = "https://example.org/release.json".into();
            assert_eq!(
                resolve_release(&source, channel, &Default::default(), &mut |url, _| {
                    assert_eq!(url, source.url);
                    Ok(bytes.clone())
                })
                .unwrap()
                .unwrap()
                .2,
                version
            );
            let other_channel = if channel == UpdateChannel::Stable {
                UpdateChannel::Testing
            } else {
                UpdateChannel::Stable
            };
            assert!(
                resolve_release(&source, other_channel, &Default::default(), &mut |_, _| Ok(
                    bytes.clone()
                ))
                .is_err()
            );
        }
    }
    #[test]
    fn changing_channel_invalidates_a_downloaded_release() {
        let dir = tempfile::tempdir().unwrap();
        let updater = Updater::default();
        let (_, bytes) = fixture("0.4.2-preview.1", false);
        let release = verify_descriptor(&bytes).unwrap();
        *updater.prepared.lock().unwrap() = Some((
            dir.path().into(),
            release.clone(),
            (source(), UpdateChannel::Testing),
        ));
        let error = updater
            .apply(
                dir.path(),
                1000,
                &source(),
                UpdateChannel::Stable,
                json!({"approve":true,"version":release.version}),
            )
            .unwrap_err();
        assert!(error.to_string().contains("渠道已变化"));
    }
    #[test]
    fn unsigned_release_manifest_validates_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("framely-0.2.0-test-linux-arm64.tar.gz");
        fs::write(&archive, b"payload").unwrap();
        let output = dir.path().join("release.json");
        create_descriptor(
            &archive,
            "https://example.org/release.tar.gz",
            &output,
            "Notes",
        )
        .unwrap();
        let bytes = fs::read(output).unwrap();
        let release = verify_descriptor(&bytes).unwrap();
        assert_eq!(release.version, "0.2.0-test");
        assert_eq!(release.sha256, crate::package::digest(b"payload"));
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("signature").is_none());
        assert!(verify_descriptor(br#"{"manifest":"encoded","signature":"old"}"#).is_err());
        value["size"] = json!(0);
        assert!(verify_descriptor(&serde_json::to_vec(&value).unwrap()).is_err());
        value["size"] = json!(7);
        value["sha256"] = json!("invalid");
        assert!(verify_descriptor(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
