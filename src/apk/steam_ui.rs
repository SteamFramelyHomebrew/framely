//! Local Steam UI API: metadata and launch options, never edits live VDF files.
use super::*;
use std::{
    io::Read,
    net::{SocketAddr, TcpStream},
};
use tungstenite::{client, protocol::WebSocketConfig, Message};

#[cfg(test)]
thread_local! {pub(super) static TEST_RESULT: std::cell::RefCell<Option<Value>>= const {std::cell::RefCell::new(None)};}
pub(crate) fn evaluate(expression: &str) -> Result<Value> {
    #[cfg(test)]
    if let Some(value) = TEST_RESULT.with(|v| v.borrow().clone()) {
        return Ok(value);
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(3))
        .build();
    let mut bytes = Vec::new();
    agent
        .get("http://127.0.0.1:8080/json")
        .call()
        .context("Steam UI connection is unavailable; keep Steam awake and retry")?
        .into_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 1024 * 1024,
        "Steam UI discovery response is too large"
    );
    let pages: Vec<Value> = serde_json::from_slice(&bytes)?;
    let page = pages
        .iter()
        .find(|p| {
            p["title"] == "SharedJSContext"
                && p["url"]
                    .as_str()
                    .is_some_and(|u| u.starts_with("https://steamloopback.host/"))
        })
        .context("Steam library UI is not ready; keep Steam awake and retry")?;
    let address = url::Url::parse(
        page["webSocketDebuggerUrl"]
            .as_str()
            .context("Missing Steam UI endpoint")?,
    )?;
    ensure!(
        address.scheme() == "ws"
            && address.host_str() == Some("127.0.0.1")
            && address.port() == Some(8080)
            && address.path().starts_with("/devtools/page/"),
        "Invalid Steam UI endpoint"
    );
    let stream = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], 8080)),
        Duration::from_secs(3),
    )?;
    // Artwork renders and hashes a 3840x1240 hero inside Steam's CEF context.
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let (mut ws, _) = client(address.as_str(), stream)
        .map_err(|_| anyhow::anyhow!("Cannot connect to Steam UI"))?;
    ws.set_config(|c| {
        *c = WebSocketConfig::default()
            .max_message_size(Some(4 * 1024 * 1024))
            .max_frame_size(Some(4 * 1024 * 1024));
    });
    ws.send(Message::Text(
        json!({"id":1,"method":"Runtime.evaluate","params":{
            "expression":expression,"returnByValue":true,"awaitPromise":true
        }})
        .to_string()
        .into(),
    ))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        ensure!(Instant::now() < deadline, "Steam UI request timed out");
        let message = ws
            .read()
            .map_err(|_| anyhow::anyhow!("Steam UI request failed"))?;
        if let Message::Text(text) = message {
            let v: Value = serde_json::from_str(&text)?;
            if v["id"] != 1 {
                continue;
            }
            // Do not return JavaScript exception descriptions: they can contain launch tokens.
            ensure!(
                v.get("error").is_none() && v["result"].get("exceptionDetails").is_none(),
                "Steam UI operation failed"
            );
            return Ok(v["result"]["result"]["value"].clone());
        }
    }
}

