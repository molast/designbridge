use serde_json::json;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::UnixStream;

const MAX_MESSAGE_BYTES: usize = 256 * 1024;

fn read_frame(reader: &mut impl Read) -> Result<Option<Vec<u8>>, String> {
    let mut length = [0u8; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("无法读取消息长度：{error}")),
    }
    let length = u32::from_ne_bytes(length) as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err("浏览器扩展消息大小无效".to_string());
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| format!("无法读取浏览器扩展消息：{error}"))?;
    Ok(Some(payload))
}

fn write_frame(writer: &mut impl Write, payload: &[u8]) -> Result<(), String> {
    if payload.is_empty() || payload.len() > MAX_MESSAGE_BYTES {
        return Err("DesignBridge 响应大小无效".to_string());
    }
    writer
        .write_all(&(payload.len() as u32).to_ne_bytes())
        .and_then(|_| writer.write_all(payload))
        .and_then(|_| writer.flush())
        .map_err(|error| format!("无法写入浏览器扩展响应：{error}"))
}

fn socket_path() -> Result<PathBuf, String> {
    dirs::data_dir()
        .map(|path| {
            path.join("com.designbridge.app")
                .join("browser-extension.sock")
        })
        .ok_or_else(|| "无法确定 DesignBridge 数据目录".to_string())
}

#[cfg(unix)]
fn forward_to_designbridge(payload: &[u8]) -> Result<Vec<u8>, String> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path).map_err(|error| {
        format!(
            "无法连接 DesignBridge 客户端（{}）：{error}",
            path.display()
        )
    })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| format!("无法设置读取超时：{error}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| format!("无法设置写入超时：{error}"))?;
    write_frame(&mut stream, payload)?;
    read_frame(&mut stream)?.ok_or_else(|| "DesignBridge 客户端未返回响应".to_string())
}

#[cfg(not(unix))]
fn forward_to_designbridge(_payload: &[u8]) -> Result<Vec<u8>, String> {
    Err("当前版本的浏览器扩展抓取仅支持 macOS 和 Linux".to_string())
}

fn error_response(message: impl Into<String>) -> Vec<u8> {
    serde_json::to_vec(&json!({ "ok": false, "message": message.into() }))
        .unwrap_or_else(|_| br#"{"ok":false,"message":"DesignBridge host error"}"#.to_vec())
}

fn run() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    while let Some(request) = read_frame(&mut reader)? {
        let response = forward_to_designbridge(&request).unwrap_or_else(error_response);
        write_frame(&mut writer, &response)?;
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        let _ = write_frame(&mut io::stdout(), &error_response(error));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_preserves_json() {
        let payload = br#"{"version":1,"type":"capture"}"#;
        let mut framed = Vec::new();
        write_frame(&mut framed, payload).unwrap();
        let decoded = read_frame(&mut framed.as_slice()).unwrap().unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn rejects_oversized_frames() {
        let mut framed = ((MAX_MESSAGE_BYTES + 1) as u32).to_ne_bytes().to_vec();
        framed.extend_from_slice(b"ignored");
        assert!(read_frame(&mut framed.as_slice()).is_err());
    }

    #[test]
    fn reads_multiple_frames_from_one_native_port() {
        let mut framed = Vec::new();
        write_frame(&mut framed, br#"{"type":"heartbeat"}"#).unwrap();
        write_frame(&mut framed, br#"{"type":"heartbeat"}"#).unwrap();
        let mut reader = framed.as_slice();

        assert!(read_frame(&mut reader).unwrap().is_some());
        assert!(read_frame(&mut reader).unwrap().is_some());
        assert!(read_frame(&mut reader).unwrap().is_none());
    }
}
