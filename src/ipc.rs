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
