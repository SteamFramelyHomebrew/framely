use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};
pub fn read(stream: &mut impl Read) -> Result<Value> {
    let mut data = Vec::new();
    BufReader::new(stream).read_until(b'\n', &mut data)?;
    ensure!(data.last() == Some(&b'\n'), "Truncated IPC message");
    Ok(serde_json::from_slice(&data)?)
}
pub fn write(stream: &mut impl Write, v: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(v)?;
    stream.write_all(&bytes)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}
pub(crate) fn response(result: Result<Value>) -> Value {
    match result {
        Ok(v) => json!({"result":v}),
        Err(e) => json!({"error":format!("{e:#}")}),
    }
}
pub fn call(path: &Path, method: &str, params: Value) -> Result<Value> {
    call_timeout(path, method, params, Duration::from_secs(90))
}
pub fn call_timeout(path: &Path, method: &str, params: Value, timeout: Duration) -> Result<Value> {
    let mut s = UnixStream::connect(path).context("Framely service unavailable")?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(Duration::from_secs(90)))?;
    write(&mut s, &json!({"method":method,"params":params}))?;
    let v = read(&mut s)?;
    if let Some(e) = v.get("error") {
        anyhow::bail!("{}", e.as_str().unwrap_or("Service error"));
    }
    Ok(v["result"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_errors_keep_the_cause_across_ipc() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("service.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read(&mut stream).unwrap();
            assert_eq!(request["method"], "install.batch");
            let result = Err(anyhow::anyhow!("Permission denied (os error 13)"))
                .context("安装插件 test.plugin 失败")
                .context("批量安装失败，已恢复原版本和启用状态");
            write(&mut stream, &response(result)).unwrap();
        });
        let error = call(&socket, "install.batch", json!({})).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("批量安装失败，已恢复原版本和启用状态"));
        assert!(message.contains("test.plugin"));
        assert!(message.contains("Permission denied (os error 13)"));
        worker.join().unwrap();
    }

    #[test]
    fn package_messages_can_exceed_previous_transport_limit() {
        let request = json!({"method":"install","params":{"package":"A".repeat(97 * 1024 * 1024)}});
        let mut encoded = Vec::new();
        write(&mut encoded, &request).unwrap();
        drop(request);
        let decoded = read(&mut encoded.as_slice()).unwrap();
        assert_eq!(
            decoded["params"]["package"].as_str().unwrap().len(),
            97 * 1024 * 1024
        );
    }
}
