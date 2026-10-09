use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    #[default]
    Screen,
    SteamVR,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Output {
    #[default]
    Eye,
    Raw,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Eye {
    #[default]
    SteamVR,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    #[default]
    H264,
    H265,
}
impl Codec {
    pub fn name(self) -> &'static str { match self { Self::H264 => "h264", Self::H265 => "h265" } }
    pub fn rtp(self) -> &'static str { match self { Self::H264 => "H264/90000", Self::H265 => "H265/90000" } }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Settings {
    pub source: Source,
    pub codec: Codec,
    pub output: Output,
    pub eye: Eye,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_mbps: u32,
    /// None selects the widest fully covered field of view for this aspect.
    pub horizontal_fov: Option<f64>,
    pub center_x: f64,
    pub center_y: f64,
    pub system_audio: bool,
    pub microphone: bool,
    pub airplay: bool,
    pub dlna: bool,
    pub receiver_name: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            source: Source::Screen,
            codec: Codec::H264,
            output: Output::Eye,
            eye: Eye::SteamVR,
            width: 1920,
            height: 1080,
            fps: 30,
            bitrate_mbps: 8,
            horizontal_fov: None,
            center_x: 0.,
            center_y: 0.,
            system_audio: true,
            microphone: false,
            airplay: false,
            dlna: false,
            receiver_name: "Framely".into(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (160..=4096).contains(&self.width)
                && self.width % 2 == 0
                && (160..=4096).contains(&self.height)
                && self.height % 2 == 0
                && u64::from(self.width) * u64::from(self.height) <= 8_847_360,
            "输出宽高必须是 160–4096 之间的偶数，且不超过 8847360 像素"
        );
        ensure!((1..=120).contains(&self.fps), "输出帧率必须是 1–120 fps");
        ensure!(
            (1..=80).contains(&self.bitrate_mbps),
            "视频码率必须是 1–80 Mbps"
        );
        ensure!(
            self.horizontal_fov
                .is_none_or(|v| v.is_finite() && (10.0..=150.0).contains(&v)),
            "水平视野必须是 10–150 度"
        );
        ensure!(
            [self.center_x, self.center_y]
                .into_iter()
                .all(|v| v.is_finite() && (-60.0..=60.0).contains(&v)),
            "取景中心必须在 -60–60 度之间"
        );
        ensure!(
            !self.receiver_name.trim().is_empty()
                && self.receiver_name.len() <= 80
                && !self.receiver_name.chars().any(char::is_control),
            "接收名称必须是 1–80 字节且不能包含控制字符"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_settings_keep_h264_and_hevc_is_explicit() {
        let legacy: Settings=serde_json::from_value(serde_json::json!({"source":"screen"})).unwrap();
        assert_eq!(legacy.codec,Codec::H264);
        let hevc: Settings=serde_json::from_value(serde_json::json!({"codec":"h265"})).unwrap();
        assert_eq!(hevc.codec,Codec::H265);
        assert_eq!(serde_json::to_value(hevc).unwrap()["codec"],"h265");
        assert!(serde_json::from_value::<Settings>(serde_json::json!({"codec":"av1"})).is_err());
    }
    #[test]
    fn settings_reject_unsafe_media_sizes_and_invalid_ranges() {
        let mut s = Settings::default();
        s.validate().unwrap();
        s.width = 4096;
        s.height = 4096;
        assert!(s.validate().is_err());
        s = Settings::default();
        s.horizontal_fov = Some(f64::NAN);
        assert!(s.validate().is_err());
        s = Settings::default();
        s.receiver_name = "Framely\nInjected".into();
        assert!(s.validate().is_err());
        assert!(
            serde_json::from_value::<Settings>(serde_json::json!({"source":"automatic"})).is_err()
        );
    }
}
