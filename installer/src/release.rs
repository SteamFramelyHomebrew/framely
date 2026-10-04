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
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(120))
        .redirects(0)
        .build()
}
fn get(url: &str) -> Result<ureq::Response> {
    let agent = agent();
    let mut url = url::Url::parse(url)?;
    for _ in 0..=5 {
        ensure!(url.scheme() == "https", "下载地址必须使用 HTTPS");
        let response = agent
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
        all.extend(items.into_iter().filter(|r| !r.draft && package(r).is_ok()));
        if finished {
            return Ok(all);
        }
    }
    Ok(all)
}
pub fn package(release: &Release) -> Result<(&Asset, &Asset)> {
    let packages: Vec<_> = release
        .assets
        .iter()
        .filter(|a| safe_archive_name(&a.name))
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
    loop {
        let n = file.read(&mut block)?;
        if n == 0 {
            break;
        }
        digest.update(&block[..n]);
    }
    ensure!(
        hex::encode(digest.finalize()) == expected,
        "SHA256 不匹配，已停止安装"
    );
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
        verify(&archive, &sums).unwrap();
        std::fs::write(&archive, b"changed").unwrap();
        assert!(verify(&archive, &sums).is_err());
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
}