pub(super) fn configure(
    game: &str,
    previous: Option<u32>,
    name: &str,
    icon: Option<&Path>,
    launch: &str,
    executable: &Path,
    package: &str,
) -> Result<u32> {
    use base64::Engine;
    let legacy = legacy_artwork(icon)?;
    let image = icon
        .map(|path| -> Result<String> {
            ensure!(
                fs::metadata(path)?.len() <= 8 * 1024 * 1024,
                "APK icon is too large"
            );
            Ok(format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(fs::read(path)?)
            ))
        })
        .transpose()?;
    let ownership = icon
        .map(|p| load_artwork_ownership(p.parent().context("Missing artwork directory")?))
        .transpose()?
        .unwrap_or(Value::Null);
    let shortcut_icon = icon.map(shortcut_icon_file).transpose()?;
    let parameters = json!({"game":game,"previous":previous,"name":name,
        "icon":shortcut_icon,"launch":launch,"exe":executable,"image":image,"package":package,
        "ownership":ownership,"legacy":legacy});
    let expression = format!(
        r#"(async()=>{{
        const p={parameters};
        const owned=await ({lookup})(p);
        const id=owned.id;
        SteamClient.Apps.SetShortcutLaunchOptions(id,p.launch);
        SteamClient.Apps.SetShortcutName(id,p.name);
        // Existing shortcuts may have a user-selected icon path.
        if(p.icon&&!p.previous) SteamClient.Apps.SetShortcutIcon(id,p.icon);
        const ownership=p.image?await ({apply})(p,id,({render}),({palette})):null;
        return {{id,ownership}};
    }})()"#,
        lookup = include_str!("steam_shortcut_lookup.js"),
        apply = include_str!("steam_artwork_apply.js"),
        render = include_str!("steam_artwork_render.js"),
        palette = include_str!("steam_artwork_palette.js"),
    );
    let result = evaluate(&expression)?;
    // Numeric mock responses remain supported by the existing IPC fixtures.
    let id = result
        .get("id")
        .unwrap_or(&result)
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v >= 0x80000000)
        .context("Invalid Steam APK App ID")?;
    if let (Some(path), Some(ownership)) = (icon, result.get("ownership").filter(|v| !v.is_null()))
    {
        validate_artwork_ownership(ownership)?;
        ensure!(
            ownership["appId"] == id,
            "Artwork ownership does not match shortcut"
        );
        super::steam_shortcuts::write_owned(
            &path
                .parent()
                .context("Missing artwork directory")?
                .join(".framely-artwork.json"),
            &serde_json::to_vec(ownership)?,
            0o600,
        )?;
        ensure!(
            ownership["failed"] != true,
            "Steam artwork update was interrupted; completed images were recorded and will be retried"
        );
    }
    Ok(id)
}

fn validate_artwork_ownership(value: &Value) -> Result<()> {
    ensure!(
        value["version"] == 1
            && value["appId"]
                .as_u64()
                .is_some_and(|id| (0x80000000..=u32::MAX as u64).contains(&id)),
        "Invalid artwork ownership version or app ID"
    );
    let hashes = value["hashes"]
        .as_object()
        .context("Invalid artwork ownership hashes")?;
    ensure!(
        hashes.len() <= 5
            && hashes.iter().all(|(kind, hash)| matches!(
                kind.as_str(),
                "0" | "1" | "2" | "3" | "4"
            ) && hash
                .as_str()
                .is_some_and(|h| h.len() == 64 && h.bytes().all(|c| c.is_ascii_hexdigit()))),
        "Invalid artwork ownership hash"
    );
    Ok(())
}

fn shortcut_icon_file(source: &Path) -> Result<PathBuf> {
    let picture = image::open(source)?.thumbnail(256, 256).to_rgba8();
    let mut icon = image::RgbaImage::new(256, 256);
    image::imageops::overlay(
        &mut icon,
        &picture,
        ((256 - picture.width()) / 2) as i64,
        ((256 - picture.height()) / 2) as i64,
    );
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(icon).write_to(&mut bytes, image::ImageFormat::Png)?;
    let path = source
        .parent()
        .context("Missing icon directory")?
        .join("steam-icon.png");
    super::steam_shortcuts::write_owned(&path, &bytes.into_inner(), 0o600)?;
    Ok(path)
}

fn load_artwork_ownership(dir: &Path) -> Result<Value> {
    let path = dir.join(".framely-artwork.json");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Null),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 8192,
        "Invalid artwork ownership file"
    );
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    validate_artwork_ownership(&value)?;
    Ok(value)
}

// Kept solely to recognize/migrate exact artwork made by earlier releases.
fn legacy_artwork(icon: Option<&Path>) -> Result<Vec<Value>> {
    use base64::Engine;
    let Some(path) = icon else { return Ok(vec![]) };
    let image = image::open(path)?.to_rgba8();
    let mut out = Vec::new();
    for (kind, w, h) in [(0, 600, 900), (1, 1280, 720), (3, 920, 430), (4, 512, 512)] {
        let mut canvas = image::RgbaImage::from_pixel(w, h, image::Rgba([22, 24, 27, 255]));
        let size = if kind == 4 {
            512
        } else {
            (w.min(h) * 3 / 5).min(360)
        };
        let icon = image::imageops::thumbnail(&image, size, size);
        image::imageops::overlay(
            &mut canvas,
            &icon,
            ((w - icon.width()) / 2) as i64,
            ((h - icon.height()) / 2) as i64,
        );
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(canvas).write_to(&mut bytes, image::ImageFormat::Png)?;
        out.push(json!({"kind":kind,"data":base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())}));
    }
    Ok(out)
}

