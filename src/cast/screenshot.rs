//! One-frame screenshots. The capture path is independent of the cast encoder.
use super::settings::{Eye, Output, Settings, Source};
use anyhow::{ensure, Context, Result};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use std::{fs, io::{BufWriter, Write}, os::unix::process::CommandExt, path::{Path, PathBuf}, process::{Child, Command, Output as ProcessOutput, Stdio}, time::{SystemTime, UNIX_EPOCH}};

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Crop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Crop {
    pub fn validate(self) -> Result<Self> {
        ensure!([self.x,self.y,self.width,self.height].into_iter().all(f64::is_finite)
            && self.x >= 0. && self.y >= 0. && self.width > 0. && self.height > 0.
            && self.x + self.width <= 1.000001 && self.y + self.height <= 1.000001,
            "截图取景范围无效");
        Ok(self)
    }
    pub fn apply(self, image: DynamicImage) -> Result<DynamicImage> {
        let self_ = self.validate()?;
        let width = image.width(); let height = image.height();
        let x = (self_.x * width as f64).round() as u32;
        let y = (self_.y * height as f64).round() as u32;
        let w = (self_.width * width as f64).round() as u32;
        let h = (self_.height * height as f64).round() as u32;
        ensure!(w > 0 && h > 0 && x + w <= width && y + h <= height,"截图取景范围无效");
        Ok(image.crop_imm(x,y,w,h))
    }
}

fn screen_process(settings: &Settings, binaries: &Path, panel_socket: &Path, wait: bool) -> Result<Child> {
    let eye = if settings.output==Output::Raw {0} else {match settings.eye {Eye::Left=>0,Eye::Right=>1,Eye::SteamVR=>super::runtime::dominant_eye()?}};
    let stream=std::os::unix::net::UnixStream::connect(panel_socket)?;
    use std::os::fd::AsRawFd;
    let fd=stream.as_raw_fd();
    let mut command=Command::new(binaries.join("framely-capture"));
    command.args(["--snapshot","--width",&settings.width.to_string(),"--height",&settings.height.to_string(),"--eye",&eye.to_string()]);
    if wait {command.arg("--snapshot-wait");}
    if settings.output==Output::Raw {command.arg("--raw");}
    else {
        if let Some(fov)=settings.horizontal_fov {command.arg(format!("--fov={fov}"));}
        command.arg(format!("--center-x={}",settings.center_x)).arg(format!("--center-y={}",settings.center_y));
    }
    command.env("FRAMELY_GRAB_FD","3").stdin(if wait {Stdio::piped()} else {Stdio::null()}).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    unsafe {command.pre_exec(move || {
        if fd!=3 && libc::dup2(fd,3)<0 {return Err(std::io::Error::last_os_error());}
        if libc::fcntl(3,libc::F_SETFD,0)<0 {return Err(std::io::Error::last_os_error());}
        Ok(())
    });}
    let child=command.spawn().context("无法启动屏幕截图采集")?;
    drop(stream);
    Ok(child)
}

fn screen_image(output: ProcessOutput) -> Result<DynamicImage> {
    ensure!(output.status.success(),"屏幕截图失败：{}",String::from_utf8_lossy(&output.stderr).trim());
    ensure!(output.stdout.len()>=8,"截图采集没有返回画面");
    let width=u32::from_le_bytes(output.stdout[0..4].try_into()?);
    let height=u32::from_le_bytes(output.stdout[4..8].try_into()?);
    ensure!(width>0&&height>0&&u64::from(width)*u64::from(height)<=9_331_200
        && output.stdout.len()==8+width as usize*height as usize*4,"截图采集数据长度无效");
    let pixels=ImageBuffer::<Rgba<u8>,_>::from_raw(width,height,output.stdout[8..].to_vec()).context("截图像素无效")?;
    Ok(DynamicImage::ImageRgba8(pixels))
}

fn screen(settings: &Settings, binaries: &Path, panel_socket: &Path) -> Result<DynamicImage> {
    screen_image(screen_process(settings,binaries,panel_socket,false)?.wait_with_output()?)
}

pub struct PreparedScreen { child: Option<Child> }
impl PreparedScreen {
    pub fn start(settings: &Settings, binaries: &Path, panel_socket: &Path) -> Result<Self> {
        ensure!(settings.source==Source::Screen,"仅屏幕采集支持预热截图");
        Ok(Self{child:Some(screen_process(settings,binaries,panel_socket,true)?)})
    }
    pub fn capture(mut self) -> Result<DynamicImage> {
        self.child.as_mut().context("截图采集程序已退出")?.stdin.take().context("无法触发截图")?.write_all(&[1])?;
        let child=self.child.take().unwrap();
        screen_image(child.wait_with_output()?)
    }
}
impl Drop for PreparedScreen {
    fn drop(&mut self) {
        if let Some(mut child)=self.child.take() {let _=child.kill();let _=child.wait();}
    }
}

fn steamvr() -> Result<DynamicImage> {
    let output=Command::new("ffmpeg").args(["-hide_banner","-loglevel","error","-nostdin","-f","v4l2","-i","/dev/video99","-frames:v","1","-f","image2pipe","-c:v","png","pipe:1"])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).output().context("无法启动 SteamVR 截图采集")?;
    ensure!(output.status.success(),"SteamVR 截图失败：{}",String::from_utf8_lossy(&output.stderr).trim());
    let image=image::load_from_memory_with_format(&output.stdout,ImageFormat::Png)?;
    ensure!(image.width()>0&&image.height()>0&&u64::from(image.width())*u64::from(image.height())<=9_331_200,"SteamVR 截图尺寸无效");
    Ok(image)
}

pub fn capture(settings: &Settings, binaries: &Path, panel_socket: Option<&Path>, crop: Option<Crop>) -> Result<DynamicImage> {
    let image=match settings.source {
        Source::Screen=>screen(settings,binaries,panel_socket.context("缺少屏幕采集助手连接")?)?,
        Source::SteamVR=>steamvr()?,
    };
    match crop {Some(crop)=>crop.apply(image),None=>Ok(image)}
}

pub fn save(image: &DynamicImage, directory: &Path) -> Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let timestamp=SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let suffix=hex::encode(rand::random::<[u8;4]>());
    let name=format!("framely-{timestamp}-{suffix}.png");
    let path=directory.join(name);
    let temporary=directory.join(format!(".framely-{timestamp}-{suffix}.part"));
    let result=(|| -> Result<()> {
        let file=fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        let mut writer=BufWriter::new(file);
        image.write_to(&mut writer,ImageFormat::Png)?;
        use std::io::Write;
        writer.flush()?;
        writer.into_inner()?.sync_all()?;
        fs::rename(&temporary,&path)?;
        Ok(())
    })();
    if result.is_err(){let _=fs::remove_file(&temporary);}
    result?;
    Ok(path)
}
