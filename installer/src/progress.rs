use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Prepare,
    Download,
    Verify,
    Transfer,
    Install,
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub stage: Stage,
    pub detail: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub step: Option<(u8, u8)>,
}
impl Progress {
    pub fn new(stage: Stage, detail: impl Into<String>) -> Self {
        Self {
            stage,
            detail: detail.into(),
            completed: 0,
            total: None,
            step: None,
        }
    }
    pub fn fraction(&self) -> Option<f32> {
        self.total
            .filter(|total| *total > 0)
            .map(|total| (self.completed.min(total) as f64 / total as f64) as f32)
    }
}

// Report small files too, throttle large streams, and only report completion
// after checking the exact byte count. The same path handles HTTPS and SFTP.
pub fn copy(
    reader: &mut impl Read,
    writer: &mut impl Write,
    total: u64,
    stage: Stage,
    detail: &str,
    report: &mut dyn FnMut(Progress),
) -> Result<()> {
    let mut value = Progress::new(stage, detail);
    value.total = Some(total);
    report(value.clone());
    let mut block = [0; 256 * 1024];
    let mut last = Instant::now();
    loop {
        let n = reader.read(&mut block)?;
        if n == 0 {
            break;
        }
        ensure!(value.completed + n as u64 <= total, "传输超过声明长度");
        writer.write_all(&block[..n])?;
        value.completed += n as u64;
        if last.elapsed() >= Duration::from_millis(200) {
            report(value.clone());
            last = Instant::now();
        }
    }
    ensure!(value.completed == total, "传输未完成");
    writer.flush()?;
    report(value);
    Ok(())
}

#[derive(Deserialize)]
struct DeviceStep {
    step: String,
    completed: Option<u64>,
    total: Option<u64>,
}

#[derive(Default)]
pub struct DeviceProgress {
    pending: String,
}
impl DeviceProgress {
    pub fn push(&mut self, chunk: &str, report: &mut dyn FnMut(Progress)) -> Vec<String> {
        let mut logs = Vec::new();
        self.pending.push_str(chunk);
        while let Some(end) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=end).collect();
            let Some(json) = line.trim().strip_prefix("FRAMELY_PROGRESS ") else {
                logs.push(line);
                continue;
            };
            let Ok(step) = serde_json::from_str::<DeviceStep>(json) else {
                continue;
            };
            let (index, detail) = match step.step.as_str() {
                "verify" => (1, "设备端校验安装包"),
                "stage" => (2, "准备设备安装目录"),
                "extract" => (3, "解压并校验发行文件"),
                "configure" => (4, "配置账号、安装文件与系统服务"),
                "activate" => (5, "启动服务并检查安装状态"),
                _ => continue,
            };
            if step
                .total
                .is_some_and(|total| total == 0 || step.completed.unwrap_or(0) > total)
            {
                continue;
            }
            let mut value = Progress::new(Stage::Install, detail);
            value.step = Some((index, 5));
            value.completed = step.completed.unwrap_or(0);
            value.total = step.total;
            report(value);
        }
        if self.pending.len() > 8192 {
            logs.push(std::mem::take(&mut self.pending));
        }
        logs
    }
    pub fn finish(&mut self) -> String {
        let pending = std::mem::take(&mut self.pending);
        if pending.trim().starts_with("FRAMELY_PROGRESS ") {
            String::new()
        } else {
            pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_progress_reports_small_files_and_rejects_incomplete_or_oversized_streams() {
        let mut reports = Vec::new();
        let mut output = Vec::new();
        copy(
            &mut &b"payload"[..],
            &mut output,
            7,
            Stage::Transfer,
            "package",
            &mut |p| reports.push(p),
        )
        .unwrap();
        assert_eq!(output, b"payload");
        assert_eq!(reports.first().unwrap().completed, 0);
        assert_eq!(reports.last().unwrap().fraction(), Some(1.0));
        for declared in [6, 8] {
            let mut reports = Vec::new();
            assert!(
                copy(
                    &mut &b"payload"[..],
                    &mut Vec::new(),
                    declared,
                    Stage::Download,
                    "package",
                    &mut |p| reports.push(p)
                )
                .is_err()
            );
            assert!(reports.iter().all(|p| p.fraction() != Some(1.0)));
        }
        assert_eq!(Progress::new(Stage::Install, "working").fraction(), None);
    }
    #[test]
    fn device_progress_handles_split_lines_and_rejects_invalid_events() {
        let mut decoder = DeviceProgress::default();
        let mut reports = Vec::new();
        let mut report = |p| reports.push(p);
        assert_eq!(
            decoder.push("service output\nFRAMELY_PRO", &mut report),
            ["service output\n"]
        );
        decoder.push("GRESS {\"step\":\"extract\",\"completed\":2,", &mut report);
        decoder.push(
            "\"total\":8}\nFRAMELY_PROGRESS {\"step\":\"configure\"}\n",
            &mut report,
        );
        decoder.push("FRAMELY_PROGRESS {\"step\":\"unknown\"}\nFRAMELY_PROGRESS {\"step\":\"extract\",\"completed\":9,\"total\":8}\n", &mut report);
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].fraction(), Some(0.25));
        assert_eq!(reports[0].step, Some((3, 5)));
        assert_eq!(reports[1].step, Some((4, 5)));
        assert_eq!(reports[1].fraction(), None);
    }
}
