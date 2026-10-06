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
// Android resource pools can encode supplementary characters as two UTF-8
// surrogate code units (CESU-8), rather than one standard four-byte sequence.
// Decode only those extensions; malformed UTF-8 and unpaired surrogates remain
// errors instead of silently replacing package metadata.
fn android_utf8(b: &[u8]) -> Result<String> {
    if let Ok(s) = std::str::from_utf8(b) {
        return Ok(s.to_owned());
    }
    let mut units = Vec::new();
    let mut remaining = b;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(s) => {
                units.extend(s.encode_utf16());
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                units.extend(std::str::from_utf8(&remaining[..valid])?.encode_utf16());
                remaining = &remaining[valid..];
                if remaining.starts_with(&[0xc0, 0x80]) {
                    // Modified UTF-8's encoding of NUL.
                    units.push(0);
                    remaining = &remaining[2..];
                } else if remaining.len() >= 3
                    && remaining[0] == 0xed
                    && (0xa0..=0xbf).contains(&remaining[1])
                    && (0x80..=0xbf).contains(&remaining[2])
                {
                    units.push(
                        ((remaining[0] as u16 & 15) << 12)
                            | ((remaining[1] as u16 & 63) << 6)
                            | (remaining[2] as u16 & 63),
                    );
                    remaining = &remaining[3..];
                } else {
                    bail!("Invalid Android resource string encoding");
                }
            }
        }
    }
    String::from_utf16(&units).context("Unpaired surrogate in Android resource string")
}
fn strings(b: &[u8]) -> Result<Vec<String>> {
    let count = u32at(b, 8)? as usize;
    ensure!(count <= 262144, "APK string pool is too large");
    let utf8 = u32at(b, 16)? & 256 != 0;
    let start = u32at(b, 20)? as usize;
    let header = u16at(b, 2)? as usize;
    let styles = u32at(b, 12)? as usize;
    let style_start = u32at(b, 24)? as usize;
    ensure!(
        header >= 28 && start >= header + (count + styles) * 4 && start <= b.len(),
        "Invalid APK string pool layout"
    );
    let end = if style_start == 0 {
        b.len()
    } else {
        style_start
    };
    ensure!(
        end >= start && end <= b.len(),
        "Invalid APK string data bounds"
    );
    let data = &b[..end];
    (0..count)
        .map(|i| {
            let mut p = start
                .checked_add(u32at(b, header + i * 4)? as usize)
                .context("Invalid string offset")?;
            let n = string_len(data, &mut p, utf8)?;
            if utf8 {
                let bytes = string_len(data, &mut p, true)?;
                let s = android_utf8(data.get(p..p + bytes).context("Invalid APK string")?)
                    .with_context(|| format!("Invalid APK string pool entry {i}"))?;
                ensure!(data.get(p + bytes) == Some(&0), "Unterminated APK string");
                // Older AAPT versions truncate encoded lengths to 15 bits.
                ensure!(
                    s.encode_utf16().count() & 0x7fff == n,
                    "Invalid APK string length"
                );
                Ok(s)
            } else {
                let bytes = data.get(p..p + n * 2).context("Invalid UTF16 string")?;
                ensure!(
                    u16at(data, p + n * 2)? == 0,
                    "Unterminated UTF16 APK string"
                );
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
    value(typ, v, s)
}
fn value(typ: u8, v: u32, s: &[String]) -> Result<Val> {
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
                    let type_offset = if u16at(c, 2)? >= 288 {
                        u32at(c, 284)?
                    } else {
                        0
                    };
                    let id = (*t.get(8).context("Invalid resource type")? as u32)
                        .checked_add(type_offset)
                        .context("Invalid APK resource type offset")?;
                    ensure!(id > 0 && id <= 255, "Invalid APK resource type ID");
                    let flags = *t.get(9).context("Invalid resource type flags")?;
                    if flags & !3 != 0 {
                        continue;
                    }
                    ensure!(flags != 3, "Conflicting APK resource offset formats");
                    let count = u32at(t, 12)? as usize;
                    let start = u32at(t, 16)? as usize;
                    let h = u16at(t, 2)? as usize;
                    ensure!(count <= 65536, "Too many APK resources");
                    let stride = if flags == 2 { 2 } else { 4 };
                    ensure!(
                        h >= 20 && start >= h + count * stride && start <= t.len(),
                        "Invalid APK resource entry table"
                    );
                    for i in 0..count {
                        let (index, offset) = match flags {
                            1 => (
                                u16at(t, h + i * 4)? as u32,
                                u16at(t, h + i * 4 + 2)? as usize * 4,
                            ),
                            2 => {
                                let offset = u16at(t, h + i * 2)?;
                                if offset == u16::MAX {
                                    continue;
                                }
                                (i as u32, offset as usize * 4)
                            }
                            _ => {
                                let offset = u32at(t, h + i * 4)?;
                                if offset == u32::MAX {
                                    continue;
                                }
                                (i as u32, offset as usize)
                            }
                        };
                        let e = start + offset as usize;
                        let size = u16at(t, e)? as usize;
                        let entry_flags = u16at(t, e + 2)?;
                        if entry_flags & 1 != 0 {
                            continue;
                        }
                        let key = (pkg << 24) | (id << 16) | index;
                        let v = if entry_flags & 8 != 0 {
                            value((entry_flags >> 8) as u8, u32at(t, e + 4)?, &pool)?
                        } else {
                            ensure!(size >= 8, "Invalid APK resource entry size");
                            val(t, e + size, &pool)?
                        };
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
        Ok(b) => resources(&b).context("Cannot parse APK resource table")?,
        Err(e) => {
            if z.by_name("resources.arsc").is_ok() {
                return Err(e);
            }
            BTreeMap::new()
        }
    };
    let b = entry(&mut z, "AndroidManifest.xml", 8 * 1024 * 1024)?;
    let mut m = xml(&b, &r).context("Cannot parse APK manifest")?;
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
    fn utf8_pool(entries: &[(u8, &[u8])]) -> Vec<u8> {
        let start = 28 + entries.len() * 4;
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for (units, bytes) in entries {
            offsets.push(data.len() as u32);
            data.extend([*units, bytes.len() as u8]);
            data.extend(*bytes);
            data.push(0);
        }
        let mut pool = Vec::new();
        pool.extend(1u16.to_le_bytes());
        pool.extend(28u16.to_le_bytes());
        pool.extend(((start + data.len()) as u32).to_le_bytes());
        pool.extend((entries.len() as u32).to_le_bytes());
        pool.extend(0u32.to_le_bytes());
        pool.extend(256u32.to_le_bytes());
        pool.extend((start as u32).to_le_bytes());
        pool.extend(0u32.to_le_bytes());
        for offset in offsets {
            pool.extend(offset.to_le_bytes());
        }
        pool.extend(data);
        pool
    }
    #[test]
    fn android_resource_strings_accept_cesu8_and_standard_utf8() {
        // Moonlight V+ resources start with CESU-8 emoji, e.g. "🌐 Bitrate".
        let pool = utf8_pool(&[
            (10, b"\xed\xa0\xbc\xed\xbc\x90 Bitrate"),
            (5, "中文 🌸".as_bytes()),
            (3, b"a\xc0\x80b"),
        ]);
        assert_eq!(strings(&pool).unwrap(), ["🌐 Bitrate", "中文 🌸", "a\0b"]);
        for invalid in [
            &b"\xed\xa0\xbc"[..], // Unpaired high surrogate.
            &b"\xed\xbc\x90"[..], // Unpaired low surrogate.
            &b"\xed\xa0"[..],
            &b"\xff"[..],
            &b"\xc0\xaf"[..], // Overlong encoding must not be accepted.
            &b"\xf4\x90\x80\x80"[..],
        ] {
            assert!(strings(&utf8_pool(&[(1, invalid)])).is_err());
        }
        assert!(strings(&utf8_pool(&[(1, b"two")])).is_err());
        let mut unterminated = utf8_pool(&[(1, b"a")]);
        *unterminated.last_mut().unwrap() = 1;
        assert!(strings(&unterminated).is_err());
    }
    #[test]
    fn utf16_resource_strings_and_string_bounds() {
        let units: Vec<u16> = "中文 🌸".encode_utf16().collect();
        let mut pool = utf8_pool(&[]);
        pool[8..12].copy_from_slice(&1u32.to_le_bytes());
        pool[16..20].copy_from_slice(&0u32.to_le_bytes());
        pool[20..24].copy_from_slice(&32u32.to_le_bytes());
        pool.extend(0u32.to_le_bytes());
        pool.extend((units.len() as u16).to_le_bytes());
        for unit in units {
            pool.extend(unit.to_le_bytes());
        }
        pool.extend(0u16.to_le_bytes());
        let len = pool.len() as u32;
        pool[4..8].copy_from_slice(&len.to_le_bytes());
        assert_eq!(strings(&pool).unwrap(), ["中文 🌸"]);
        pool.pop();
        assert!(strings(&pool).is_err());
        let mut pool = utf8_pool(&[(1, b"a")]);
        pool[20..24].copy_from_slice(&0u32.to_le_bytes());
        assert!(strings(&pool).is_err());
        let mut pool = utf8_pool(&[(1, b"a")]);
        pool[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(strings(&pool).is_err());
    }
    #[test]
    fn resource_offsets_and_compact_entries() {
        for flags in [0u8, 1, 2] {
            for compact in [false, true] {
                let mut table = vec![0; 20];
                table[0..2].copy_from_slice(&0x201u16.to_le_bytes());
                table[2..4].copy_from_slice(&20u16.to_le_bytes());
                table[8] = 1;
                table[9] = flags;
                let count: u32 = if flags == 1 { 1 } else { 6 };
                table[12..16].copy_from_slice(&count.to_le_bytes());
                match flags {
                    1 => {
                        table.extend(5u16.to_le_bytes());
                        table.extend(0u16.to_le_bytes());
                    }
                    2 => {
                        for _ in 0..5 {
                            table.extend(u16::MAX.to_le_bytes());
                        }
                        table.extend(0u16.to_le_bytes());
                    }
                    _ => {
                        for _ in 0..5 {
                            table.extend(u32::MAX.to_le_bytes());
                        }
                        table.extend(0u32.to_le_bytes());
                    }
                }
                let start = table.len() as u32;
                table[16..20].copy_from_slice(&start.to_le_bytes());
                table.extend((if compact { 123u16 } else { 8 }).to_le_bytes());
                table.extend((if compact { 0x308u16 } else { 0 }).to_le_bytes());
                table.extend(0u32.to_le_bytes());
                if !compact {
                    table.extend([8, 0, 0, 3]);
                    table.extend(0u32.to_le_bytes());
                }
                let len = table.len() as u32;
                table[4..8].copy_from_slice(&len.to_le_bytes());
                let mut package = vec![0; 288];
                package[0..2].copy_from_slice(&0x200u16.to_le_bytes());
                package[2..4].copy_from_slice(&288u16.to_le_bytes());
                package[8..12].copy_from_slice(&127u32.to_le_bytes());
                package[284..288].copy_from_slice(&2u32.to_le_bytes());
                package.extend(table);
                let len = package.len() as u32;
                package[4..8].copy_from_slice(&len.to_le_bytes());
                let mut resource = vec![0; 12];
                resource[0..2].copy_from_slice(&2u16.to_le_bytes());
                resource[2..4].copy_from_slice(&12u16.to_le_bytes());
                resource.extend(utf8_pool(&[(5, b"Label")]));
                resource.extend(package);
                let len = resource.len() as u32;
                resource[4..8].copy_from_slice(&len.to_le_bytes());
                let parsed = resources(&resource).unwrap();
                assert_eq!(text(parsed.get(&0x7f030005), &parsed), "Label");
                resource.pop();
                assert!(resources(&resource).is_err());
            }
        }
    }
    #[test]
    fn inspect_apk_with_cesu8_resources() {
        use std::io::Write;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("emoji.apk");
        fixture(&p, true, 42);
        let manifest = entry(
            &mut zip::ZipArchive::new(File::open(&p).unwrap()).unwrap(),
            "AndroidManifest.xml",
            8 * 1024 * 1024,
        )
        .unwrap();
        let pool = utf8_pool(&[(2, b"\xed\xa0\xbc\xed\xbc\x90")]);
        let mut resources = Vec::new();
        resources.extend(2u16.to_le_bytes());
        resources.extend(12u16.to_le_bytes());
        resources.extend(((12 + pool.len()) as u32).to_le_bytes());
        resources.extend(0u32.to_le_bytes());
        resources.extend(pool);
        let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
        for (name, bytes) in [
            ("AndroidManifest.xml", manifest),
            ("resources.arsc", resources),
        ] {
            z.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(&bytes).unwrap();
        }
        z.finish().unwrap();
        let m = read(&p).unwrap();
        assert_eq!(m.package, "com.example.app");
        assert_eq!(m.name, "Example");
        assert_eq!(m.activities, ["com.example.app.Main"]);
        // Exercise the same binary upload -> inspection -> review path as the
        // PC management panel, without running or installing the APK.
        let bytes = std::fs::read(&p).unwrap();
        let home = d.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let upload_dir = crate::apk::upload_dir(&home, bytes.len() as u64).unwrap();
        let uploads = crate::uploads::Uploads::default();
        let upload = uploads.start_in(bytes.len() as u64, &upload_dir).unwrap();
        let id = upload["upload"].as_str().unwrap();
        uploads.append(id, 0, &bytes).unwrap();
        let staged = uploads.take(id).unwrap();
        let review =
            crate::apk::inspect(&home, &staged.path, &crate::jobs::Cancellation::default())
                .unwrap();
        assert_eq!(review["metadata"]["name"], "Example");
        assert!(review["ticket"].is_string());
    }
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
    fixture_named(path, launch, version, "com.example.app");
}
#[cfg(test)]
pub(crate) fn fixture_named(path: &Path, launch: bool, version: u32, package: &str) {
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
        package,
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
