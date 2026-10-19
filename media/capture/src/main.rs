//! Framely's panel capture worker. stdout is Annex-B H.264, stderr is logging.
mod clock;
mod encoder;
mod gpu;
mod kms;
mod lut;
mod panel_geometry;

use anyhow::{Context, Result};
use clap::Parser;
use framely_capture::openvr::OpenVr;
use std::{io::{self, Read, Write}, os::fd::AsRawFd, path::Path, time::{Duration, Instant}};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value_t = 1920)] width: u32,
    #[arg(long, default_value_t = 1440)] height: u32,
    #[arg(long, default_value_t = 30)] fps: u32,
    #[arg(long, default_value_t = 8)] bitrate: u32,
    #[arg(long, default_value_t = 0)] eye: usize,
    #[arg(long)] raw: bool,
    #[arg(long)] describe_view: bool,
    /// Emit one native-resolution RGBA frame (width, height, then pixels).
    #[arg(long)] snapshot: bool,
    /// Prepare the snapshot pipeline, then wait for a byte on stdin before sampling.
    #[arg(long)] snapshot_wait: bool,
    #[arg(long, value_enum, default_value_t = encoder::Codec::H264)] codec: encoder::Codec,
    #[arg(long)] fov: Option<f64>,
    #[arg(long, default_value_t = 0.)] center_x: f64,
    #[arg(long, default_value_t = 0.)] center_y: f64,
}
fn run(a: Args) -> Result<()> {
    anyhow::ensure!(a.width >= 160 && a.width <= 4096 && a.width % 2 == 0
        && a.height >= 160 && a.height <= 4096 && a.height % 2 == 0
        && a.width as u64 * a.height as u64 <= 8_847_360
        && (1..=120).contains(&a.fps) && (1..=80).contains(&a.bitrate) && a.eye <= 1,
        "Invalid output dimensions, rate or eye");
    anyhow::ensure!(a.fov.is_none_or(|v| v.is_finite() && (10.0..=150.0).contains(&v))
        && [a.center_x, a.center_y].into_iter().all(|v| v.is_finite() && (-60.0..=60.0).contains(&v)), "Invalid field of view");
    anyhow::ensure!(!a.describe_view || !a.raw, "Raw view has no single-eye region");
    anyhow::ensure!(!a.snapshot || !a.describe_view, "Choose one capture mode");
    anyhow::ensure!(!a.snapshot_wait || a.snapshot, "Snapshot wait requires snapshot mode");
    if a.snapshot { return snapshot(&a); }
    let kms = if a.describe_view {None} else {Some(kms::Kms::open(Path::new("/dev/dri/card0"))?)};
    let height = if a.raw { let kms=kms.as_ref().unwrap(); ((a.width as f64 * kms.mode_size.1 as f64 / kms.mode_size.0 as f64).round() as u32 + 1) & !1 } else { a.height };
    anyhow::ensure!(height>=160&&height<=4096&&u64::from(a.width)*u64::from(height)<=8_847_360,"Raw output exceeds supported dimensions");
    let map = if a.raw { let kms=kms.as_ref().unwrap(); lut::raw(a.width, height, kms.mode_size.0, kms.mode_size.1) }
        else { lut::undistorted(&OpenVr::connect()?, a.eye, a.fov, a.width, height, a.center_x, a.center_y)? };
    if let Some((tan_h, cx, cy)) = map.view {
        let description=serde_json::json!({"event":"view","tanHalfHorizontal":tan_h,"centerTanX":cx,"centerTanY":cy,"aspect":a.width as f64/height as f64,"captureToHead":map.capture_to_head,"eye":a.eye,"projection":"factory-panel-v1","regionFullyVisible":map.region_fully_visible,"editorReferenceTan":map.editor_reference_tan,"visibility":map.visibility});
        if a.describe_view { anyhow::ensure!(map.capture_to_head.is_some(),"SteamVR cannot provide an accurate eye transform");println!("{description}"); return Ok(()); }
        eprintln!("{description}");
    }
    let kms=kms.unwrap();
    let mut enc = encoder::Encoder::open(&encoder::find_device()?, encoder::Config {
        codec: a.codec, width: a.width, height, fps: a.fps,
        bitrate: a.bitrate * 1_000_000, qp: None,
    }, 3)?;
    let filter = if map.supersample { gpu::Filter::Supersample } else { gpu::Filter::Sharp };
    let mut gpu = gpu::Gpu::new(enc.layout(), 3, &map, filter, gpu::Priority::Low, None)?;
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut origin = None;
    let mut next = Duration::ZERO;
    let interval = Duration::from_secs_f64(1. / a.fps as f64);
    let tolerance = Duration::from_secs_f64(0.5 / kms.refresh_hz);
    let mut frames = 0u64;
    let mut last_status = Instant::now();
    loop {
        let time = kms.wait_vblank().context("Headset display is unavailable")?;
        let first = *origin.get_or_insert(time);
        let elapsed = time.saturating_sub(first);
        if elapsed + tolerance < next { continue; }
        // Keep a stable cadence when output FPS is not a divisor of display refresh.
        while next <= elapsed + tolerance { next += interval; }
        let Some(fb) = kms.current_fb()? else { continue; };
        let buf = kms.export(fb)?;
        kms::wait_idle(&buf.fd, Duration::from_millis(4));
        let id = kms::buffer_id(&buf.fd)?;
        if !gpu.has_source(fb, id) { gpu.add_source(buf, id)?; }
        if let Some(slot) = enc.free_slot()? {
            gpu.convert(fb, id, slot)?;
            enc.queue_frame(slot, gpu.targets()[slot].fd.as_raw_fd(), elapsed.as_micros() as u64)?;
            frames += 1;
        }
        let mut error = None;
        enc.poll_packets(|packet| {
            if let Err(e) = output.write_all(packet.data).and_then(|_| output.flush()) { error = Some(e); }
        })?;
        if let Some(e) = error { return Err(e.into()); }
        if frames == 1 || last_status.elapsed() > Duration::from_secs(5) {
            eprintln!("{}", serde_json::json!({"event":"capture","width":a.width,"height":height,"frames":frames}));
            last_status = Instant::now();
        }
    }
}

