use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU32, Ordering};

use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags};

use crate::auth;
use crate::error::LinbusError;
use crate::fd_pass;
use crate::message::{self, Message, MessageType};

pub struct Connection {
    stream: UnixStream,
    serial: AtomicU32,
    unique_name: String,
    recv_buf: Vec<u8>,
    epoll: Epoll,
}

impl Connection {
    /// Connect to the session bus, authenticate, and call Hello().
    pub fn session() -> Result<Self, LinbusError> {
        let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .map_err(|_| LinbusError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "DBUS_SESSION_BUS_ADDRESS not set",
            )))?;
        let path = parse_bus_address(&addr)?;
        Self::connect(&path)
    }

    /// Connect to the system bus, authenticate, and call Hello().
    pub fn system() -> Result<Self, LinbusError> {
        Self::connect("/run/dbus/system_bus_socket")
    }

    fn connect(path: &str) -> Result<Self, LinbusError> {
        let mut stream = if path.starts_with('\0') {
            // Abstract socket — use nix for abstract namespace
            let raw_fd = nix::sys::socket::socket(
                nix::sys::socket::AddressFamily::Unix,
                nix::sys::socket::SockType::Stream,
                nix::sys::socket::SockFlag::SOCK_CLOEXEC,
                None,
            )?;
            let unix_addr = nix::sys::socket::UnixAddr::new_abstract(path[1..].as_bytes())?;
            nix::sys::socket::connect(raw_fd.as_raw_fd(), &unix_addr)?;
            let owned: OwnedFd = raw_fd;
            UnixStream::from(owned)
        } else {
            UnixStream::connect(path)?
        };

        stream.set_nonblocking(false)?;
        auth::authenticate(&mut stream)?;
        stream.set_nonblocking(true)?;

        let epoll = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;
        let event = EpollEvent::new(EpollFlags::EPOLLIN, 0);
        epoll.add(&stream, event)?;

        let mut conn = Self {
            stream,
            serial: AtomicU32::new(1),
            unique_name: String::new(),
            recv_buf: Vec::with_capacity(65536),
            epoll,
        };

        // Call Hello() to get unique name
        let hello = Message::method_call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "Hello",
        );
        conn.send_msg(&hello)?;
        let reply = conn.recv_blocking(5000)?
            .ok_or_else(|| LinbusError::Timeout)?;

        if reply.msg_type == MessageType::Error {
            return Err(LinbusError::MethodError {
                name: reply.error_name.unwrap_or_default(),
                message: reply.body.first().and_then(|v| v.as_str()).unwrap_or("").to_string(),
            });
        }

        conn.unique_name = reply.body.first()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(conn)
    }

    pub fn unique_name(&self) -> &str {
        &self.unique_name
    }

    pub fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }

    fn next_serial(&self) -> u32 {
        self.serial.fetch_add(1, Ordering::Relaxed)
    }

    /// Send a message, assigning a serial. Returns the serial used.
    pub fn send_msg(&self, msg: &Message) -> Result<u32, LinbusError> {
        let serial = self.next_serial();
        let data = message::marshal_message(msg, serial);
        fd_pass::send_with_fds(self.stream.as_raw_fd(), &data, &[])?;
        Ok(serial)
    }

    /// Receive one complete message (blocking with timeout in ms).
    /// Returns None on timeout.
    pub fn recv_blocking(&mut self, timeout_ms: u16) -> Result<Option<Message>, LinbusError> {
        let (msg, _fds) = self.recv_with_fds(timeout_ms)?;
        Ok(msg)
    }

    /// Receive one complete message plus any file descriptors.
    pub fn recv_with_fds(&mut self, timeout_ms: u16) -> Result<(Option<Message>, Vec<OwnedFd>), LinbusError> {
        let mut all_fds = Vec::new();

        loop {
            // Check if we already have a complete message in the buffer
            if let Some(msg_len) = message::message_length(&self.recv_buf) {
                if self.recv_buf.len() >= msg_len {
                    let msg_data: Vec<u8> = self.recv_buf.drain(..msg_len).collect();
                    let (msg, _) = message::read_message(&msg_data)?;
                    return Ok((Some(msg), all_fds));
                }
            }

            // Wait for data
            let mut events = [EpollEvent::empty(); 1];
            let n = self.epoll.wait(&mut events, timeout_ms)?;
            if n == 0 {
                return Ok((None, all_fds));
            }

            // Read available data
            let mut tmp = [0u8; 65536];
            let (nbytes, fds) = fd_pass::recv_with_fds(self.stream.as_raw_fd(), &mut tmp)?;
            if nbytes == 0 {
                return Err(LinbusError::Io(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "connection closed",
                )));
            }
            self.recv_buf.extend_from_slice(&tmp[..nbytes]);
            all_fds.extend(fds);
        }
    }

    /// Send a method call and wait for the reply (matching reply_serial).
    pub fn call(&mut self, msg: &Message, timeout_ms: u16) -> Result<Message, LinbusError> {
        let serial = self.send_msg(msg)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
        let mut skipped = 0u32;

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let ms = remaining.as_millis().min(u16::MAX as u128) as u16;
            if ms == 0 {
                return Err(LinbusError::Timeout);
            }

            let reply = self.recv_blocking(ms)?;
            match reply {
                Some(msg) if msg.reply_serial == Some(serial) => {
                    if msg.msg_type == MessageType::Error {
                        return Err(LinbusError::MethodError {
                            name: msg.error_name.unwrap_or_default(),
                            message: msg.body.first().and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        });
                    }
                    return Ok(msg);
                }
                Some(_) => continue, // not our reply, keep reading
                None => return Err(LinbusError::Timeout),
            }
        }
    }

    /// Send a method call and get reply including fds.
    pub fn call_with_fds(&mut self, msg: &Message, timeout_ms: u16) -> Result<(Message, Vec<OwnedFd>), LinbusError> {
        let serial = self.send_msg(msg)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let ms = remaining.as_millis().min(u16::MAX as u128) as u16;
            if ms == 0 {
                return Err(LinbusError::Timeout);
            }

            let (reply, fds) = self.recv_with_fds(ms)?;
            match reply {
                Some(msg) if msg.reply_serial == Some(serial) => {
                    if msg.msg_type == MessageType::Error {
                        return Err(LinbusError::MethodError {
                            name: msg.error_name.unwrap_or_default(),
                            message: msg.body.first().and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        });
                    }
                    return Ok((msg, fds));
                }
                Some(_) => continue,
                None => return Err(LinbusError::Timeout),
            }
        }
    }
}

fn parse_bus_address(addr: &str) -> Result<String, LinbusError> {
    // Format: "unix:path=/run/user/1000/bus" or "unix:path=/tmp/dbus-xxx,guid=..."
    // or "unix:abstract=/tmp/dbus-xxx,guid=..."
    // Multiple addresses separated by ';'
    for part in addr.split(';') {
        if let Some(rest) = part.strip_prefix("unix:") {
            for kv in rest.split(',') {
                if let Some(path) = kv.strip_prefix("path=") {
                    return Ok(path.to_string());
                }
                if let Some(abs) = kv.strip_prefix("abstract=") {
                    return Ok(format!("\0{}", abs));
                }
            }
        }
    }
    Err(LinbusError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("cannot parse bus address: {}", addr),
    )))
}
