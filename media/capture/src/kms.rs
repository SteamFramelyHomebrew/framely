//! Finds the plane the VR compositor scans out and hands its buffers over as
//! dmabufs. The compositor keeps DRM master; we only look.
//!
//! Getting buffer handles for someone else's framebuffer needs CAP_SYS_ADMIN.
//! The recorder uses it if it has it itself, the panel helper otherwise (see
//! framely_capture::grab).

use std::fs::{File, OpenOptions};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use drm::control::{crtc, plane, Device as ControlDevice};
use framely_capture::grab;
use drm::{ClientCapability, Device, VblankWaitFlags, VblankWaitTarget};

struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl Device for Card {}
impl ControlDevice for Card {}

/// One scanout buffer, exported as a dmabuf.
pub struct ScanoutBuffer {
    pub fb_id: u32,
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    pub fourcc: u32,
    pub modifier: u64,
    pub pitch: u32,
    pub offset: u32,
}

pub struct Kms {
    card: Card,
    helper: RefCell<Option<grab::Grabber>>,
    pipe: u32,
    plane: Cell<plane::Handle>,
    crtc: crtc::Handle,
    next_plane_probe: Cell<Instant>,
    pub refresh_hz: f64,
    pub mode_size: (u32, u32),
}

impl Kms {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        let card = Card(file);
        card.set_client_capability(ClientCapability::UniversalPlanes, true)
            .context("enabling universal planes")?;

        let res = card.resource_handles().context("reading DRM resources")?;
        let (pipe, crtc, mode) = res
            .crtcs()
            .iter()
            .enumerate()
            .find_map(|(i, &h)| {
                let info = card.get_crtc(h).ok()?;
                Some((i as u32, h, info.mode()?))
            })
            .context("no active display found, is the headset on?")?;

        let plane = find_scanout_plane(&card, crtc)?;
        let (w, h) = mode.size();
        let refresh_hz = mode_refresh(&mode);
        log::info!(
            "display: {}x{} @ {:.2} Hz, crtc {:?}, plane {:?}",
            w,
            h,
            refresh_hz,
            crtc,
            plane
        );

        Ok(Self {
            card,
            helper: RefCell::new(grab::Grabber::inherited()?),
            pipe,
            plane: Cell::new(plane),
            crtc,
            next_plane_probe: Cell::new(Instant::now()),
            refresh_hz,
            mode_size: (w as u32, h as u32),
        })
    }

    /// Blocks until the next vblank and returns its CLOCK_MONOTONIC timestamp.
    pub fn wait_vblank(&self) -> Result<Duration> {
        let reply = loop {
            match self.card.wait_vblank(VblankWaitTarget::Relative(1), VblankWaitFlags::empty(), self.pipe, 0) {
                // A pause or stop signal landed mid-wait; just wait again.
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                other => break other.context("waiting for vblank")?,
            }
        };
        Ok(reply.time().unwrap_or_else(crate::clock::now))
    }

    /// Id of the framebuffer being scanned out right now, if any.
    pub fn current_fb(&self) -> Result<Option<u32>> {
        let info = self.card.get_plane(self.plane.get()).context("reading scanout plane")?;
        if info.crtc() == Some(self.crtc) && info.framebuffer().is_some() {
            return Ok(info.framebuffer().map(u32::from));
        }
        // SteamVR can move scanout to another plane when waking or restarting.
        // Keep normal capture cheap, and bound probes while the display is off.
        if Instant::now() < self.next_plane_probe.get() { return Ok(None); }
        self.next_plane_probe.set(Instant::now() + Duration::from_secs(1));
        let Ok(plane) = find_scanout_plane(&self.card, self.crtc) else { return Ok(None) };
        if plane != self.plane.get() {
            log::info!("scanout moved to plane {plane:?}");
            self.plane.set(plane);
        }
        Ok(self.card.get_plane(plane).context("reading replacement scanout plane")?.framebuffer().map(u32::from))
    }

    pub fn export(&self, fb_id: u32) -> Result<ScanoutBuffer> {
        let buf = match grab::export(&self.card, fb_id)? {
            Some(buf) => buf,
            // No permission of our own: the helper has it, if it's installed.
            None => self.export_through_helper(fb_id)?,
        };
        Ok(ScanoutBuffer {
            fb_id,
            fd: buf.fd,
            width: buf.width,
            height: buf.height,
            fourcc: buf.fourcc,
            modifier: buf.modifier,
            pitch: buf.pitch,
            offset: buf.offset,
        })
    }

    fn export_through_helper(&self, fb_id: u32) -> Result<grab::Exported> {
        self.helper.borrow_mut().as_mut()
            .context("Framely panel helper is not connected; start capture from Framely's casting page")?
            .export(fb_id)
    }

}

fn find_scanout_plane(card: &Card, crtc: crtc::Handle) -> Result<plane::Handle> {
    let mut best: Option<(plane::Handle, u64)> = None;
    for handle in card.plane_handles().context("listing planes")? {
        let Ok(info) = card.get_plane(handle) else { continue };
        if info.crtc() != Some(crtc) {
            continue;
        }
        let Some(fb) = info.framebuffer() else { continue };
        // GETFB2 exposes dimensions even when the unprivileged recorder cannot
        // see GEM handles. Legacy GETFB can fail on modern scanout formats.
        // If it fails for every plane, choosing the first one can select a cursor.
        let props = card.get_properties(handle).ok();
        let mut cursor = false;
        let (mut width, mut height) = (0, 0);
        if let Some(props) = props {
            for (&property, &value) in props.iter() {
                let Ok(info) = card.get_property(property) else { continue };
                match info.name().to_bytes() {
                    b"type" => cursor = value == 2, // DRM_PLANE_TYPE_CURSOR
                    b"CRTC_W" => width = value,
                    b"CRTC_H" => height = value,
                    _ => {}
                }
            }
        }
        let size = card.get_planar_framebuffer(fb).map(|f| f.size())
            .or_else(|_| card.get_framebuffer(fb).map(|f| f.size()));
        let area = size.map(|(w,h)| w as u64 * h as u64).unwrap_or(width.saturating_mul(height));
        if cursor || area == 0 { continue; }
        if best.is_none_or(|(_, a)| area > a) {
            best = Some((handle, area));
        }
    }
    best.map(|(h, _)| h).context("no plane is scanning out, is the VR compositor running?")
}

fn mode_refresh(mode: &drm::control::Mode) -> f64 {
    let (htotal, vtotal) = (mode.hsync().2 as f64, mode.vsync().2 as f64);
    if htotal > 0.0 && vtotal > 0.0 {
        mode.clock() as f64 * 1000.0 / (htotal * vtotal)
    } else {
        mode.vrefresh() as f64
    }
}

/// Identifies the buffer behind a dmabuf fd. Every export of the same buffer
/// shares one dma_buf file, so its inode is stable for the buffer's lifetime.
pub fn buffer_id(fd: &OwnedFd) -> Result<u64> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(std::io::Error::last_os_error()).context("looking at a scanout buffer");
    }
    Ok(st.st_ino)
}

/// Waits (briefly) until nobody is still drawing into the buffer.
pub fn wait_idle(fd: &OwnedFd, timeout: Duration) {
    let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut pfd, 1, timeout.as_millis() as libc::c_int) };
}
