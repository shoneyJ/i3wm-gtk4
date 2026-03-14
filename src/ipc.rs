//! Raw i3 IPC protocol implementation over Unix socket.
//!
//! Protocol: "i3-ipc" magic (6 bytes) + payload_len (u32 LE) + msg_type (u32 LE) + payload

use serde_json::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;

const I3_MAGIC: &[u8; 6] = b"i3-ipc";
const HEADER_LEN: usize = 6 + 4 + 4; // magic + length + type

// Request message types
pub const MSG_RUN_COMMAND: u32 = 0;
pub const MSG_GET_WORKSPACES: u32 = 1;
pub const MSG_SUBSCRIBE: u32 = 2;
pub const MSG_GET_TREE: u32 = 4;

// Event types (response type has bit 31 set)
pub const EVENT_WORKSPACE: u32 = 0x80000000;
pub const EVENT_WINDOW: u32 = 0x80000003;

pub struct I3Connection {
    stream: UnixStream,
}

impl I3Connection {
    /// Connect to the i3 IPC socket.
    /// Tries I3SOCK env var first, then `i3 --get-socketpath`.
    pub fn connect() -> Result<Self, Box<dyn std::error::Error>> {
        let socket_path = get_socket_path()?;
        let stream = UnixStream::connect(&socket_path)?;
        Ok(Self { stream })
    }

    fn send_raw(&mut self, msg_type: u32, payload: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let len = payload.len() as u32;
        self.stream.write_all(I3_MAGIC)?;
        self.stream.write_all(&len.to_le_bytes())?;
        self.stream.write_all(&msg_type.to_le_bytes())?;
        if !payload.is_empty() {
            self.stream.write_all(payload)?;
        }
        self.stream.flush()?;
        Ok(())
    }

    fn recv_raw(&mut self) -> Result<(u32, Vec<u8>), Box<dyn std::error::Error>> {
        let mut header = [0u8; HEADER_LEN];
        self.stream.read_exact(&mut header)?;

        // Verify magic
        if &header[..6] != I3_MAGIC {
            return Err("Invalid i3 IPC magic bytes".into());
        }

        let len = u32::from_le_bytes(header[6..10].try_into()?) as usize;
        let msg_type = u32::from_le_bytes(header[10..14].try_into()?);

        let mut payload = vec![0u8; len];
        if len > 0 {
            self.stream.read_exact(&mut payload)?;
        }

        Ok((msg_type, payload))
    }

    fn send_and_recv(&mut self, msg_type: u32, payload: &[u8]) -> Result<Value, Box<dyn std::error::Error>> {
        self.send_raw(msg_type, payload)?;
        let (_, resp) = self.recv_raw()?;
        Ok(serde_json::from_slice(&resp)?)
    }

    /// Get the list of workspaces with metadata.
    pub fn get_workspaces(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        self.send_and_recv(MSG_GET_WORKSPACES, b"")
    }

    /// Get the full window tree.
    pub fn get_tree(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        self.send_and_recv(MSG_GET_TREE, b"")
    }

    /// Run an i3 command (e.g., "workspace number 3").
    pub fn run_command(&mut self, cmd: &str) -> Result<Value, Box<dyn std::error::Error>> {
        self.send_and_recv(MSG_RUN_COMMAND, cmd.as_bytes())
    }

    /// Subscribe to i3 events. This connection becomes an event stream after subscribing.
    pub fn subscribe(&mut self, events: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        let payload = serde_json::to_string(events)?;
        self.send_raw(MSG_SUBSCRIBE, payload.as_bytes())?;
        let (_, resp) = self.recv_raw()?;
        let result: Value = serde_json::from_slice(&resp)?;
        if result["success"].as_bool() != Some(true) {
            return Err(format!("i3 subscribe failed: {}", result).into());
        }
        Ok(())
    }

    /// Read the next event from the subscription stream. Blocks until an event arrives.
    pub fn read_event(&mut self) -> Result<(u32, Value), Box<dyn std::error::Error>> {
        let (msg_type, payload) = self.recv_raw()?;
        Ok((msg_type, serde_json::from_slice(&payload)?))
    }
}

fn get_socket_path() -> Result<String, Box<dyn std::error::Error>> {
    // Try I3SOCK environment variable first
    if let Ok(path) = std::env::var("I3SOCK") {
        if std::path::Path::new(&path).exists() {
            return Ok(path);
        }
    }

    // Try SWAYSOCK for sway compatibility
    if let Ok(path) = std::env::var("SWAYSOCK") {
        if std::path::Path::new(&path).exists() {
            return Ok(path);
        }
    }

    // Fall back to asking i3 directly
    let output = Command::new("i3")
        .arg("--get-socketpath")
        .output()
        .map_err(|e| format!("Failed to run `i3 --get-socketpath`: {}", e))?;

    if !output.status.success() {
        return Err("i3 --get-socketpath returned non-zero exit code".into());
    }

    let path = String::from_utf8(output.stdout)?.trim().to_string();
    if path.is_empty() {
        return Err("i3 socket path is empty".into());
    }

    Ok(path)
}
