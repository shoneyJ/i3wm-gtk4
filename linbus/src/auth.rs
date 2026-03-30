use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use crate::error::LinbusError;

/// Perform the D-Bus SASL EXTERNAL authentication handshake.
/// Returns Ok(()) on success.
pub fn authenticate(stream: &mut UnixStream) -> Result<(), LinbusError> {
    // 1. Send null byte (credential byte)
    stream.write_all(&[0])?;

    // 2. Send AUTH EXTERNAL <hex-encoded-uid>
    let uid = nix::unistd::getuid();
    let uid_hex = hex_encode_uid(uid.as_raw());
    let auth_line = format!("AUTH EXTERNAL {}\r\n", uid_hex);
    stream.write_all(auth_line.as_bytes())?;

    // 3. Read response — expect "OK <guid>"
    let response = read_line(stream)?;
    if !response.starts_with("OK ") {
        return Err(LinbusError::AuthFailed(format!("expected OK, got: {}", response)));
    }

    // 4. Send BEGIN to switch to binary protocol
    stream.write_all(b"BEGIN\r\n")?;

    Ok(())
}

fn hex_encode_uid(uid: u32) -> String {
    let uid_str = uid.to_string();
    uid_str.bytes().map(|b| format!("{:02x}", b)).collect()
}

fn read_line(stream: &mut UnixStream) -> Result<String, LinbusError> {
    let mut buf = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte)?;
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n") {
            buf.truncate(buf.len() - 2);
            return String::from_utf8(buf)
                .map_err(|e| LinbusError::AuthFailed(format!("invalid utf8: {}", e)));
        }
        if buf.len() > 4096 {
            return Err(LinbusError::AuthFailed("response too long".into()));
        }
    }
}
