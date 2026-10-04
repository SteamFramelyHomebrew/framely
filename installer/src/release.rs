use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, time::Duration};

#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub name: Option<String>,
    pub published_at: Option<String>,
    pub body: Option<String>,
    pub prerelease: bool,
    pub draft: bool,
    pub assets: Vec<Asset>,
}
pub fn validate_repo(repo: &str) -> Result<()> {
    let parts: Vec<_> = repo.split('/').collect();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|p| !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))),
        "仓库格式应为 owner/repo"
    );
    Ok(())
}
fn agent(url: &url::Url) -> Result<ureq::Agent> {
    let mut builder = ureq::AgentBuilder::new()
        .try_proxy_from_env(false)
        .timeout(Duration::from_secs(120))
        .redirects(0);
    if let Some(proxy) = crate::proxy::for_url(url)? {
        builder = builder.proxy(proxy);
    }
    Ok(builder.build())
}
fn get(url: &str) -> Result<ureq::Response> {
    let mut url = url::Url::parse(url)?;
    for _ in 0..=5 {
        ensure!(url.scheme() == "https", "下载地址必须使用 HTTPS");
        // Re-evaluate bypass rules for every redirect target.
        let response = agent(&url)?
            .get(url.as_str())
            .set("User-Agent", "Framely-Installer")
            .call()?;
        if (300..400).contains(&response.status()) {
            url = url.join(response.header("Location").context("重定向缺少地址")?)?;
        } else {
            return Ok(response);
        }
    }
    bail!("下载重定向次数过多")
}
pub fn list(repo: &str) -> Result<Vec<Release>> {
    validate_repo(repo)?;
    let mut all = Vec::new();
    for page in 1..=10 {
        let response = get(&format!(
            "https://api.github.com/repos/{repo}/releases?per_page=100&page={page}"
        ))?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 4 * 1024 * 1024, "Release 列表过大");
        let items: Vec<Release> = serde_json::from_slice(&bytes)?;
        let finished = items.len() < 100;
        all.extend(
            items
                .into_iter()
                .filter(|r| !r.draft && (package(r).is_ok() || package_for(r, "update").is_ok())),
        );
        if finished {
            return Ok(all);
        }
    }
    Ok(all)
}
pub fn package(release: &Release) -> Result<(&Asset, &Asset)> {
    package_for(release, "install")
}
pub fn package_for<'a>(release: &'a Release, action: &str) -> Result<(&'a Asset, &'a Asset)> {
    let offline = action == "install"
        && release
            .assets
            .iter()
            .any(|a| safe_archive_name(&a.name) && a.name.ends_with("-offline-linux-arm64.tar.gz"));
    ensure!(
        action != "install"
            || offline
            || !release.assets.iter().any(|a| a.name == "framely-cef.json"),
        "首次安装需要包含 CEF 的完整离线包"
    );
    let packages: Vec<_> = release
        .assets
        .iter()
        .filter(|a| {
            safe_archive_name(&a.name) && a.name.ends_with("-offline-linux-arm64.tar.gz") == offline
        })
        .collect();
    ensure!(packages.len() == 1, "该版本缺少唯一的 Framely ARM64 发行包");
    let checksum = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .context("缺少 SHA256SUMS")?;
    Ok((packages[0], checksum))
}
pub fn safe_archive_name(name: &str) -> bool {
    name.strip_prefix("framely-")
        .is_some_and(|suffix| suffix.as_bytes().first().is_some_and(u8::is_ascii_digit))
        && name.ends_with("-linux-arm64.tar.gz")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
}
pub fn expected_hash(checksums: &str, name: &str) -> Result<String> {
    let mut matched = None;
    for line in checksums.lines().filter(|s| !s.trim().is_empty()) {
        let bytes = line.as_bytes();
        ensure!(
            bytes.len() > 66
                && bytes[..64].iter().all(u8::is_ascii_hexdigit)
                && bytes[64] == b' '
                && matches!(bytes[65], b' ' | b'*'),
            "SHA256SUMS 格式错误"
        );
        if &line[66..] == name {
            ensure!(matched.is_none(), "校验文件包含重复条目");
            matched = Some(line[..64].to_lowercase());
        }
    }
    matched.context("校验文件中没有该压缩包的条目")
}
pub fn verify(archive: &Path, checksums: &Path) -> Result<()> {
    verify_with_progress(archive, checksums, &mut |_| {})
}
pub fn verify_with_progress(
    archive: &Path,
    checksums: &Path,
    report: &mut dyn FnMut(crate::progress::Progress),
) -> Result<()> {
    ensure!(
        archive.is_file() && checksums.is_file(),
        "请选择压缩包及外部 SHA256SUMS"
    );
    ensure!(
        archive.metadata()?.len() <= 2 * 1024 * 1024 * 1024,
        "压缩包过大"
    );
    ensure!(checksums.metadata()?.len() <= 1024 * 1024, "校验文件过大");
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .context("文件名不是 UTF-8")?;
    ensure!(
        safe_archive_name(name),
        "请选择 Framely Linux ARM64 发行压缩包"
    );
    let expected = expected_hash(&std::fs::read_to_string(checksums)?, name)?;
    let mut file = File::open(archive)?;
    let mut digest = Sha256::new();
    let mut block = [0; 256 * 1024];
    let mut progress =
        crate::progress::Progress::new(crate::progress::Stage::Verify, "校验本地安装包 SHA256");
    let total = file.metadata()?.len();
    progress.total = Some(total);
    report(progress.clone());
    let mut last_update = std::time::Instant::now();
    loop {
        let n = file.read(&mut block)?;
        if n == 0 {
            break;
        }
        digest.update(&block[..n]);
        progress.completed += n as u64;
        if progress.completed < total && last_update.elapsed() >= Duration::from_millis(200) {
            report(progress.clone());
            last_update = std::time::Instant::now();
        }
    }
    ensure!(
        hex::encode(digest.finalize()) == expected,
        "SHA256 不匹配，已停止安装"
    );
    ensure!(progress.completed == total, "校验期间安装包长度发生变化");
    report(progress);
    Ok(())
}
pub fn download(
    asset: &Asset,
    dest: &Path,
    progress: &mut dyn FnMut(crate::progress::Progress),
) -> Result<()> {
    ensure!(
        asset.size > 0 && asset.size <= 2 * 1024 * 1024 * 1024,
        "下载长度无效"
    );
    let mut starting =
        crate::progress::Progress::new(crate::progress::Stage::Download, &asset.name);
    starting.total = Some(asset.size);
    progress(starting);
    let mut input = get(&asset.browser_download_url)?.into_reader();
    let mut output = File::create(dest)?;
    crate::progress::copy(
        &mut input,
        &mut output,
        asset.size,
        crate::progress::Stage::Download,
        &asset.name,
        progress,
    )?;
    output.sync_all()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    fn https_requests_use_discovered_desktop_proxy() {
        use std::{
            io::Write,
            net::TcpListener,
            os::unix::fs::PermissionsExt,
            process::{Command, Stdio},
            time::Instant,
        };
        if std::env::var_os("FRAMELY_PROXY_TEST_CHILD").is_some() {
            assert!(get("https://example.invalid/installer-proxy-check").is_err());
            return;
        }
        let work = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mock = work.path().join("gsettings");
        std::fs::write(&mock, format!("#!/bin/sh\nprintf '%s\\n' \"org.gnome.system.proxy mode 'manual'\" \"org.gnome.system.proxy.https host '127.0.0.1'\" \"org.gnome.system.proxy.https port {port}\"\n")).unwrap();
        std::fs::set_permissions(&mock, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "release::tests::https_requests_use_discovered_desktop_proxy",
                "--nocapture",
            ])
            .env("FRAMELY_PROXY_TEST_CHILD", "1")
            .env("XDG_CURRENT_DESKTOP", "GNOME")
            .env("XDG_CONFIG_HOME", work.path())
            .env("XDG_CONFIG_DIRS", work.path())
            .env(
                "PATH",
                std::env::join_paths(std::iter::once(work.path().to_path_buf()).chain(
                    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
                ))
                .unwrap(),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for key in [
            "http_proxy",
            "HTTP_PROXY",
            "https_proxy",
            "HTTPS_PROXY",
            "all_proxy",
            "ALL_PROXY",
            "no_proxy",
            "NO_PROXY",
        ] {
            command.env_remove(key);
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut connection = loop {
            if let Ok((connection, _)) = listener.accept() {
                break connection;
            }
            if Instant::now() >= deadline || child.try_wait().unwrap().is_some() {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "Installer did not connect through detected proxy: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        connection
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        while !request.ends_with(b"\r\n\r\n") {
            match connection.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => request.extend_from_slice(&buffer[..n]),
            }
        }
        connection
            .write_all(
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
        drop(connection);
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(request.starts_with(b"CONNECT example.invalid:443 HTTP/1.1\r\n"));
    }
    #[test]
    fn checksum_rejects_ambiguity_and_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("framely-1-test-linux-arm64.tar.gz");
        let sums = dir.path().join("SHA256SUMS");
        std::fs::write(&archive, b"payload").unwrap();
        let line = format!(
            "{}  {}\n",
            hex::encode(Sha256::digest(b"payload")),
            archive.file_name().unwrap().to_str().unwrap()
        );
        std::fs::write(&sums, &line).unwrap();
        let mut progress = Vec::new();
        verify_with_progress(&archive, &sums, &mut |value| progress.push(value)).unwrap();
        assert_eq!(progress.first().unwrap().completed, 0);
        assert_eq!(progress.last().unwrap().fraction(), Some(1.0));
        assert!(
            progress
                .iter()
                .all(|value| value.stage == crate::progress::Stage::Verify)
        );
        std::fs::write(&archive, b"changed").unwrap();
        progress.clear();
        assert!(verify_with_progress(&archive, &sums, &mut |value| progress.push(value)).is_err());
        assert!(!progress.iter().any(|value| value.fraction() == Some(1.0)));
        assert!(
            expected_hash(
                &(line.clone() + &line),
                archive.file_name().unwrap().to_str().unwrap()
            )
            .is_err()
        );
        assert!(!safe_archive_name("../framely-1-linux-arm64.tar.gz"));
        assert!(!safe_archive_name(
            "framely-installer-0.4.1-linux-arm64.tar.gz"
        ));
        assert!(validate_repo("owner/repo/extra").is_err());
    }

    #[test]
    fn installer_release_is_not_a_frame_release() {
        let mut release = Release {
            tag_name: "installer-v0.4.1".into(),
            name: None,
            published_at: None,
            body: None,
            prerelease: false,
            draft: false,
            assets: vec![
                Asset {
                    name: "framely-installer-0.4.1-linux-arm64.tar.gz".into(),
                    browser_download_url: "https://example.org/installer.tar.gz".into(),
                    size: 1,
                },
                Asset {
                    name: "SHA256SUMS".into(),
                    browser_download_url: "https://example.org/SHA256SUMS".into(),
                    size: 1,
                },
            ],
        };
        assert!(package(&release).is_err());
        release.assets[0].name = "framely-0.4.2-build-linux-arm64.tar.gz".into();
        assert!(package(&release).is_ok());
    }

    #[test]
    fn first_install_uses_offline_and_updates_use_core_with_three_archives() {
        let mut release = Release {
            tag_name: "v0.4.2".into(),
            name: None,
            published_at: None,
            body: None,
            prerelease: false,
            draft: false,
            assets: [
                "framely-0.4.2-build-linux-arm64.tar.gz",
                "framely-0.4.2-build-offline-linux-arm64.tar.gz",
                "framely-cef-154-build-linux-arm64.tar.gz",
                "framely-cef.json",
                "SHA256SUMS",
            ]
            .into_iter()
            .map(|name| Asset {
                name: name.into(),
                browser_download_url: format!("https://example.org/{name}"),
                size: 1,
            })
            .collect(),
        };
        assert!(
            package_for(&release, "install")
                .unwrap()
                .0
                .name
                .ends_with("-offline-linux-arm64.tar.gz")
        );
        assert_eq!(
            package_for(&release, "update").unwrap().0.name,
            "framely-0.4.2-build-linux-arm64.tar.gz"
        );
        release
            .assets
            .retain(|a| !a.name.ends_with("-offline-linux-arm64.tar.gz"));
        assert!(package_for(&release, "install").is_err());
        assert!(package_for(&release, "update").is_ok());
    }
}
