use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ssh2::{ExtendedData, Session};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Credentials {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Zeroizing<String>,
}
#[derive(Clone, Debug)]
pub struct Probe {
    pub fingerprint: String,
    pub known: bool,
}
pub struct Connection {
    pub session: Session,
    pub host: String,
    pub user: String,
    pub version: String,
    pub installation: crate::maintenance::Installation,
}
#[derive(Default, Serialize, Deserialize)]
struct Hosts {
    keys: BTreeMap<String, String>,
}
fn hosts_file() -> Result<PathBuf> {
    Ok(ProjectDirs::from("org", "Framely", "Installer")
        .context("无法确定配置目录")?
        .config_dir()
        .join("known-hosts.json"))
}
fn load_hosts() -> Result<Hosts> {
    let path = hosts_file()?;
    if path.exists() {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    } else {
        Ok(Hosts::default())
    }
}
fn handshake(host: &str, port: u16) -> Result<Session> {
    ensure!(
        !host.is_empty() && !host.contains(['\n', '\r']),
        "设备地址无效"
    );
    let addresses: Vec<_> = (host, port).to_socket_addrs()?.collect();
    let stream = addresses
        .iter()
        .find_map(|a| TcpStream::connect_timeout(a, Duration::from_secs(3)).ok())
        .context("SSH 无法连接，请检查 IP 和开发者模式")?;
    let mut session = Session::new()?;
    session.set_tcp_stream(stream);
    session.set_timeout(10000);
    session.handshake()?;
    Ok(session)
}
fn fingerprint(session: &Session) -> Result<String> {
    let (key, _) = session.host_key().context("SSH 主机未提供公钥")?;
    Ok(format!(
        "SHA256:{}",
        STANDARD_NO_PAD.encode(Sha256::digest(key))
    ))
}
fn normalized_saved_fingerprint(saved: &str) -> String {
    if let Some(hex_digest) = saved.strip_prefix("SHA256:")
        && hex_digest.len() == 64
        && let Ok(bytes) = hex::decode(hex_digest)
    {
        return format!("SHA256:{}", STANDARD_NO_PAD.encode(bytes));
    }
    saved.to_owned()
}
pub fn probe(host: &str, port: u16) -> Result<Probe> {
    let session = handshake(host, port)?;
    let fingerprint = fingerprint(&session)?;
    let hosts = load_hosts()?;
    let endpoint = format!("{host}:{port}");
    if let Some(saved) = hosts.keys.get(&endpoint) {
        ensure!(
            normalized_saved_fingerprint(saved) == fingerprint,
            "SSH 主机指纹发生变化，已阻止连接；请先核实设备身份"
        );
    }
    Ok(Probe {
        fingerprint,
        known: hosts.keys.contains_key(&endpoint),
    })
}
pub fn connect(
    credentials: Credentials,
    expected: &str,
    progress: &mut dyn FnMut(String),
) -> Result<Connection> {
    ensure!(
        credentials.user != "root"
            && !credentials.user.is_empty()
            && credentials
                .user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "Steam 用户名无效"
    );
    ensure!(
        !credentials.password.contains(['\n', '\r']),
        "密码不能包含换行符"
    );
    ensure!(!credentials.password.is_empty(), "请输入设备登录密码");
    progress("正在建立 SSH 连接…".into());
    let session = handshake(&credentials.host, credentials.port).context("无法建立 SSH 连接")?;
    ensure!(
        fingerprint(&session)? == expected,
        "SSH 指纹与刚才检查的不一致"
    );
    progress("正在验证用户名和密码…".into());
    session
        .userauth_password(&credentials.user, &credentials.password)
        .context("SSH 登录失败：请检查用户名和密码，并确认已开启开发者模式")?;
    ensure!(session.authenticated(), "SSH 登录失败");
    progress("正在检查设备型号…".into());
    let identity = exec(
        &session,
        "printf '%s\\n' \"$(hostname)\" \"$(uname -s)\" \"$(uname -m)\"",
        None,
        &mut |_| {},
    )?;
    let fields: Vec<_> = identity.lines().collect();
    ensure!(
        fields.len() >= 3 && fields[1] == "Linux" && fields[2] == "aarch64",
        "目标设备不是 Linux ARM64 Steam Frame"
    );
    progress("正在检查管理员权限…".into());
    // Check sudo credentials without echoing the password or putting it in arguments.
    exec(
        &session,
        "sudo -S -p '' -k -v",
        Some(&credentials.password),
        &mut |_| {},
    )
    .context("SSH 已登录，但管理员权限验证失败：请检查此账号的 sudo 权限及密码")?;
    progress("正在读取已安装版本…".into());
    let installation = read_installation(&session, &credentials.password)?;
    let version = installation
        .current_version
        .as_deref()
        .unwrap_or(if installation.present {
            "安装需要修复"
        } else {
            "未安装"
        });
    progress("正在保存设备指纹…".into());
    let path = hosts_file()?;
    fs::create_dir_all(path.parent().unwrap())
        .context("无法创建指纹配置目录，请检查本机目录权限")?;
    let mut hosts = load_hosts()?;
    hosts.keys.insert(
        format!("{}:{}", credentials.host, credentials.port),
        expected.into(),
    );
    fs::write(path, serde_json::to_vec_pretty(&hosts)?)
        .context("无法保存设备指纹，请检查本机配置目录权限")?;
    Ok(Connection {
        session,
        host: credentials.host,
        user: credentials.user,
        version: format!("{} · {}", fields[0], version.trim()),
        installation,
    })
}
fn read_installation(
    session: &Session,
    password: &str,
) -> Result<crate::maintenance::Installation> {
    let script = r#"import json, os, pathlib
root = pathlib.Path('/home/.framely/state')
if not root.is_dir(): root = pathlib.Path('/var/lib/framely')
def version(name):
    file = root / name / 'VERSION'
    return file.read_text().strip() or None if file.is_file() else None
print(json.dumps({'present': os.path.lexists(root / 'current'), 'currentVersion': version('current'), 'previousVersion': version('previous')}))
"#;
    let output = exec(
        session,
        &format!("sudo -S -p '' -- python3 -c {}", quote(script)),
        Some(password),
        &mut |_| {},
    )
    .context("SSH 已登录，但无法读取安装状态")?;
    serde_json::from_str(output.trim()).context("设备安装状态无效")
}
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn exec(
    session: &Session,
    command: &str,
    password: Option<&str>,
    log: &mut dyn FnMut(String),
) -> Result<String> {
    exec_with_progress(session, command, password, log, None)
}
fn exec_with_progress(
    session: &Session,
    command: &str,
    password: Option<&str>,
    log: &mut dyn FnMut(String),
    mut progress: Option<&mut dyn FnMut(crate::progress::Progress)>,
) -> Result<String> {
    let mut channel = session.channel_session()?;
    channel.handle_extended_data(ExtendedData::Merge)?;
    channel.exec(command)?;
    if let Some(password) = password {
        channel.write_all(password.as_bytes())?;
        channel.write_all(b"\n")?;
        channel.flush()?;
    }
    channel.send_eof()?;
    let mut result = String::new();
    let mut chunk = [0; 4096];
    let mut decoder = crate::progress::DeviceProgress::default();
    loop {
        let n = channel.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&chunk[..n]).to_string();
        // A remote process must never cause credentials to appear in UI logs.
        let line = password
            .filter(|s| !s.is_empty())
            .map_or(line.clone(), |p| line.replace(p, "[密码已隐藏]"));
        let line = if let Some(report) = progress.as_deref_mut() {
            decoder.push(&line, report).join("")
        } else {
            line
        };
        if !line.is_empty() {
            log(line.clone());
        }
        if result.len() < 128 * 1024 {
            result.push_str(&line);
        }
    }
    if progress.is_some() {
        let tail = decoder.finish();
        if !tail.is_empty() {
            log(tail.clone());
            result.push_str(&tail);
        }
    }
    channel.wait_close()?;
    ensure!(
        channel.exit_status()? == 0,
        "远程操作失败：{}",
        result.trim()
    );
    Ok(result)
}
fn upload(
    session: &Session,
    path: &Path,
    remote: &str,
    offset: u64,
    total: u64,
    progress: &mut dyn FnMut(crate::progress::Progress),
) -> Result<()> {
    let size = path.metadata()?.len();
    let sftp = session.sftp()?;
    let mut output = sftp.create(Path::new(remote))?;
    let mut input = fs::File::open(path)?;
    crate::progress::copy(
        &mut input,
        &mut output,
        size,
        crate::progress::Stage::Transfer,
        &path.file_name().unwrap_or_default().to_string_lossy(),
        &mut |mut value| {
            value.completed += offset;
            value.total = Some(total);
            progress(value);
        },
    )?;
    output.close()?;
    Ok(())
}
pub struct Operation<'a> {
    pub action: &'a str,
    pub archive: Option<&'a Path>,
    pub sums: Option<&'a Path>,
    pub repo: &'a str,
}
pub fn operate(
    connection: &Connection,
    password: Zeroizing<String>,
    operation: Operation<'_>,
    log: &mut dyn FnMut(String),
    progress: &mut dyn FnMut(crate::progress::Progress),
) -> Result<()> {
    let Operation {
        action,
        archive,
        sums,
        repo,
    } = operation;
    ensure!(
        ["install", "update", "repair", "rollback", "uninstall"].contains(&action),
        "未知操作"
    );
    super::release::validate_repo(repo)?;
    let session = &connection.session;
    session.set_timeout(15 * 60 * 1000);
    let installation = read_installation(session, &password)?;
    ensure!(
        installation.allows(action),
        "设备安装状态已变化，请重新连接并选择可用操作"
    );
    if action == "update" {
        if let Some(name) = archive.and_then(|p| p.file_name()).and_then(|s| s.to_str()) {
            ensure!(
                !installation.same_package(name),
                "该版本已安装，请使用修复安装"
            );
        }
    }
    let dir = exec(
        session,
        "umask 077; mktemp -d /home/$(id -un)/.framely-installer-XXXXXXXX",
        None,
        log,
    )?
    .trim()
    .to_owned();
    ensure!(
        dir.starts_with(&format!("/home/{}/.framely-installer-", connection.user))
            && !dir.contains(['\n', '\r']),
        "临时目录无效"
    );
    let result = (|| -> Result<()> {
        let total = super::BOOTSTRAP.len() as u64
            + archive
                .map(|p| p.metadata().map(|m| m.len()))
                .transpose()?
                .unwrap_or(0)
            + sums
                .map(|p| p.metadata().map(|m| m.len()))
                .transpose()?
                .unwrap_or(0);
        let mut transfer =
            crate::progress::Progress::new(crate::progress::Stage::Transfer, "传输安装引擎");
        transfer.total = Some(total);
        progress(transfer.clone());
        let sftp = session.sftp()?;
        let script = format!("{dir}/bootstrap.py");
        let mut file = sftp.create(Path::new(&script))?;
        file.write_all(super::BOOTSTRAP)?;
        file.close()?;
        transfer.completed = super::BOOTSTRAP.len() as u64;
        progress(transfer);
        let mut args = format!(
            "sudo -S -p '' -k -- python3 -u {} {} --yes --progress-json --repo {} --user {}",
            quote(&script),
            quote(action),
            quote(repo),
            quote(&connection.user)
        );
        if let (Some(archive), Some(sums)) = (archive, sums) {
            super::release::verify(archive, sums)?;
            let name = archive
                .file_name()
                .and_then(|s| s.to_str())
                .context("压缩包名无效")?;
            let target = format!("{dir}/{name}");
            upload(
                session,
                archive,
                &target,
                super::BOOTSTRAP.len() as u64,
                total,
                progress,
            )?;
            let checksums = format!("{dir}/SHA256SUMS");
            upload(
                session,
                sums,
                &checksums,
                super::BOOTSTRAP.len() as u64 + archive.metadata()?.len(),
                total,
                progress,
            )?;
            args += &format!(
                " --archive {} --checksums {}",
                quote(&target),
                quote(&checksums)
            );
        } else {
            ensure!(
                !matches!(action, "install" | "update"),
                "安装或更新需要已校验的发行包"
            );
        }
        let mut installing =
            crate::progress::Progress::new(crate::progress::Stage::Install, "设备端校验安装包");
        installing.step = Some((1, 5));
        progress(installing);
        exec_with_progress(session, &args, Some(&password), log, Some(progress))?;
        let mut activating = crate::progress::Progress::new(
            crate::progress::Stage::Install,
            "检查系统服务并清理临时文件",
        );
        activating.step = Some((5, 5));
        progress(activating);
        if action != "uninstall" {
            exec(
                session,
                "systemctl is-active framely.service && systemctl is-enabled framely.service framely-session.service",
                None,
                log,
            )?;
        }
        Ok(())
    })();
    // Remove uploaded packages even if hooks or installation failed.
    let cleanup = exec(
        session,
        &format!("rm -rf -- {}", quote(&dir)),
        None,
        &mut |_| {},
    );
    session.set_timeout(10000);
    result?;
    cleanup?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoting_preserves_shell_literals() {
        assert_eq!(quote("a'b;$()"), "'a'\\''b;$()'");
    }
    #[test]
    fn old_saved_fingerprints_remain_comparable() {
        let digest = [0_u8; 32];
        let standard = format!("SHA256:{}", STANDARD_NO_PAD.encode(digest));
        assert_eq!(
            normalized_saved_fingerprint(&format!("SHA256:{}", hex::encode(digest))),
            standard
        );
        assert_eq!(normalized_saved_fingerprint(&standard), standard);
        assert_ne!(
            normalized_saved_fingerprint(&format!("SHA256:{}", hex::encode([1_u8; 32]))),
            standard
        );
    }
    #[test]
    #[ignore = "Requires an explicitly supplied SSH device and trusted OpenSSH fingerprints"]
    fn real_device_fingerprint_matches_openssh() {
        let host = std::env::var("FRAMELY_TEST_HOST").expect("Set FRAMELY_TEST_HOST");
        let expected =
            std::env::var("FRAMELY_TEST_FINGERPRINTS").expect("Set trusted OpenSSH fingerprints");
        let probe = probe(&host, 22).expect("SSH handshake");
        assert!(
            expected
                .split_whitespace()
                .any(|value| value == probe.fingerprint),
            "{}",
            probe.fingerprint
        );
    }
    #[test]
    #[ignore = "Requires an explicitly supplied SSH device; attempts one intentionally invalid login"]
    fn real_device_reports_authentication_failure() {
        let host = std::env::var("FRAMELY_TEST_HOST").expect("Set FRAMELY_TEST_HOST");
        let probe = probe(&host, 22).expect("SSH handshake");
        let mut stages = Vec::new();
        let result = connect(
            Credentials {
                host,
                port: 22,
                user: "steamos".into(),
                password: Zeroizing::new("framely-invalid-password-test-9c45bc39".into()),
            },
            &probe.fingerprint,
            &mut |stage| stages.push(stage),
        );
        let error = match result {
            Ok(_) => panic!("Invalid test password unexpectedly accepted"),
            Err(error) => format!("{error:#}"),
        };
        assert!(error.contains("SSH 登录失败"), "{error}");
        assert!(
            stages
                .iter()
                .any(|stage| stage.contains("验证用户名和密码"))
        );
        eprintln!("stages={stages:?}; error={error}");
    }
}

