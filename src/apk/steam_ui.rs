//! Local Steam UI API: metadata and launch options, never edits live VDF files.
use super::*;
use std::{
    io::Read,
    net::{SocketAddr, TcpStream},
};
use tungstenite::{client, protocol::WebSocketConfig, Message};

#[cfg(test)]
thread_local! {pub(super) static TEST_RESULT: std::cell::RefCell<Option<Value>>= const {std::cell::RefCell::new(None)};}
fn evaluate(expression: &str) -> Result<Value> {
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
    stream.set_read_timeout(Some(Duration::from_secs(6)))?;
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
    let deadline = Instant::now() + Duration::from_secs(8);
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
                "Steam could not configure the APK entry"
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
) -> Result<u32> {
    let art = artwork(icon)?;
    let parameters = json!({"game":game,"previous":previous,"name":name,
        "icon":icon,"launch":launch,"exe":executable,"art":art});
    let expression = format!(
        r#"(async()=>{{
        const p={parameters};
        let app;
        for(let n=0;n<10;n++){{
            const apps=Array.from(appStore.m_mapApps.values());
            const matches=apps.filter(a=>a.display_name==='Devkit Game: '+p.game);
            app=matches.length===1?matches[0]:apps.find(a=>a.appid===p.previous);
            if(app) break;
            await new Promise(r=>setTimeout(r,100));
        }}
        if(!app || app.appid<2147483648) throw Error('Missing owned shortcut');
        const id=app.appid;
        let sub;
        const details=await new Promise((resolve,reject)=>{{
            let timer=setTimeout(()=>{{sub?.unregister();reject(Error('Details timed out'))}},3000);
            sub=SteamClient.Apps.RegisterForAppDetails(id,d=>{{clearTimeout(timer);sub?.unregister();resolve(d)}});
        }});
        sub?.unregister();
        const exe=details.strShortcutExe?.replace(/^"|"$/g,'').replace('/./','/');
        if(exe!==p.exe) throw Error('Shortcut target does not match owned APK');
        SteamClient.Apps.SetShortcutLaunchOptions(id,p.launch);
        SteamClient.Apps.SetShortcutName(id,p.name);
        if(p.icon) SteamClient.Apps.SetShortcutIcon(id,p.icon);
        for(const a of p.art){{
            try{{await SteamClient.Apps.SetCustomArtworkForApp(id,a.data,'png',a.kind)}}
            catch(e){{
                await new Promise(r=>setTimeout(r,200));
                await SteamClient.Apps.SetCustomArtworkForApp(id,a.data,'png',a.kind);
            }}
        }}
        return id;
    }})()"#
    );
    let result = evaluate(&expression)?;
    result
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v >= 0x80000000)
        .context("Invalid Steam APK App ID")
}

fn artwork(icon: Option<&Path>) -> Result<Vec<Value>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn steam_artwork_has_matching_aspect_ratios_and_never_crops_icon() {
        use base64::Engine;
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("icon.png");
        image::RgbaImage::from_pixel(128, 64, image::Rgba([230, 30, 20, 255]))
            .save(&p)
            .unwrap();
        let art = artwork(Some(&p)).unwrap();
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
        assert!(artwork(None).unwrap().is_empty());
    }
}
