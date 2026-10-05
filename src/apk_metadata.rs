//! Bounded Android binary XML/resource reader. No APK code is executed.
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub package: String,
    pub name: String,
    pub version: String,
    pub version_code: u64,
    pub min_sdk: u32,
    pub activities: Vec<String>,
    pub declared_activities: Vec<String>,
    pub icon: Option<String>,
    pub abis: Vec<String>,
    pub vr: bool,
}
fn u16at(b: &[u8], p: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        b.get(p..p + 2)
            .context("Truncated APK metadata")?
            .try_into()?,
    ))
}
fn u32at(b: &[u8], p: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4)
            .context("Truncated APK metadata")?
            .try_into()?,
    ))
}
fn string_len(b: &[u8], p: &mut usize, utf8: bool) -> Result<usize> {
    if utf8 {
        let a = *b.get(*p).context("Invalid string length")?;
        *p += 1;
        if a & 128 != 0 {
            let v =
                ((a as usize & 127) << 8) | *b.get(*p).context("Invalid string length")? as usize;
            *p += 1;
            Ok(v)
        } else {
            Ok(a as usize)
        }
    } else {
        let a = u16at(b, *p)?;
        *p += 2;
        if a & 0x8000 != 0 {
            let v = ((a as usize & 0x7fff) << 16) | u16at(b, *p)? as usize;
            *p += 2;
            Ok(v)
        } else {
            Ok(a as usize)
        }
    }
}
fn strings(b: &[u8]) -> Result<Vec<String>> {
    let count = u32at(b, 8)? as usize;
    ensure!(count <= 262144, "APK string pool is too large");
    let utf8 = u32at(b, 16)? & 256 != 0;
    let start = u32at(b, 20)? as usize;
    let header = u16at(b, 2)? as usize;
    (0..count)
        .map(|i| {
            let mut p = start
                .checked_add(u32at(b, header + i * 4)? as usize)
                .context("Invalid string offset")?;
            let n = string_len(b, &mut p, utf8)?;
            if utf8 {
                let bytes = string_len(b, &mut p, true)?;
                Ok(
                    std::str::from_utf8(b.get(p..p + bytes).context("Invalid APK string")?)?
                        .to_owned(),
                )
            } else {
                let bytes = b.get(p..p + n * 2).context("Invalid UTF16 string")?;
                Ok(String::from_utf16(
                    &bytes
                        .chunks_exact(2)
                        .map(|s| u16::from_le_bytes([s[0], s[1]]))
                        .collect::<Vec<_>>(),
                )?)
            }
        })
        .collect()
}
fn chunks(b: &[u8], mut p: usize) -> Result<Vec<&[u8]>> {
    let mut out = Vec::new();
    while p < b.len() {
        let n = u32at(b, p + 4)? as usize;
        ensure!(n >= 8 && n <= b.len() - p, "Invalid APK chunk size");
        out.push(&b[p..p + n]);
        p += n;
    }
    Ok(out)
}
#[derive(Clone)]
enum Val {
    Text(String),
    Ref(u32),
    Int(u32),
}
fn val(b: &[u8], p: usize, s: &[String]) -> Result<Val> {
    let typ = *b.get(p + 3).context("Invalid value")?;
    let v = u32at(b, p + 4)?;
    Ok(match typ {
        3 => Val::Text(
            s.get(v as usize)
                .context("Invalid string reference")?
                .clone(),
        ),
        1 => Val::Ref(v),
        _ => Val::Int(v),
    })
}
fn resources(b: &[u8]) -> Result<BTreeMap<u32, Val>> {
    let mut map = BTreeMap::new();
    let mut pool = Vec::new();
    for c in chunks(b, u16at(b, 2)? as usize)? {
        match u16at(c, 0)? {
            1 => pool = strings(c)?,
            0x200 => {
                let pkg = u32at(c, 8)?;
                for t in chunks(c, u16at(c, 2)? as usize)? {
                    if u16at(t, 0)? != 0x201 {
                        continue;
                    }
                    let id = *t.get(8).context("Invalid resource type")? as u32;
                    let flags = t[9];
                    if flags != 0 {
                        continue;
                    }
                    let count = u32at(t, 12)? as usize;
                    let start = u32at(t, 16)? as usize;
                    let h = u16at(t, 2)? as usize;
                    ensure!(count <= 65536, "Too many APK resources");
                    for i in 0..count {
                        let offset = u32at(t, h + i * 4)?;
                        if offset == u32::MAX {
                            continue;
                        }
                        let e = start + offset as usize;
                        let size = u16at(t, e)? as usize;
                        if u16at(t, e + 2)? & 1 != 0 {
                            continue;
                        }
                        let key = (pkg << 24) | (id << 16) | i as u32;
                        let v = val(t, e + size, &pool)?;
                        map.entry(key).or_insert(v);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(map)
}
fn text(v: Option<&Val>, r: &BTreeMap<u32, Val>) -> String {
    let mut v = v;
    for _ in 0..16 {
        match v {
            Some(Val::Text(s)) => return s.clone(),
            Some(Val::Int(n)) => return n.to_string(),
            Some(Val::Ref(id)) => v = r.get(id),
            None => break,
        }
    }
    String::new()
}
fn numeric(v: Option<&Val>) -> u32 {
    match v {
        Some(Val::Int(n)) => *n,
        Some(Val::Text(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}
pub fn valid_package(s: &str) -> bool {
    s.len() <= 255
        && s.contains('.')
        && s.split('.').all(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && !p.as_bytes()[0].is_ascii_digit()
        })
}
fn activity(p: &str, n: &str) -> String {
    if n.starts_with('.') {
        format!("{p}{n}")
    } else if !n.contains('.') {
        format!("{p}.{n}")
    } else {
        n.into()
    }
}
fn xml(b: &[u8], r: &BTreeMap<u32, Val>) -> Result<Metadata> {
    ensure!(u16at(b, 0)? == 3, "APK manifest must be Android binary XML");
    let mut m = Metadata::default();
    let mut pool = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut current = None::<(String, bool, bool, bool, bool)>;
    let mut label = None;
    let mut icon = None;
    let mut app_enabled = true;
    for c in chunks(b, u16at(b, 2)? as usize)? {
        match u16at(c, 0)? {
            1 => pool = strings(c)?,
            0x102 => {
                let tag = pool
                    .get(u32at(c, 20)? as usize)
                    .context("Invalid XML element")?
                    .clone();
                let start = 16 + u16at(c, 24)? as usize;
                let stride = u16at(c, 26)? as usize;
                let count = u16at(c, 28)? as usize;
                ensure!(stride >= 20 && count <= 4096, "Invalid XML attributes");
                let mut attrs = BTreeMap::new();
                for i in 0..count {
                    let p = start + i * stride;
                    let name = pool
                        .get(u32at(c, p + 4)? as usize)
                        .context("Invalid XML attribute")?
                        .clone();
                    attrs.insert(name, val(c, p + 12, &pool)?);
                }
                match tag.as_str() {
                    "manifest" => {
                        m.package = text(attrs.get("package"), r);
                        m.version = text(attrs.get("versionName"), r);
                        m.version_code = numeric(attrs.get("versionCode")) as u64
                            | ((numeric(attrs.get("versionCodeMajor")) as u64) << 32);
                    }
                    "uses-sdk" => m.min_sdk = numeric(attrs.get("minSdkVersion")),
                    "application" => {
                        label = attrs.get("label").cloned();
                        icon = attrs.get("icon").cloned();
                        app_enabled = attrs.get("enabled").is_none_or(|v| numeric(Some(v)) != 0);
                    }
                    "activity" | "activity-alias" => {
                        let n = activity(&m.package, &text(attrs.get("name"), r));
                        let enabled = app_enabled
                            && attrs.get("enabled").is_none_or(|v| numeric(Some(v)) != 0)
                            && attrs.get("exported").is_none_or(|v| numeric(Some(v)) != 0);
                        m.declared_activities.push(n.clone());
                        current = Some((n, enabled, false, false, false));
                    }
                    "intent-filter" => {
                        if let Some(a) = current.as_mut() {
                            a.2 = false;
                            a.3 = false;
                        }
                    }
                    "action" => {
                        if let Some(a) = current.as_mut() {
                            if text(attrs.get("name"), r) == "android.intent.action.MAIN" {
                                a.2 = true;
                            }
                        }
                    }
                    "category" => {
                        let name = text(attrs.get("name"), r);
                        if let Some(a) = current.as_mut() {
                            if [
                                "android.intent.category.LAUNCHER",
                                "android.intent.category.LEANBACK_LAUNCHER",
                                "com.oculus.intent.category.VR",
                            ]
                            .contains(&name.as_str())
                            {
                                a.3 = true;
                            }
                        }
                        if name == "com.oculus.intent.category.VR" {
                            m.vr = true;
                        }
                    }
                    "meta-data" => {
                        let name = text(attrs.get("name"), r);
                        if name == "com.samsung.android.vr.application.mode"
                            || name == "com.oculus.supportedDevices"
                        {
                            m.vr = true;
                        }
                    }
                    _ => {}
                }
                stack.push(tag);
            }
            0x103 => {
                let tag = stack.pop().context("Unbalanced APK XML")?;
                if tag == "intent-filter" {
                    if let Some(a) = current.as_mut() {
                        a.4 |= a.2 && a.3;
                    }
                }
                if tag == "activity" || tag == "activity-alias" {
                    if let Some((n, enabled, main, launcher, matched)) = current.take() {
                        if enabled && (matched || main && launcher) {
                            m.activities.push(n);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    ensure!(valid_package(&m.package), "Invalid APK package name");
    m.name = text(label.as_ref(), r);
    if m.name.is_empty() {
        m.name = m.package.clone();
    }
    if m.version.is_empty() {
        m.version = m.version_code.to_string();
    }
    let path = text(icon.as_ref(), r);
    if path.starts_with("res/") {
        m.icon = Some(path);
    }
    m.activities.sort();
    m.activities.dedup();
    Ok(m)
}
fn entry(z: &mut zip::ZipArchive<File>, name: &str, max: u64) -> Result<Vec<u8>> {
    let mut f = z.by_name(name)?;
    ensure!(f.size() <= max, "APK metadata exceeds size limit");
    let mut b = Vec::new();
    f.by_ref().take(max + 1).read_to_end(&mut b)?;
    ensure!(b.len() as u64 <= max, "APK metadata exceeds size limit");
    Ok(b)
}
pub fn read(path: &Path) -> Result<Metadata> {
    let mut z = zip::ZipArchive::new(File::open(path)?).context("Invalid APK archive")?;
    ensure!(z.len() <= 65536, "Too many APK files");
    let r = match entry(&mut z, "resources.arsc", 64 * 1024 * 1024) {
        Ok(b) => resources(&b)?,
        Err(e) => {
            if z.by_name("resources.arsc").is_ok() {
                return Err(e);
            }
            BTreeMap::new()
        }
    };
    let b = entry(&mut z, "AndroidManifest.xml", 8 * 1024 * 1024)?;
    let mut m = xml(&b, &r)?;
    if let Some(path) = m.icon.take() {
        let mime = if path.ends_with(".png") {
            Some("image/png")
        } else if path.ends_with(".webp") {
            Some("image/webp")
        } else {
            None
        };
        if let Some(mime) = mime {
            if let Ok(b) = entry(&mut z, &path, 2 * 1024 * 1024) {
                m.icon = Some(format!("data:{mime};base64,{}", STANDARD.encode(b)));
            }
        }
    }
    for i in 0..z.len() {
        let f = z.by_index(i)?;
        if let Some(p) = f.name().strip_prefix("lib/") {
            if let Some((abi, _)) = p.split_once('/') {
                if !m.abis.contains(&abi.to_string()) {
                    m.abis.push(abi.into());
                }
            }
        }
    }
    if !m.abis.is_empty()
        && !m
            .abis
            .iter()
            .any(|a| a == "arm64-v8a" || a == "armeabi-v7a")
    {
        bail!("APK has no compatible ARM libraries");
    }
    Ok(m)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_and_activity_validation() {
        assert!(valid_package("com.example.app"));
        for p in ["../x", "com..app", "com.1app", "app", "com.app;rm"] {
            assert!(!valid_package(p));
        }
        assert_eq!(activity("com.a", ".Main"), "com.a.Main");
    }
    #[test]
    fn malformed_chunks_and_resource_cycles_fail_safely() {
        assert!(chunks(&[0; 8], 0).is_err());
        let r = BTreeMap::from([(1, Val::Ref(2)), (2, Val::Ref(1))]);
        assert_eq!(text(Some(&Val::Ref(1)), &r), "");
    }
}
#[cfg(test)]
pub(crate) fn fixture(path: &Path, launch: bool, version: u32) {
    use std::io::Write;
    fn chunk(kind: u16, header: u16, body: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(kind.to_le_bytes());
        b.extend(header.to_le_bytes());
        b.extend(((body.len() + 8) as u32).to_le_bytes());
        b.extend(body);
        b
    }
    let names = [
        "manifest",
        "package",
        "com.example.app",
        "versionCode",
        "versionName",
        "1.0",
        "application",
        "label",
        "Example",
        "activity",
        "name",
        ".Main",
        "intent-filter",
        "action",
        "android.intent.action.MAIN",
        "category",
        "android.intent.category.LAUNCHER",
        "enabled",
    ];
    let mut data = Vec::new();
    let mut offsets = Vec::new();
    for s in names {
        offsets.push(data.len() as u32);
        data.push(s.len() as u8);
        data.push(s.len() as u8);
        data.extend(s.as_bytes());
        data.push(0);
    }
    while data.len() % 4 != 0 {
        data.push(0)
    }
    let mut pool = Vec::new();
    pool.extend((names.len() as u32).to_le_bytes());
    pool.extend(0u32.to_le_bytes());
    pool.extend(256u32.to_le_bytes());
    pool.extend((28 + names.len() as u32 * 4).to_le_bytes());
    pool.extend(0u32.to_le_bytes());
    for o in offsets {
        pool.extend(o.to_le_bytes())
    }
    pool.extend(data);
    let mut body = chunk(1, 28, &pool);
    fn start(tag: u32, attrs: &[(u32, u8, u32)]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(1u32.to_le_bytes());
        b.extend(u32::MAX.to_le_bytes());
        b.extend(u32::MAX.to_le_bytes());
        b.extend(tag.to_le_bytes());
        b.extend(20u16.to_le_bytes());
        b.extend(20u16.to_le_bytes());
        b.extend((attrs.len() as u16).to_le_bytes());
        b.extend([0; 6]);
        for (name, typ, value) in attrs {
            b.extend(u32::MAX.to_le_bytes());
            b.extend(name.to_le_bytes());
            b.extend(u32::MAX.to_le_bytes());
            b.extend(8u16.to_le_bytes());
            b.push(0);
            b.push(*typ);
            b.extend(value.to_le_bytes());
        }
        chunk(0x102, 16, &b)
    }
    fn end(tag: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(1u32.to_le_bytes());
        b.extend(u32::MAX.to_le_bytes());
        b.extend(u32::MAX.to_le_bytes());
        b.extend(tag.to_le_bytes());
        chunk(0x103, 16, &b)
    }
    body.extend(start(0, &[(1, 3, 2), (3, 0x10, version), (4, 3, 5)]));
    body.extend(start(6, &[(7, 3, 8)]));
    body.extend(start(9, &[(10, 3, 11)]));
    if launch {
        body.extend(start(12, &[]));
        body.extend(start(13, &[(10, 3, 14)]));
        body.extend(end(13));
        body.extend(start(15, &[(10, 3, 16)]));
        body.extend(end(15));
        body.extend(end(12));
    }
    body.extend(end(9));
    body.extend(end(6));
    body.extend(end(0));
    let xml = chunk(3, 8, &body);
    let mut z = zip::ZipWriter::new(File::create(path).unwrap());
    z.start_file(
        "AndroidManifest.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    z.write_all(&xml).unwrap();
    z.finish().unwrap();
}
#[cfg(test)]
mod manifest_tests {
    use super::*;
    #[test]
    fn launchers_labels_versions_and_services() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.apk");
        fixture(&p, true, 42);
        let m = read(&p).unwrap();
        assert_eq!(m.package, "com.example.app");
        assert_eq!(m.name, "Example");
        assert_eq!(m.version_code, 42);
        assert_eq!(m.activities, vec!["com.example.app.Main"]);
        fixture(&p, false, 43);
        assert!(read(&p).unwrap().activities.is_empty());
    }
    #[test]
    fn corrupt_zip_and_xml_are_rejected() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.apk");
        std::fs::write(&p, b"not an APK").unwrap();
        assert!(read(&p).is_err());
        for n in 0..100 {
            assert!(xml(&vec![0; n], &BTreeMap::new()).is_err());
        }
    }
}
