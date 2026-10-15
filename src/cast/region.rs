//! Projection metadata comes from the exact LUT used by panel capture.
use super::settings::{Eye, Output, Settings, Source};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Region {
    pub editing: bool,
    pub editor_id: String,
    enabled: bool,
    settings: Option<Settings>,
    description: Option<Value>,
    pub error: Option<String>,
}
pub fn supported(settings: &Settings) -> bool {
    settings.source == Source::Screen && settings.output == Output::Eye
}
pub fn validate_description(value: &Value) -> Result<()> {
    ensure!(value["projection"]=="factory-panel-v1" && matches!(value["eye"].as_u64(),Some(0|1)), "缺少有效的面板校准数据，无法显示准确录制区域");
    let tan = value["tanHalfHorizontal"]
        .as_f64()
        .context("Missing capture FOV")?;
    let aspect = value["aspect"].as_f64().context("Missing capture aspect")?;
    ensure!(
        tan.is_finite() && tan > 0. && aspect.is_finite() && aspect > 0.,
        "Invalid capture view"
    );
    for name in ["centerTanX", "centerTanY"] {
        ensure!(
            value[name].as_f64().is_some_and(f64::is_finite),
            "Invalid capture center"
        );
    }
    let matrix: [[f64; 4]; 3] = serde_json::from_value(value["captureToHead"].clone())?;
    ensure!(
        matrix.iter().flatten().all(|v| v.is_finite()),
        "Invalid capture transform"
    );
    for i in 0..3 {
        for j in 0..3 {
            let dot: f64 = (0..3).map(|k| matrix[i][k] * matrix[j][k]).sum();
            ensure!(
                (dot - if i == j { 1. } else { 0. }).abs() < 0.01,
                "Invalid capture basis"
            );
        }
    }
    Ok(())
}
impl Region {
    /// Last editor identity remains available after closing so the native host
    /// can reconcile it on every poll, even if a one-shot close command is lost.
    pub fn editor_state(&self) -> Value {
        json!({"id":self.editor_id,"open":self.editing})
    }
    pub fn acknowledge_editor_closed(&mut self, id: &str) {
        // A delayed acknowledgement for the previous window must not close a
        // newer editor that has already been opened.
        if self.editor_id == id {
            self.editing = false;
        }
    }
    pub fn require_visible(view: &Value) -> Result<()> {
        ensure!(view["regionFullyVisible"] == true, "取景范围超出头显可见区域，请缩小或调整位置");
        Ok(())
    }
    pub fn invalidate(&mut self) {
        self.description = None;
        self.settings = None;
    }
    pub fn refresh(
        &mut self,
        settings: &Settings,
        binaries: &Path,
        active: Option<Value>,
    ) -> Result<()> {
        self.enabled = settings.recording_region_visible;
        if !supported(settings) || (!self.enabled && !self.editing) || (active.is_none() && !self.editing) {
            self.description = None;
            self.settings = None;
            self.error = None;
            return Ok(());
        }
        if let Some(value) = active {
            // Never substitute configured geometry for a live recording.
            validate_description(&value)?;
            if !self.editing { Self::require_visible(&value)?; }
            self.description = Some(value);
            self.settings = Some(settings.clone());
            self.error = None;
            return Ok(());
        }
        let eye = match settings.eye {
            Eye::Left => 0,
            Eye::Right => 1,
            Eye::SteamVR => super::runtime::dominant_eye()?,
        };
        if self
            .settings
            .as_ref()
            .is_some_and(|s| s.same_capture(settings))
            && self.description.as_ref().is_some_and(|d| d["eye"] == eye)
        {
            if !self.editing { Self::require_visible(self.description.as_ref().unwrap())?; }
            return Ok(());
        }
        self.description = None;
        self.settings = None;
        let mut command = Command::new(binaries.join("framely-capture"));
        command.args([
            "--describe-view",
            "--width",
            &settings.width.to_string(),
            "--height",
            &settings.height.to_string(),
            "--eye",
            &eye.to_string(),
        ]);
        command.args([
            format!("--center-x={}", settings.center_x),
            format!("--center-y={}", settings.center_y),
        ]);
        if let Some(fov) = settings.horizontal_fov {
            command.arg(format!("--fov={fov}"));
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("无法读取实际录制区域，请更新采集程序")?;
        // Drain metadata while the worker runs: the visibility grid can be
        // larger than a small pipe's capacity. Waiting first would deadlock.
        let stdout=child.stdout.take().context("Missing geometry output")?;
        let output=std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut bytes=Vec::new();stdout.take(65537).read_to_end(&mut bytes)?;Ok(bytes)
        });
        let deadline = Instant::now() + Duration::from_secs(12);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status))=>break status,
                Ok(None)=>{},
                Err(error)=>{let _=child.kill();let _=child.wait();let _=output.join();return Err(error.into());}
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = output.join();
                anyhow::bail!("读取录制区域超时");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let bytes=output.join().map_err(|_|anyhow::anyhow!("Invalid geometry output reader"))??;
        ensure!(
            status.success(),
            "无法读取实际录制区域，请确认 SteamVR 已启动"
        );
        ensure!(bytes.len() <= 65536, "Invalid capture geometry output");
        let value: Value = serde_json::from_slice(&bytes)?;
        validate_description(&value)?;
        if !self.editing { Self::require_visible(&value)?; }
        self.description = Some(value);
        self.settings = Some(settings.clone());
        self.error = None;
        Ok(())
    }
    pub fn fail(&mut self, error: &anyhow::Error) {
        self.description = None;
        self.error = Some(format!("{error:#}"));
    }
    pub fn snapshot(&self, active:bool) -> Value {
        json!({"visible":active && self.enabled && !self.editing && self.description.is_some(),"editing":self.editing,"view":self.description,"settings":self.settings,"error":self.error})
    }
    pub fn editor(&self) -> Result<Value> {
        let view = self
            .description
            .as_ref()
            .context("无法读取实际录制区域，请确认 SteamVR 已启动")?;
        let reference=view["editorReferenceTan"].as_f64().filter(|v|v.is_finite()&&*v>0.).context("无法读取实际录制区域，请更新采集程序")?;
        let visibility=&view["visibility"];
        let n=visibility["size"].as_u64().filter(|n|(8..=256).contains(n)&&n%4==0).context("无法读取实际录制区域，请更新采集程序")?;
        ensure!(visibility["extent"].as_f64()==Some(reference) && visibility["cells"].as_str().is_some_and(|s|s.len()==(n*n/4) as usize&&s.bytes().all(|c|c.is_ascii_hexdigit())),"无法读取实际录制区域，请更新采集程序");
        Ok(
            json!({"id":self.editor_id,"settings":self.settings,"view":view,"referenceTan":reference}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recording_region_editor_close_state_survives_poll_and_rejects_stale_ack() {
        let mut region=Region::default();
        region.editor_id="first".into();region.editing=true;
        assert_eq!(region.editor_state(),json!({"id":"first","open":true}));
        region.acknowledge_editor_closed("first");
        assert_eq!(region.editor_state(),json!({"id":"first","open":false}));
        // Reading a state response must not consume the required close state.
        assert_eq!(region.editor_state(),json!({"id":"first","open":false}));
        region.editor_id="second".into();region.editing=true;
        region.acknowledge_editor_closed("first");
        assert_eq!(region.editor_state(),json!({"id":"second","open":true}));
        region.acknowledge_editor_closed("second");
        assert!(!region.editing);
    }
    #[test]
    fn recording_region_drains_large_metadata_before_waiting() {
        use std::os::unix::fs::PermissionsExt;
        let directory=tempfile::tempdir().unwrap();
        let view=json!({"projection":"factory-panel-v1","eye":0,"regionFullyVisible":true,"editorReferenceTan":1.7,"visibility":{"size":8,"extent":1.7,"cells":"f".repeat(16)},"tanHalfHorizontal":0.5,"aspect":4./3.,"centerTanX":0.,"centerTanY":0.,"captureToHead":[[1,0,0,0],[0,1,0,0],[0,0,1,0]],"padding":"x".repeat(48000)});
        let worker=directory.path().join("framely-capture");
        std::fs::write(&worker,format!("#!/bin/sh\nprintf '%s' '{}'\n",view)).unwrap();
        std::fs::set_permissions(&worker,std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut region=Region::default();region.editing=true;
        let started=Instant::now();
        region.refresh(&Settings {eye:Eye::Left,..Settings::default()},directory.path(),None).unwrap();
        assert!(started.elapsed()<Duration::from_secs(3));
        assert_eq!(region.editor().unwrap()["referenceTan"],1.7);
    }
    #[test]
    fn recording_region_visibility_persists_independently_of_capture() {
        let root=tempfile::tempdir().unwrap();
        let mut core=crate::tests::accepted_service(root.path(),1000).unwrap();
        let before=core.handle("status",json!({})).unwrap()["database"]["casting"].clone();
        core.handle("cast.region.visible.save",json!({"enabled":true})).unwrap();
        let mut enabled=core.handle("status",json!({})).unwrap()["database"]["casting"].clone();
        assert_eq!(enabled["recordingRegionVisible"],true);enabled["recordingRegionVisible"]=json!(false);assert_eq!(enabled,before);
        drop(core);
        let mut restored=crate::service::Service::load(root.path(),1000).unwrap();
        assert_eq!(restored.handle("status",json!({})).unwrap()["database"]["casting"]["recordingRegionVisible"],true);
        assert!(!serde_json::from_value::<Settings>(json!({})).unwrap().recording_region_visible);
    }
    #[test]
    fn recording_region_rejects_missing_invalid_or_stale_geometry() {
        let view = json!({"projection":"factory-panel-v1","eye":0,"regionFullyVisible":true,"editorReferenceTan":1.7,"visibility":{"size":8,"extent":1.7,"cells":"f".repeat(16)},"tanHalfHorizontal":1.,"aspect":4./3.,"centerTanX":0.1,"centerTanY":-0.1,"captureToHead":[[1,0,0,-0.032],[0,1,0,0],[0,0,1,0]]});
        assert!(Region::require_visible(&json!({"regionFullyVisible":false})).is_err());
        assert!(Region::require_visible(&json!({})).is_err());
        validate_description(&view).unwrap();
        let mut region = Region::default();
        let mut settings = Settings::default();
        settings.recording_region_visible = true;
        region
            .refresh(&settings, Path::new("/missing"), Some(view))
            .unwrap();
        assert_eq!(region.snapshot(true)["visible"], true);
        assert_eq!(region.editor().unwrap()["referenceTan"],1.7);
        let mut smaller=region.description.clone().unwrap();smaller["tanHalfHorizontal"]=json!(0.1);
        region.refresh(&settings,Path::new("/missing"),Some(smaller)).unwrap();
        assert_eq!(region.editor().unwrap()["referenceTan"],1.7);
        assert_eq!(region.snapshot(false)["visible"], false);
        region.refresh(&settings,Path::new("/missing"),None).unwrap();
        assert_eq!(region.snapshot(true)["visible"], false);
        assert!(region
            .refresh(&settings, Path::new("/missing"), Some(json!({})))
            .is_err());
        region.fail(&anyhow::anyhow!("invalid"));
        assert_eq!(region.snapshot(true)["visible"], false);
        settings.output = Output::Raw;
        region
            .refresh(&settings, Path::new("/missing"), None)
            .unwrap();
        assert_eq!(region.snapshot(true)["visible"], false);
    }
}