fn snapshot(a: &Args) -> Result<()> {
    let kms = kms::Kms::open(Path::new("/dev/dri/card0"))?;
    let (width, height, map) = if a.raw {
        let (w, h) = kms.mode_size;
        (w, h, lut::raw(w, h, w, h))
    } else {
        let aspect = a.width as f64 / a.height as f64;
        let panel_w = kms.mode_size.0 / 2;
        let panel_h = kms.mode_size.1;
        let max_w = (panel_h as f64 * aspect).round().min(panel_w as f64) as u32;
        let max_h = (max_w as f64 / aspect).round().min(panel_h as f64) as u32;
        let base = lut::undistorted(&OpenVr::connect()?, a.eye, None, max_w, max_h, a.center_x, a.center_y)?;
        let full_tan = base.view.context("No eye projection")?.0;
        let selected_tan = a.fov.map(|f| (f.to_radians() / 2.0).tan()).unwrap_or(full_tan);
        let fraction = (selected_tan / full_tan).clamp(0.01, 1.0);
        let width = ((max_w as f64 * fraction).round() as u32).max(2);
        let height = ((max_h as f64 * fraction).round() as u32).max(2);
        (width, height, lut::undistorted(&OpenVr::connect()?, a.eye, a.fov, width, height, a.center_x, a.center_y)?)
    };
    anyhow::ensure!(width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 9_331_200, "Invalid native snapshot size");
    let mut gpu = gpu::Gpu::new_snapshot(width, height, &map)?;
    if a.snapshot_wait {
        let mut trigger = [0u8; 1];
        io::stdin().read_exact(&mut trigger).context("Snapshot was cancelled")?;
    }
    // The editor overlay has been closed by the caller. Skip one complete
    // scanout before choosing the frame so its pixels cannot enter the PNG.
    kms.wait_vblank()?;
    kms.wait_vblank()?;
    let fb = kms.current_fb()?.context("Headset display is unavailable")?;
    let buf = kms.export(fb)?;
    kms::wait_idle(&buf.fd, Duration::from_millis(4));
    let id = kms::buffer_id(&buf.fd)?;
    gpu.add_source(buf, id)?;
    gpu.convert(fb, id, 0)?;
    let pixels = gpu.snapshot_pixels()?;
    let mut output = io::BufWriter::new(io::stdout().lock());
    output.write_all(&width.to_le_bytes())?;
    output.write_all(&height.to_le_bytes())?;
    output.write_all(&pixels)?;
    output.flush()?;
    Ok(())
}
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if let Err(e) = run(Args::parse()) { eprintln!("{e:#}"); std::process::exit(1); }
}
