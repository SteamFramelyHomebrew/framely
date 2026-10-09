//! Framely's root framebuffer export helper and its private session socket.
//! The worker holds no capabilities. The core starts a version-matched helper,
//! which exports framebuffer DMA-BUF handles until the session socket closes.

use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;

use anyhow::{bail, Context, Result};
use drm::control::{framebuffer, Device as ControlDevice};

/// IPC protocol shared by the packaged helper and capture worker.
pub const VERSION: u32 = 1;
const REPLY_LEN: usize = 32;

/// One scanout buffer, as a dmabuf.
pub struct Exported {
    pub fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    pub fourcc: u32,
    pub modifier: u64,
    pub pitch: u32,
    pub offset: u32,
}

/// Exports a framebuffer, which only works with the permission. None when
/// the kernel kept the buffer to itself, i.e. without it.
pub fn export<D: ControlDevice>(card: &D, fb_id: u32) -> Result<Option<Exported>> {
    let handle: framebuffer::Handle = drm::control::from_u32(fb_id).context("bad framebuffer id")?;
    let info = card.get_planar_framebuffer(handle).with_context(|| format!("reading framebuffer {fb_id}"))?;
    let buffers = info.buffers();
    let Some(bo) = buffers[0] else { return Ok(None) };
    if buffers[1..].iter().any(Option::is_some) {
        bail!("multi-plane scanout buffers aren't supported yet");
    }
    let fd = card.buffer_to_prime_fd(bo, libc::O_CLOEXEC as u32).context("exporting scanout buffer as dmabuf");
    // The export holds its own reference, the GEM handle is ours to drop.
    let _ = card.close_buffer(bo);
    let (width, height) = info.size();
    Ok(Some(Exported {
        fd: fd?,
        width,
        height,
        fourcc: info.pixel_format() as u32,
        modifier: info.modifier().map(u64::from).unwrap_or(0),
        pitch: info.pitches()[0],
        offset: info.offsets()[0],
    }))
}

struct Card(std::fs::File);

impl AsFd for Card {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for Card {}
impl ControlDevice for Card {}

/// The helper's side: says its version, then answers framebuffer ids on the
/// socket it was started with (its stdin) until the recorder hangs up.
pub fn serve(card_path: &Path) -> Result<()> {
    // Only ever the display, nothing else gets opened with the permission.
    use std::os::unix::fs::FileTypeExt;
    let real = std::fs::canonicalize(card_path).with_context(|| format!("finding {}", card_path.display()))?;
    let is_device = std::fs::metadata(&real).is_ok_and(|m| m.file_type().is_char_device());
    if !real.to_string_lossy().starts_with("/dev/dri/card") || !is_device {
        bail!("{} isn't a display", card_path.display());
    }
    let file = std::fs::OpenOptions::new().read(true).write(true).open(&real);
    let card = Card(file.with_context(|| format!("opening {}", real.display()))?);
    let mut sock = unsafe { UnixStream::from_raw_fd(0) };
    sock.write_all(&VERSION.to_le_bytes())?;
    let mut id = [0u8; 4];
    loop {
        match sock.read_exact(&mut id) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        }
        let fb_id = u32::from_le_bytes(id);
        match export(&card, fb_id) {
            Ok(Some(buf)) => send(&sock, &reply(0, &buf), Some(buf.fd.as_raw_fd()))?,
            Ok(None) => {
                eprintln!("framely-panel-grab: the kernel hid framebuffer {fb_id}, is the permission gone?");
                send(&sock, &[1; REPLY_LEN], None)?
            }
            Err(e) => {
                eprintln!("framely-panel-grab: {e:#}");
                send(&sock, &[1; REPLY_LEN], None)?
            }
        }
    }
}

fn reply(status: u32, b: &Exported) -> [u8; REPLY_LEN] {
    let mut out = [0u8; REPLY_LEN];
    for (i, v) in [status, b.width, b.height, b.fourcc, b.pitch, b.offset].into_iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out[24..32].copy_from_slice(&b.modifier.to_le_bytes());
    out
}

/// The recorder's side: the helper, started and asked per frame.
pub struct Grabber {
    sock: UnixStream,
}

