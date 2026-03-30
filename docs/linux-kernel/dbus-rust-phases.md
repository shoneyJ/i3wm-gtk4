# linbus — Direct D-Bus Library for i3More

**linbus** (Linux Bus) — a minimal D-Bus implementation using Linux kernel primitives directly.
Replaces [`zbus`](https://github.com/z-galaxy/zbus) crate. No async runtime, no epoll abstraction crates. Just `nix` + `std`.

## Why Replace zbus

```
zbus stack (what we remove):          linbus stack (what we build):
─────────────────────────              ──────────────────────────
Your Rust code                         Your Rust code
    ↓                                      ↓
zbus (~50 transitive deps)             linbus (0 extra deps, uses nix)
    ↓                                      ↓
async-io / polling crate               epoll_create1, epoll_ctl, epoll_wait
    ↓                                      ↓
epoll syscalls                          Unix domain socket (AF_UNIX)
    ↓                                      ↓
Unix domain socket                     /run/user/1000/bus
```

**Goal**: Learn the D-Bus wire protocol and Linux kernel IPC by implementing it from scratch.

---

## Library Structure

```
linbus/
├── Cargo.toml
├── src/
│   ├── lib.rs              — pub mod re-exports
│   ├── auth.rs             — SASL auth handshake (EXTERNAL)
│   ├── conn.rs             — Connection (session/system bus, socket + serial counter)
│   ├── marshal.rs          — Serialize Rust values → D-Bus wire format
│   ├── unmarshal.rs        — Deserialize D-Bus wire format → Rust values
│   ├── message.rs          — Message struct (header + body), MessageType enum
│   ├── value.rs            — Value enum (String, Bool, I32, U32, Array, Dict, Struct, Variant, Fd)
│   ├── signature.rs        — D-Bus type signature parser ("a{sv}", "(iiibiiay)", etc.)
│   ├── bus.rs              — Hello(), RequestName(), AddMatch() helpers
│   ├── proxy.rs            — Client proxy: call methods, read properties
│   ├── dispatch.rs         — Server: register handlers, dispatch METHOD_CALL, emit signals
│   ├── fd_pass.rs          — SCM_RIGHTS file descriptor passing via sendmsg/recvmsg
│   └── error.rs            — LinbusError enum
└── examples/
    └── hello_bus.rs        — Connect, Hello, print unique name
```

### Cargo.toml

```toml
[package]
name = "linbus"
version = "0.1.0"
edition = "2021"
description = "Minimal D-Bus library using Linux kernel primitives (epoll, Unix sockets, SCM_RIGHTS)"

[dependencies]
nix = { version = "0.29", features = ["net", "event", "socket", "uio", "process"] }
log = "0.4"
```

**Zero additional dependencies** beyond `nix` (already in the project) and `log`.

---

## Public API — What i3More Consumers Will Call

### Connection

```rust
// linbus::conn

pub struct Connection {
    socket: OwnedFd,
    serial: AtomicU32,
    unique_name: String,       // ":1.234" assigned by dbus-daemon
    recv_buf: Vec<u8>,         // partial-read buffer
    pending_fds: Vec<OwnedFd>, // fds received via SCM_RIGHTS
}

impl Connection {
    /// Connect to session bus, authenticate, call Hello().
    pub fn session() -> Result<Self, LinbusError>;

    /// Connect to system bus, authenticate, call Hello().
    pub fn system() -> Result<Self, LinbusError>;

    /// Claim a well-known bus name (e.g. "org.freedesktop.Notifications").
    pub fn request_name(&self, name: &str) -> Result<(), LinbusError>;

    /// Register a match rule for signal filtering.
    pub fn add_match(&self, rule: &str) -> Result<(), LinbusError>;

    /// Send a raw message. Returns the serial used.
    pub fn send(&self, msg: &Message) -> Result<u32, LinbusError>;

    /// Read one message from the socket (blocking, with timeout_ms).
    /// Returns None on timeout.
    pub fn recv(&self, timeout_ms: u16) -> Result<Option<Message>, LinbusError>;

    /// Read one message, also extracting any file descriptors passed via SCM_RIGHTS.
    pub fn recv_with_fds(&self, timeout_ms: u16) -> Result<Option<(Message, Vec<OwnedFd>)>, LinbusError>;

    /// Get the unique bus name assigned to this connection.
    pub fn unique_name(&self) -> &str;

    /// Get the raw fd for use with external epoll if needed.
    pub fn as_raw_fd(&self) -> RawFd;
}
```

### Proxy (client-side method calls)

```rust
// linbus::proxy

pub struct Proxy<'a> {
    conn: &'a Connection,
    destination: String,
    path: String,
    interface: String,
}

impl<'a> Proxy<'a> {
    pub fn new(conn: &'a Connection, dest: &str, path: &str, iface: &str) -> Self;

    /// Call a method and wait for reply. Deserializes response body.
    pub fn call(&self, method: &str, body: &[Value]) -> Result<Vec<Value>, LinbusError>;

    /// Call a method, also receiving file descriptors in the reply.
    pub fn call_with_fds(&self, method: &str, body: &[Value]) -> Result<(Vec<Value>, Vec<OwnedFd>), LinbusError>;

    /// Read a property via org.freedesktop.DBus.Properties.Get.
    pub fn get_property(&self, property_name: &str) -> Result<Value, LinbusError>;
}
```

### Dispatcher (server-side method handling)

```rust
// linbus::dispatch

pub type MethodHandler = Box<dyn Fn(&Message) -> Result<Vec<Value>, LinbusError> + Send>;

pub struct Dispatcher {
    conn: Connection,
    handlers: HashMap<(String, String), MethodHandler>, // (interface, member) → handler
    properties: HashMap<(String, String), Value>,        // (interface, property) → value
}

impl Dispatcher {
    pub fn new(conn: Connection) -> Self;

    /// Register a handler for incoming METHOD_CALLs.
    pub fn add_handler(&mut self, interface: &str, member: &str, handler: MethodHandler);

    /// Set a property value (served via org.freedesktop.DBus.Properties).
    pub fn set_property(&mut self, interface: &str, name: &str, value: Value);

    /// Emit a signal.
    pub fn emit_signal(&self, path: &str, interface: &str, member: &str, body: &[Value]) -> Result<(), LinbusError>;

    /// Run the event loop. Reads messages, dispatches to handlers.
    /// Calls `idle_fn` every `idle_ms` when no messages arrive.
    pub fn run(&mut self, idle_ms: u16, idle_fn: impl FnMut(&mut Self)) -> Result<(), LinbusError>;
}
```

### Value (D-Bus type system)

```rust
// linbus::value

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    ObjectPath(String),
    Signature(String),
    Bool(bool),
    Byte(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F64(f64),
    Array(Vec<Value>),
    Dict(Vec<(Value, Value)>),         // a{kv} — stored as key-value pairs
    Struct(Vec<Value>),                 // (...) — fields
    Variant(Box<Value>),                // v — wrapped with signature
}

impl Value {
    /// Attempt to extract a &str from a String value.
    pub fn as_str(&self) -> Option<&str>;
    pub fn as_i32(&self) -> Option<i32>;
    pub fn as_u32(&self) -> Option<u32>;
    pub fn as_bool(&self) -> Option<bool>;
    pub fn as_u8(&self) -> Option<u8>;
    pub fn as_array(&self) -> Option<&[Value]>;
    pub fn as_dict(&self) -> Option<&[(Value, Value)]>;
    pub fn as_struct_fields(&self) -> Option<&[Value]>;
    pub fn as_variant(&self) -> Option<&Value>;

    /// Convert Dict to a HashMap<String, Value> for convenience.
    pub fn to_string_dict(&self) -> Option<HashMap<String, Value>>;
}
```

### Message

```rust
// linbus::message

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MessageType {
    MethodCall,
    MethodReturn,
    Error,
    Signal,
}

#[derive(Debug)]
pub struct Message {
    pub msg_type: MessageType,
    pub flags: u8,
    pub serial: u32,
    pub reply_serial: Option<u32>,
    pub path: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
    pub destination: Option<String>,
    pub sender: Option<String>,
    pub signature: Option<String>,
    pub body: Vec<Value>,
}

impl Message {
    pub fn method_call(dest: &str, path: &str, iface: &str, member: &str) -> Self;
    pub fn method_return(reply_serial: u32) -> Self;
    pub fn signal(path: &str, iface: &str, member: &str) -> Self;
    pub fn error(reply_serial: u32, error_name: &str, message: &str) -> Self;
}
```

### Error

```rust
// linbus::error

#[derive(Debug)]
pub enum LinbusError {
    Io(std::io::Error),
    Nix(nix::Error),
    AuthFailed(String),
    ProtocolError(String),     // malformed message, bad alignment, etc.
    NameRequestFailed(u32),    // RequestName returned non-1
    MethodError {              // D-Bus ERROR message received
        name: String,
        message: String,
    },
    Timeout,
    InvalidSignature(String),
}
```

---

## Wire Protocol Implementation Details

### auth.rs — SASL Handshake

```
1. Send null byte: \0
2. Send: AUTH EXTERNAL <hex-encoded-uid>\r\n
3. Read: OK <server-guid>\r\n
4. Send: BEGIN\r\n
5. Switch to binary protocol
```

UID obtained via `nix::unistd::getuid()`. Hex-encode as ASCII.

### marshal.rs — Serialization Rules

Every type has an alignment requirement. The marshaller must insert padding bytes to satisfy alignment before writing each value.

| Type | Alignment | Wire Format |
|---|---|---|
| BYTE (y) | 1 | 1 byte |
| BOOLEAN (b) | 4 | u32 (0 or 1) |
| INT16 (n) | 2 | 2 bytes LE |
| UINT16 (q) | 2 | 2 bytes LE |
| INT32 (i) | 4 | 4 bytes LE |
| UINT32 (u) | 4 | 4 bytes LE |
| INT64 (x) | 8 | 8 bytes LE |
| UINT64 (t) | 8 | 8 bytes LE |
| DOUBLE (d) | 8 | 8 bytes LE (IEEE 754) |
| STRING (s) | 4 | u32 length + bytes + \0 |
| OBJECT_PATH (o) | 4 | same as STRING |
| SIGNATURE (g) | 1 | u8 length + bytes + \0 |
| ARRAY (a) | 4 | u32 byte-length + pad-to-element-alignment + elements |
| STRUCT (() | 8 | pad to 8, then fields sequentially |
| VARIANT (v) | 1 | signature(g) + value marshalled per signature |
| DICT_ENTRY ({) | 8 | pad to 8, then key + value |

**Key function signatures:**

```rust
pub fn marshal(buf: &mut Vec<u8>, value: &Value, sig_char: u8);
pub fn marshal_body(buf: &mut Vec<u8>, values: &[Value], signature: &str);
pub fn marshal_message(msg: &Message) -> Vec<u8>;
```

### unmarshal.rs — Deserialization

```rust
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn read_value(&mut self, sig: &str) -> Result<Value, LinbusError>;
    pub fn read_message(data: &[u8]) -> Result<(Message, usize), LinbusError>;
    fn align_to(&mut self, alignment: usize);
    fn read_u8(&mut self) -> Result<u8, LinbusError>;
    fn read_u32(&mut self) -> Result<u32, LinbusError>;
    fn read_i32(&mut self) -> Result<i32, LinbusError>;
    fn read_string(&mut self) -> Result<String, LinbusError>;
    fn read_signature(&mut self) -> Result<String, LinbusError>;
    fn read_array(&mut self, element_sig: &str) -> Result<Value, LinbusError>;
    fn read_struct(&mut self, field_sigs: &[String]) -> Result<Value, LinbusError>;
    fn read_variant(&mut self) -> Result<Value, LinbusError>;
    fn read_dict_entry(&mut self, key_sig: &str, val_sig: &str) -> Result<(Value, Value), LinbusError>;
}
```

### signature.rs — Type Signature Parser

Parses D-Bus signatures into individual type tokens:
- `"su"` → `["s", "u"]`
- `"a{sv}"` → `["a{sv}"]`
- `"(iiibiiay)"` → `["(iiibiiay)"]`

```rust
pub fn split_signature(sig: &str) -> Result<Vec<String>, LinbusError>;
pub fn element_alignment(sig: &str) -> usize;
```

### fd_pass.rs — SCM_RIGHTS

```rust
use nix::sys::socket::{sendmsg, recvmsg, ControlMessage, ControlMessageOwned, MsgFlags};
use nix::sys::uio::IoSlice;

pub fn send_with_fds(fd: RawFd, data: &[u8], fds: &[RawFd]) -> Result<(), LinbusError>;
pub fn recv_with_fds(fd: RawFd, buf: &mut [u8]) -> Result<(usize, Vec<OwnedFd>), LinbusError>;
```

Only used by `lock/security.rs` for the logind Inhibit() call that returns a file descriptor.

---

## How Each i3More File Migrates

### 1. lock/security.rs (simplest — Phase 0+2+5+9)

**Before (zbus):**
```rust
let conn = zbus::blocking::Connection::system()?;
let reply = conn.call_method(Some("org.freedesktop.login1"), ...)?;
let fd: zbus::zvariant::OwnedFd = reply.body().deserialize()?;
```

**After (linbus):**
```rust
let conn = linbus::Connection::system()?;
let proxy = linbus::Proxy::new(&conn, "org.freedesktop.login1",
    "/org/freedesktop/login1", "org.freedesktop.login1.Manager");
let (values, fds) = proxy.call_with_fds("Inhibit", &[
    Value::String("handle-switch".into()),
    Value::String("i3more-lock".into()),
    Value::String("Lock screen active".into()),
    Value::String("block".into()),
])?;
let raw = fds[0].as_raw_fd();
std::mem::forget(fds.into_iter().next().unwrap());
```

### 2. notify/daemon.rs (server — Phase 0+2+3+4)

**Before (zbus):** `#[interface]` macro, `connection::Builder`, `SignalEmitter`

**After (linbus):**
```rust
let conn = linbus::Connection::session()?;
conn.request_name("org.freedesktop.Notifications")?;

let mut dispatcher = linbus::Dispatcher::new(conn);

dispatcher.add_handler("org.freedesktop.Notifications", "GetCapabilities",
    Box::new(|_msg| Ok(vec![Value::Array(vec![
        Value::String("body".into()), Value::String("actions".into()), ...
    ])])));

dispatcher.add_handler("org.freedesktop.Notifications", "Notify",
    Box::new(move |msg| {
        let body = &msg.body;
        let app_name = body[0].as_str().unwrap_or("");
        let replaces_id = body[1].as_u32().unwrap_or(0);
        // ... parse remaining args, send via mpsc ...
        Ok(vec![Value::U32(id)])
    }));

// Signal emission (from action_rx polling):
dispatcher.emit_signal("/org/freedesktop/Notifications",
    "org.freedesktop.Notifications", "ActionInvoked",
    &[Value::U32(id), Value::String(action_key)])?;

dispatcher.run(50, |d| {
    // poll action_rx, emit signals
});
```

### 3. notify/types.rs

**Before:** `hints: HashMap<String, OwnedValue>`
**After:** `hints: HashMap<String, linbus::Value>`

### 4. notify/render.rs (variant parsing — Phase 8)

**Before:** `zbus::zvariant::Value::Structure`, `downcast_ref`, `fields()`

**After:**
```rust
fn parse_image_data(value: &linbus::Value) -> Option<gdk::Texture> {
    let fields = value.as_struct_fields()?;
    let width = fields[0].as_i32()?;
    let height = fields[1].as_i32()?;
    let rowstride = fields[2].as_i32()?;
    let has_alpha = fields[3].as_bool()?;
    let data: Vec<u8> = fields[6].as_array()?
        .iter().filter_map(|v| v.as_u8()).collect();
    // ... same texture creation logic ...
}
```

**Before (hints image-path):** `<&str>::try_from(image_path)`
**After:** `image_path.as_str()`

### 5. tray/item.rs (property reads — Phase 5+6)

**Before:** `zbus::Proxy::new()`, `proxy.get_property::<T>()`

**After:**
```rust
pub fn read_item_props(conn: &linbus::Connection, bus_name: &str, object_path: &str)
    -> Result<TrayItemProps, linbus::LinbusError>
{
    let proxy = linbus::Proxy::new(conn, bus_name, object_path, "org.kde.StatusNotifierItem");
    let title = proxy.get_property("Title")?.as_str().unwrap_or("").to_string();
    let icon_name = proxy.get_property("IconName")?.as_str().unwrap_or("").to_string();
    // ...
}
```

### 6. tray/dbusmenu.rs (complex variants — Phase 5+8)

**Before:** `zbus::zvariant::Structure`, `Dict`, `Array`, `downcast_ref`, `try_clone`

**After:**
```rust
fn prop_string(props: &HashMap<String, linbus::Value>, key: &str) -> Option<String> {
    props.get(key)?.as_variant()?.as_str().map(|s| s.to_string())
}

fn parse_children(children_raw: &[linbus::Value]) -> Vec<MenuItem> {
    for child_val in children_raw {
        let inner = child_val.as_variant().unwrap_or(child_val);
        let fields = inner.as_struct_fields()?;
        let id = fields[0].as_i32()?;
        let props = fields[1].to_string_dict().unwrap_or_default();
        let sub_children = fields[2].as_array().unwrap_or(&[]);
        // ...
    }
}

fn get_layout(conn: &linbus::Connection, bus_name: &str, menu_path: &str) -> Result<MenuItem, LinbusError> {
    let proxy = linbus::Proxy::new(conn, bus_name, menu_path, "com.canonical.dbusmenu");
    let result = proxy.call("GetLayout", &[
        Value::I32(0), Value::I32(-1), Value::Array(vec![]),
    ])?;
    // result[0] = u32 revision, result[1] = struct(i32, a{sv}, av)
}
```

### 7. tray/render.rs (method calls — Phase 5)

**Before:** `zbus::Connection::session().await`, `zbus::Proxy::new`, `proxy.call`

**After:**
```rust
fn invoke_item_method(id: &TrayItemId, method: &str, x: i32, y: i32) {
    let bus_name = id.bus_name.clone();
    let object_path = id.object_path.clone();
    let method = method.to_string();
    std::thread::spawn(move || {
        let conn = match linbus::Connection::session() {
            Ok(c) => c, Err(e) => { log::warn!("..."); return; }
        };
        let proxy = linbus::Proxy::new(&conn, &bus_name, &object_path,
            "org.kde.StatusNotifierItem");
        if let Err(e) = proxy.call(&method, &[Value::I32(x), Value::I32(y)]) {
            log::warn!("Tray {} failed: {}", method, e);
        }
    });
}
```

**Note:** No more `async_io::block_on` — linbus is synchronous. Thread-per-call pattern preserved.

### 8. tray/watcher.rs (server + signal listening — Phase 3+4+7)

**Before:** `#[interface]` macro, `MatchRule`, `MessageStream`, `futures_util::StreamExt`

**After:**
```rust
fn run_watcher(tx: mpsc::Sender<TrayEvent>) -> Result<(), LinbusError> {
    let conn = linbus::Connection::session()?;
    conn.request_name("org.kde.StatusNotifierWatcher")?;
    conn.add_match("type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged'")?;

    let mut dispatcher = linbus::Dispatcher::new(conn);

    // Properties
    dispatcher.set_property("org.kde.StatusNotifierWatcher",
        "IsStatusNotifierHostRegistered", Value::Bool(true));
    dispatcher.set_property("org.kde.StatusNotifierWatcher",
        "ProtocolVersion", Value::I32(0));

    // Method: RegisterStatusNotifierItem
    dispatcher.add_handler("org.kde.StatusNotifierWatcher",
        "RegisterStatusNotifierItem",
        Box::new(move |msg| {
            let service = msg.body[0].as_str().unwrap_or("");
            let sender = msg.sender.as_deref().unwrap_or("");
            // ... register item, emit signal ...
            Ok(vec![])
        }));

    // Event loop — dispatches method calls AND receives signals
    dispatcher.run(500, |d| {
        // Check for NameOwnerChanged signals in pending messages
        // Load properties for newly registered items
    });
}
```

**Key simplification:** No `async/await` at all. The `Dispatcher::run()` loop handles both
incoming method calls AND signal monitoring using `epoll_wait` with timeout.

---

## Dependencies Removed After Full Migration

```toml
# Remove from Cargo.toml:
zbus = "5"           # ~50 transitive deps including tokio-like runtime
async-io = "2"       # epoll abstraction (linbus uses nix directly)
futures-util = "0.3"  # only used for StreamExt on MessageStream
```

**Keep:** `nix` (already a dependency, add `uio` feature for `IoSlice`).

---

## Implementation Phases — Build Order

| Phase | Module | Lines | What It Enables | Kernel Syscalls |
|---|---|---|---|---|
| 1 | error.rs, value.rs, signature.rs | ~120 | Type system foundation | — |
| 2 | marshal.rs | ~200 | Serialize values to wire format | — |
| 3 | unmarshal.rs | ~250 | Deserialize wire format to values | — |
| 4 | message.rs | ~80 | Message construction/parsing | — |
| 5 | auth.rs, conn.rs | ~120 | Connect to bus, authenticate, Hello() | `socket`, `connect`, `getuid` |
| 6 | bus.rs | ~40 | RequestName, AddMatch helpers | `sendmsg`, `recvmsg` |
| 7 | proxy.rs | ~80 | Client method calls + property reads | — |
| 8 | dispatch.rs | ~150 | Server event loop with epoll | `epoll_create1`, `epoll_ctl`, `epoll_wait` |
| 9 | fd_pass.rs | ~40 | SCM_RIGHTS fd passing | `sendmsg`/`recvmsg` + `cmsg` |
| | **Total** | **~1080** | | |

## Migration Order — File by File

| Order | File | Validates | Complexity |
|---|---|---|---|
| 1 | lock/security.rs | conn + proxy + fd_pass | ★☆☆☆☆ (1 method call) |
| 2 | notify/types.rs | value.rs types | ★☆☆☆☆ (type alias change) |
| 3 | notify/daemon.rs | dispatch + signal emission | ★★★☆☆ (server with handlers) |
| 4 | tray/item.rs | proxy + property reads | ★★☆☆☆ (multiple get_property) |
| 5 | tray/render.rs | proxy + method calls | ★★☆☆☆ (Activate/ContextMenu) |
| 6 | notify/render.rs | value variant parsing | ★★★☆☆ (image-data struct) |
| 7 | tray/dbusmenu.rs | proxy + deep variant parsing | ★★★★☆ (recursive menu tree) |
| 8 | tray/watcher.rs | dispatch + signal listening | ★★★☆☆ (server + NameOwnerChanged) |

After step 8: remove `zbus`, `async-io`, `futures-util` from Cargo.toml.

## Verification Plan

After each migration step, verify with:

```bash
# Step 1: Lock screen VT inhibitor still works
docker compose run --rm test-lock

# Step 3: Notifications still received
notify-send "Test" "Hello World"
notify-send -t 0 "Persistent" "Won't auto-close"
dbus-send --session --dest=org.freedesktop.Notifications --type=method_call \
  /org/freedesktop/Notifications org.freedesktop.Notifications.GetServerInformation

# Step 4-5: Tray icons still appear and respond to clicks
# (manual: check nm-applet, blueman, etc.)

# Step 7: Tray menus still open and items are clickable
# (manual: right-click tray icon)

# Step 8: Tray items register/unregister correctly
# (manual: start/stop an app with tray icon)

# Final: full build with no zbus references
cargo build --release 2>&1 | grep -i zbus  # should be empty
```