/// Collect directly over SSH; neither the web panel nor the daemon is required.
pub fn export_logs(
    connection: &Connection,
    password: &str,
    destination: &Path,
    logs: &[String],
) -> Result<()> {
    use base64::engine::general_purpose::STANDARD;
    ensure!(
        !password.is_empty() && !password.contains(['\n', '\r']),
        "请输入有效的设备登录密码"
    );
    connection.session.set_timeout(60_000);
    let script = include_str!("../../tools/export-diagnostics.py");
    // Keep archive output private and binary-safe. The ordinary text logger
    // masks password substrings, which could corrupt a base64 archive.
    let mut channel = connection.session.channel_session()?;
    let local_log = logs.join("\n").replace(password, "[密码已隐藏]");
    let recent: String = local_log
        .chars()
        .rev()
        .take(64 * 1024)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    channel.exec(&format!(
        "sudo -S -p '' -- python3 -c {} --installer-log-base64 {}",
        quote(script),
        quote(&STANDARD.encode(recent))
    ))?;
    channel.write_all(password.as_bytes())?;
    channel.write_all(b"\n")?;
    channel.flush()?;
    channel.send_eof()?;
    let mut output = String::new();
    Read::by_ref(&mut channel)
        .take(12 * 1024 * 1024)
        .read_to_string(&mut output)?;
    let mut error = String::new();
    channel.stderr().take(4096).read_to_string(&mut error)?;
    channel.wait_close()?;
    ensure!(
        channel.exit_status()? == 0,
        "日志收集失败：{}",
        error.replace(password, "[密码已隐藏]")
    );
    let archive: serde_json::Value =
        serde_json::from_str(output.trim()).context("日志包响应无效")?;
    let bytes = STANDARD.decode(archive["data"].as_str().context("日志包缺少数据")?)?;
    ensure!(
        bytes.len() <= 9 * 1024 * 1024 && bytes.starts_with(b"PK\x03\x04"),
        "日志包格式无效"
    );
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).context("无法创建日志包")?;
    file.write_all(&bytes).context("无法保存日志包")?;
    file.as_file().sync_all()?;
    file.persist(destination).context("无法保存日志包")?;
    Ok(())
}