impl Grabber {
    /// A dedicated socket passed by Framely's root supervisor. It grants no
    /// general capability to the Steam user process and is never public.
    pub fn inherited() -> Result<Option<Self>> {
        let Some(raw) = std::env::var_os("FRAMELY_GRAB_FD") else { return Ok(None) };
        std::env::remove_var("FRAMELY_GRAB_FD");
        let fd: RawFd = raw.to_string_lossy().parse().context("invalid helper fd")?;
        if fd < 3 { bail!("invalid helper fd"); }
        let mut sock = unsafe { UnixStream::from_raw_fd(fd) };
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC); }
        sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        sock.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;
        let mut version = [0; 4];
        sock.read_exact(&mut version).context("Framely panel helper did not answer")?;
        if u32::from_le_bytes(version) != VERSION { bail!("panel helper version mismatch"); }
        Ok(Some(Self { sock }))
    }

    pub fn export(&mut self, fb_id: u32) -> Result<Exported> {
        self.sock.write_all(&fb_id.to_le_bytes()).context("asking the panel helper")?;
        let (bytes, fd) = recv(&self.sock)?;
        let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        let (Some(fd), 0) = (fd, word(0)) else { bail!("the panel helper couldn't read framebuffer {fb_id}") };
        Ok(Exported {
            fd,
            width: word(1),
            height: word(2),
            fourcc: word(3),
            pitch: word(4),
            offset: word(5),
            modifier: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        })
    }
}


fn send(sock: &UnixStream, bytes: &[u8; REPLY_LEN], fd: Option<RawFd>) -> Result<()> {
    let mut iov = libc::iovec { iov_base: bytes.as_ptr() as *mut _, iov_len: bytes.len() };
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
    let mut control = vec![0u8; space];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    if let Some(fd) = fd {
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen = space as _;
        unsafe {
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
            std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<RawFd>(), fd);
        }
    }
    if unsafe { libc::sendmsg(sock.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) } != REPLY_LEN as isize {
        return Err(std::io::Error::last_os_error()).context("answering the recorder");
    }
    Ok(())
}

fn recv(sock: &UnixStream) -> Result<([u8; REPLY_LEN], Option<OwnedFd>)> {
    let mut bytes = [0u8; REPLY_LEN];
    let mut iov = libc::iovec { iov_base: bytes.as_mut_ptr().cast(), iov_len: bytes.len() };
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
    let mut control = vec![0u8; space];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = space as _;
    let got = unsafe { libc::recvmsg(sock.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC | libc::MSG_WAITALL) };
    if got != REPLY_LEN as isize {
        bail!("the panel helper hung up");
    }
    let mut fd = None;
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if !cmsg.is_null() && (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
            fd = Some(OwnedFd::from_raw_fd(std::ptr::read_unaligned(libc::CMSG_DATA(cmsg).cast::<RawFd>())));
        }
    }
    Ok((bytes, fd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervisor_socket_uses_existing_helper_protocol() {
        use std::os::fd::IntoRawFd;
        let (ours, mut helper) = UnixStream::pair().unwrap();
        let server = std::thread::spawn(move || {
            helper.write_all(&VERSION.to_le_bytes()).unwrap();
            let mut id = [0; 4]; helper.read_exact(&mut id).unwrap();
            assert_eq!(u32::from_le_bytes(id), 42);
            let file = std::fs::File::open("/dev/null").unwrap();
            let buf = Exported {fd: OwnedFd::from(file),width:1920,height:1080,fourcc:0x34325258,modifier:0,pitch:7680,offset:0};
            send(&helper, &reply(0, &buf), Some(buf.fd.as_raw_fd())).unwrap();
        });
        std::env::set_var("FRAMELY_GRAB_FD", ours.into_raw_fd().to_string());
        let mut grabber = Grabber::inherited().unwrap().unwrap();
        assert!(std::env::var_os("FRAMELY_GRAB_FD").is_none());
        let buf = grabber.export(42).unwrap();
        assert_eq!((buf.width, buf.height), (1920, 1080));
        server.join().unwrap();
    }

    #[test]
    fn replies_round_trip_with_a_file() {
        let (a, b) = UnixStream::pair().unwrap();
        let file = std::fs::File::open("/dev/null").unwrap();
        let buf = Exported {
            fd: OwnedFd::from(file),
            width: 1920,
            height: 1080,
            fourcc: 0x34325258,
            modifier: 0x0500000000000001,
            pitch: 7680,
            offset: 64,
        };
        send(&a, &reply(0, &buf), Some(buf.fd.as_raw_fd())).unwrap();
        let (bytes, fd) = recv(&b).unwrap();
        assert!(fd.is_some());
        assert_eq!(bytes, reply(0, &buf));
        send(&a, &[1; REPLY_LEN], None).unwrap();
        let (_, fd) = recv(&b).unwrap();
        assert!(fd.is_none());
    }
}
