use super::{
    runtime::Casting,
    settings::{Eye, Settings},
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "displays calibration markers on Steam Frame; requires explicit user approval"]
fn recording_region_device_alignment() {
    let media = PathBuf::from(std::env::var("FRAMELY_REGION_TEST_MEDIA").unwrap());
    let marker = std::env::var("FRAMELY_REGION_TEST_MARKER").unwrap();
    let directory = tempfile::tempdir().unwrap();
    for (eye, width, height, fov, cx, cy) in [
        (Eye::Left, 640, 480, None, 0., 0.),
        (Eye::Right, 640, 360, Some(55.), 8., -5.),
        (Eye::Left, 360, 640, Some(45.), -6., 4.),
        (Eye::Right, 1920, 1440, Some(70.), 0., 0.),
        (Eye::Right, 2560, 1600, Some(76.47219658546469), 0., -1.504517427660815),
        (Eye::Left, 2560, 1600, Some(76.47219658546469), 0., -1.504517427660815),
    ] {
        let mut command = Command::new(media.join("framely-capture"));
        command.args([
            "--describe-view",
            "--eye",
            if eye == Eye::Left { "0" } else { "1" },
            "--width",
            &width.to_string(),
            "--height",
            &height.to_string(),
        ]);
        command.args([format!("--center-x={cx}"), format!("--center-y={cy}")]);
        if let Some(fov) = fov {
            command.arg(format!("--fov={fov}"));
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut view: Value = serde_json::from_slice(&output.stdout).unwrap();
        super::region::validate_description(&view).unwrap();
        view["width"] = width.into();
        view["height"] = height.into();
        let geometry = directory.path().join("geometry.json");
        let ready = directory.path().join("ready");
        let stop = directory.path().join("stop");
        let _ = fs::remove_file(&ready);
        let _ = fs::remove_file(&stop);
        fs::write(&geometry, view.to_string()).unwrap();
        let mut overlay = Command::new(&marker)
            .arg(&geometry)
            .arg(&ready)
            .arg(&stop)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(overlay.try_wait().unwrap().is_none());
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(20));
        }
        let helper = crate::ipc::call(
            Path::new("/run/framely/control.sock"),
            "cast.panel.start",
            serde_json::json!({}),
        )
        .unwrap();
        let socket = PathBuf::from(helper["socket"].as_str().unwrap());
        let mut cast = Casting::new(directory.path().join("capture"), media.clone());
        let settings = Settings {
            eye,
            width,
            height,
            horizontal_fov: fov,
            center_x: cx,
            center_y: cy,
            system_audio: false,
            ..Settings::default()
        };
        let result = (|| -> anyhow::Result<Vec<u8>> {
            let status=cast.start(&settings, Some(&socket))?;
            super::region::validate_description(&status["view"])?;
            for key in ["tanHalfHorizontal","centerTanX","centerTanY","aspect"] {
                anyhow::ensure!((status["view"][key].as_f64().unwrap()-view[key].as_f64().unwrap()).abs()<1e-10,"described and active capture differ: {key}");
            }
            let frame = Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-rtsp_transport",
                    "tcp",
                    "-i",
                    &format!("rtsp://127.0.0.1:{}/headset", cast.test_rtsp_port()),
                    "-frames:v",
                    "1",
                    "-pix_fmt",
                    "rgb24",
                    "-f",
                    "rawvideo",
                    "pipe:1",
                ])
                .output()?;
            anyhow::ensure!(
                frame.status.success(),
                "{}",
                String::from_utf8_lossy(&frame.stderr)
            );
            Ok(frame.stdout)
        })();
        cast.stop();
        fs::write(&stop, b"stop").unwrap();
        let _ = overlay.wait();
        let rgb = result.unwrap();
        assert_eq!(rgb.len(), width as usize * height as usize * 3);
        let positions = [
            (16, 16),
            (width - 16, 16),
            (16, height - 16),
            (width - 16, height - 16),
            (width / 2, height / 2),
        ];
        for (index, (x, y)) in positions.into_iter().enumerate() {
            let mut n = 0.;
            let mut sx = 0.;
            let mut sy = 0.;
            for dy in -24..24 {
                for dx in -24..24 {
                    let px = x as i32 + dx;
                    let py = y as i32 + dy;
                    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 { continue; }
                    let i = (py as usize * width as usize + px as usize) * 3;
                    let (r, g, b) = (rgb[i], rgb[i + 1], rgb[i + 2]);
                    let hit = match index {
                        0 => r > 160 && g < 100 && b < 100,
                        1 => g > 160 && r < 100 && b < 100,
                        2 => b > 160 && r < 100 && g < 100,
                        3 => r > 160 && b > 160 && g < 100,
                        _ => r > 200 && g > 200 && b > 200,
                    };
                    if hit {
                        n += 1.;
                        sx += px as f64;
                        sy += py as f64;
                    }
                }
            }
            assert!(n > 400., "fiducial {index} missing for {eye:?}: {n} pixels");
            let error = (sx / n - (x as f64 - 0.5))
                .abs()
                .max((sy / n - (y as f64 - 0.5)).abs());
            println!(
                "ALIGNMENT {eye:?} {width}x{height} marker={index} error={error:.2}px pixels={n}"
            );
            assert!(
                error <= 3.,
                "recording border does not match sampled image: {error:.2}px"
            );
        }
    }
}