pub(super) fn clear_artwork(app_id: u32, executable: &Path) -> Result<()> {
    ensure!(app_id >= 0x80000000, "Invalid owned Steam shortcut ID");
    let parameters = json!({"id":app_id,"exe":executable});
    evaluate(&format!(
        r#"(async()=>{{
        const p={parameters};
        const app=appStore.m_mapApps.get(p.id);
        if(!app) return;
        let sub;
        const details=await new Promise((resolve,reject)=>{{
            const timer=setTimeout(()=>{{sub?.unregister();reject(Error('Details timed out'))}},3000);
            sub=SteamClient.Apps.RegisterForAppDetails(p.id,d=>{{clearTimeout(timer);sub?.unregister();resolve(d)}});
        }});
        sub?.unregister();
        const exe=details.strShortcutExe?.replace(/^"|"$/g,'').replace('/./','/');
        if(exe!==p.exe) throw Error('Shortcut target does not match owned APK');
        for(const kind of [0,1,2,3]) await SteamClient.Apps.ClearCustomArtworkForApp(p.id,kind);
        await SteamClient.Apps.SetShortcutIcon(p.id,'');
    }})()"#
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_icon_has_its_own_256px_file_and_preserves_source() {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("icon.png");
        image::RgbaImage::from_pixel(128, 64, image::Rgba([230, 30, 20, 255]))
            .save(&source)
            .unwrap();
        let before = fs::read(&source).unwrap();
        let path = shortcut_icon_file(&source).unwrap();
        assert_eq!(path.file_name().unwrap(), "steam-icon.png");
        let picture = image::open(path).unwrap().to_rgba8();
        assert_eq!(picture.dimensions(), (256, 256));
        assert_eq!(picture.get_pixel(0, 0).0[3], 0);
        assert_eq!(picture.get_pixel(128, 128).0, [230, 30, 20, 255]);
        assert_eq!(fs::read(source).unwrap(), before);
    }
    #[test]
    fn steam_artwork_has_matching_aspect_ratios_and_never_crops_icon() {
        use base64::Engine;
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("icon.png");
        image::RgbaImage::from_pixel(128, 64, image::Rgba([230, 30, 20, 255]))
            .save(&p)
            .unwrap();
        let art = legacy_artwork(Some(&p)).unwrap();
        assert_eq!(art.len(), 4);
        for (a, expected) in art
            .iter()
            .zip([(600, 900), (1280, 720), (920, 430), (512, 512)])
        {
            let b = base64::engine::general_purpose::STANDARD
                .decode(a["data"].as_str().unwrap())
                .unwrap();
            let i = image::load_from_memory(&b).unwrap().to_rgba8();
            assert_eq!(i.dimensions(), expected);
            assert_eq!(
                i.get_pixel(expected.0 / 2, expected.1 / 2).0,
                [230, 30, 20, 255]
            );
        }
        assert!(legacy_artwork(None).unwrap().is_empty());
    }

    #[test]
    fn artwork_ownership_is_bounded_and_never_follows_symlinks() {
        let t = tempfile::tempdir().unwrap();
        assert!(load_artwork_ownership(t.path()).unwrap().is_null());
        let valid = json!({"version":1,"appId":0x92345678u32,"hashes":{"0":"a".repeat(64)}});
        let file = t.path().join(".framely-artwork.json");
        fs::write(&file, serde_json::to_vec(&valid).unwrap()).unwrap();
        assert_eq!(load_artwork_ownership(t.path()).unwrap(), valid);
        for invalid in [
            json!({}),
            json!({"version":1,"appId":42,"hashes":{}}),
            json!({"version":1,"appId":0x92345678u32,"hashes":{"5":"a".repeat(64)}}),
            json!({"version":1,"appId":0x92345678u32,"hashes":{"0":"not-a-hash"}}),
        ] {
            fs::write(&file, serde_json::to_vec(&invalid).unwrap()).unwrap();
            assert!(load_artwork_ownership(t.path()).is_err());
        }
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(t.path().join("other"), &file).unwrap();
        assert!(load_artwork_ownership(t.path()).is_err());
    }
}
