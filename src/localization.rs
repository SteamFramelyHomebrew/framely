use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LanguagePack {
    pub schema_version: u32,
    pub locale: String,
    pub name: String,
    pub messages: BTreeMap<String, String>,
}
pub fn valid_locale(locale: &str) -> bool {
    let parts: Vec<_> = locale.split('-').collect();
    locale.len() <= 35
        && (2..=8).contains(&parts[0].len())
        && parts[0].bytes().all(|b| b.is_ascii_alphabetic())
        && parts[1..]
            .iter()
            .all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
        && locale != "auto"
}
impl LanguagePack {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1 && valid_locale(&self.locale),
            "无效的语言文件格式或语言代码"
        );
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 120
                && !self.name.chars().any(char::is_control),
            "无效的语言名称"
        );
        ensure!(
            !self.messages.is_empty() && self.messages.len() <= 3000,
            "语言文件必须包含 1–3000 条翻译"
        );
        for (key, value) in &self.messages {
            ensure!(
                !key.is_empty() && key.len() <= 16384 && value.len() <= 16384,
                "翻译文本过长"
            );
            ensure!(
                placeholders(key) == placeholders(value),
                "翻译占位符不匹配：{key}"
            );
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= 1024 * 1024,
            "语言文件超过 1 MiB"
        );
        Ok(())
    }
}
fn placeholders(text: &str) -> std::collections::BTreeSet<String> {
    let mut result = std::collections::BTreeSet::new();
    for part in text.split('{').skip(1) {
        if let Some((key, _)) = part.split_once('}') {
            if !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) {
                result.insert(key.into());
            }
        }
    }
    result
}
pub fn list(root: &Path) -> Result<serde_json::Value> {
    let dir = root.join("locales");
    let mut packs = vec![];
    let mut invalid = vec![];
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let result = (|| -> Result<LanguagePack> {
                ensure!(entry.file_type()?.is_file(), "Not a regular file");
                ensure!(entry.metadata()?.len() <= 1024 * 1024, "File too large");
                let pack: LanguagePack = serde_json::from_slice(&fs::read(&path)?)?;
                pack.validate()?;
                ensure!(
                    path.file_stem().and_then(|s| s.to_str()) == Some(pack.locale.as_str()),
                    "Locale does not match filename"
                );
                Ok(pack)
            })();
            match result {
                Ok(p) => packs.push(p),
                Err(_) => invalid.push(entry.file_name().to_string_lossy().to_string()),
            }
        }
    }
    packs.sort_by(|a, b| a.locale.cmp(&b.locale));
    invalid.sort();
    Ok(serde_json::json!({"packs":packs,"invalidFiles":invalid}))
}
pub fn install(root: &Path, pack: &LanguagePack) -> Result<()> {
    pack.validate()?;
    let dir = root.join("locales");
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755))?;
    let count = fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|e| e == "json"))
        .count();
    ensure!(
        count < 50 || dir.join(format!("{}.json", pack.locale)).is_file(),
        "最多安装 50 个语言文件"
    );
    let stage = dir.join(format!("{}.json.tmp", pack.locale));
    fs::write(&stage, serde_json::to_vec_pretty(pack)?)?;
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o644))?;
    fs::File::open(&stage)?.sync_all()?;
    fs::rename(stage, dir.join(format!("{}.json", pack.locale)))?;
    fs::File::open(&dir)?.sync_all()?;
    Ok(())
}
pub fn validate_selection(root: &Path, locale: &str) -> Result<()> {
    if matches!(locale, "auto" | "zh-CN" | "en-US") {
        return Ok(());
    }
    ensure!(valid_locale(locale), "无效的语言代码");
    let bytes = fs::read(root.join("locales").join(format!("{locale}.json")))
        .context("语言文件尚未安装")?;
    ensure!(bytes.len() <= 1024 * 1024, "语言文件超过 1 MiB");
    let pack: LanguagePack = serde_json::from_slice(&bytes)?;
    pack.validate()?;
    ensure!(pack.locale == locale, "语言代码不匹配");
    Ok(())
}
