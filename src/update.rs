use crate::{
    jobs::Jobs,
    model::{UpdateSource, API_VERSION},
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
    let mut f = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut b = [0u8; 65536];
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        hash.update(&b[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}
type Candidate = (Release, Vec<u8>, String);
type Prepared = (PathBuf, Release, String);
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
    pub fn check(&self, source: UpdateSource, proxy: crate::model::ProxySettings) -> Result<Value> {
        source.validate()?;
        let busy = self.acquire()?;
        let updater = self.clone();
        self.jobs.task("system.check", move |cancel| {
            let _busy = busy;
            cancel.check()?;
            let response = crate::http::get_with_proxy(
                &source.url,
                false,
                Duration::from_secs(15),
                &[],
                &proxy,
            )?;
            let mut bytes = Vec::new();
            response
                .into_reader()
                .take(256 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            let release = verify_descriptor(&bytes)?;
            let info = json!({"release":release});
            cancel.commit(|| {
                *updater.candidate.lock().unwrap() = Some((release, bytes, source.url));
                Ok(())
            })?;
            if let Some((path, _, _)) = updater.prepared.lock().unwrap().take() {
                let _ = fs::remove_dir_all(path);
            }
            Ok(info)
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
        self.jobs.task("system.download",move |cancel| {
            let _busy=busy;cancel.check()?;
            let cache=PathBuf::from("/home/.framely/update-cache");fs::create_dir_all(&cache)?;let metadata=fs::symlink_metadata(&cache)?;ensure!(metadata.is_dir()&&!metadata.file_type().is_symlink()&&metadata.uid()==0,"更新缓存权限无效");fs::set_permissions(&cache,fs::Permissions::from_mode(0o700))?;
            let stage=cache.join(format!("{:032x}",rand::random::<u128>()));fs::create_dir(&stage)?;fs::set_permissions(&stage,fs::Permissions::from_mode(0o700))?;
            let result=(||->Result<Value>{
                write_status(&state_path,json!({"phase":"downloading","received":0,"total":release.size,"version":release.version}))?;
                let response=crate::http::get_with_proxy(&release.url, false, Duration::from_secs(120), &[], &proxy)?;
                if let Some(length)=response.header("Content-Length"){ensure!(length.parse::<u64>()?==release.size,"下载长度与更新清单不一致");}
                let mut input=response.into_reader();let mut output=fs::File::create(stage.join("release.tar.gz"))?;let mut buffer=[0u8;262144];let mut received=0u64;let mut update_at=std::time::Instant::now();
                loop{cancel.check()?;let n=input.read(&mut buffer)?;if n==0{break;}received+=n as u64;ensure!(received<=release.size,"发行包超过声明长度");output.write_all(&buffer[..n])?;
                    if update_at.elapsed()>=Duration::from_millis(250){write_status(&state_path,json!({"phase":"downloading","received":received,"total":release.size,"version":release.version}))?;update_at=std::time::Instant::now();}}
                output.sync_all()?;ensure!(received==release.size,"下载未完成");ensure!(file_digest(&stage.join("release.tar.gz"))?==release.sha256,"发行包 SHA256 不匹配");
                fs::write(stage.join("descriptor.json"),descriptor)?;
                cancel.commit(|| { *updater.prepared.lock().unwrap()=Some((stage.clone(),release.clone(),source_url));Ok(()) })?;
                write_status(&state_path,json!({"phase":"ready","version":release.version,"received":received,"total":release.size}))?;
                Ok(json!(release))
            })();
            if let Err(e)=&result {let _=fs::remove_dir_all(stage);let _=write_status(&state_path,json!({"phase":"failed","error":e.to_string()}));}
            result
        })
    }
    pub fn apply(
        &self,
        root: &Path,
        manager: u32,
        source: &UpdateSource,
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
        ensure!(source_url == source.url, "更新源已变化，请重新检查并下载");
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
