use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::conn::Connection;
use crate::error::LinbusError;
use crate::message::{Message, MessageType};
use crate::value::Value;

pub type MethodHandler = Box<dyn Fn(&Message) -> Result<Vec<Value>, LinbusError> + Send>;

/// Server-side dispatcher: registers handlers and runs an event loop.
pub struct Dispatcher {
    pub conn: Connection,
    handlers: HashMap<(String, String), MethodHandler>, // (interface, member) → handler
    properties: HashMap<(String, String), Value>,        // (interface, property) → value
    stop_flag: Arc<AtomicBool>,
    owned_names: Vec<String>,
    match_rules: Vec<String>,
}

impl Dispatcher {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            handlers: HashMap::new(),
            properties: HashMap::new(),
            stop_flag: Arc::new(AtomicBool::new(false)),
            owned_names: Vec::new(),
            match_rules: Vec::new(),
        }
    }

    /// Get a handle to the stop flag. Set to true to break the event loop.
    pub fn stop_handle(&self) -> Arc<AtomicBool> {
        self.stop_flag.clone()
    }

    /// Track a bus name claimed via `conn.request_name()` for cleanup on drop.
    pub fn track_name(&mut self, name: &str) {
        self.owned_names.push(name.to_string());
    }

    /// Track a match rule added via `conn.add_match()` for cleanup on drop.
    pub fn track_match_rule(&mut self, rule: &str) {
        self.match_rules.push(rule.to_string());
    }

    /// Register a handler for incoming METHOD_CALLs.
    pub fn add_handler(&mut self, interface: &str, member: &str, handler: MethodHandler) {
        self.handlers.insert((interface.into(), member.into()), handler);
    }

    /// Set a property value (served via org.freedesktop.DBus.Properties).
    pub fn set_property(&mut self, interface: &str, name: &str, value: Value) {
        self.properties.insert((interface.into(), name.into()), value);
    }

    /// Emit a signal.
    pub fn emit_signal(
        &self,
        path: &str,
        interface: &str,
        member: &str,
        body: &[Value],
    ) -> Result<(), LinbusError> {
        let msg = Message::signal(path, interface, member)
            .with_body(body.to_vec());
        self.conn.send_msg(&msg)?;
        Ok(())
    }

    /// Dispatch a single incoming message. Returns true if it was handled.
    pub fn dispatch_one(&self, msg: &Message) -> Result<bool, LinbusError> {
        if msg.msg_type != MessageType::MethodCall {
            return Ok(false);
        }

        let iface = msg.interface.as_deref().unwrap_or("");
        let member = msg.member.as_deref().unwrap_or("");

        // Handle Peer.Ping
        if iface == "org.freedesktop.DBus.Peer" && member == "Ping" {
            let reply = Message::method_return(msg.serial);
            self.conn.send_msg(&reply)?;
            return Ok(true);
        }

        // Check user-registered handlers first (they override built-in Properties)
        let key = (iface.to_string(), member.to_string());
        if let Some(handler) = self.handlers.get(&key) {
            match handler(msg) {
                Ok(body) => {
                    if msg.flags & 0x1 == 0 {
                        let reply = Message::method_return(msg.serial).with_body(body);
                        self.conn.send_msg(&reply)?;
                    }
                    return Ok(true);
                }
                Err(e) => {
                    let reply = Message::error(
                        msg.serial,
                        "org.freedesktop.DBus.Error.Failed",
                        &e.to_string(),
                    );
                    self.conn.send_msg(&reply)?;
                    return Ok(true);
                }
            }
        }

        // Built-in Properties.Get (fallback when no user handler registered)
        if iface == "org.freedesktop.DBus.Properties" && member == "Get" {
            return self.handle_properties_get(msg);
        }

        // Built-in Properties.GetAll (fallback when no user handler registered)
        if iface == "org.freedesktop.DBus.Properties" && member == "GetAll" {
            return self.handle_properties_get_all(msg);
        }

        // Unknown method
        if msg.flags & 0x1 == 0 {
            let reply = Message::error(
                msg.serial,
                "org.freedesktop.DBus.Error.UnknownMethod",
                &format!("No handler for {}.{}", iface, member),
            );
            self.conn.send_msg(&reply)?;
        }
        Ok(false)
    }

    fn handle_properties_get(&self, msg: &Message) -> Result<bool, LinbusError> {
        let iface = msg.body.first().and_then(|v| v.as_str()).unwrap_or("");
        let prop = msg.body.get(1).and_then(|v| v.as_str()).unwrap_or("");

        let key = (iface.to_string(), prop.to_string());
        if let Some(value) = self.properties.get(&key) {
            let reply = Message::method_return(msg.serial)
                .with_body(vec![Value::Variant(Box::new(value.clone()))]);
            self.conn.send_msg(&reply)?;
        } else {
            let reply = Message::error(
                msg.serial,
                "org.freedesktop.DBus.Error.UnknownProperty",
                &format!("{}.{} not found", iface, prop),
            );
            self.conn.send_msg(&reply)?;
        }
        Ok(true)
    }

    fn handle_properties_get_all(&self, msg: &Message) -> Result<bool, LinbusError> {
        let iface = msg.body.first().and_then(|v| v.as_str()).unwrap_or("");

        let mut pairs = Vec::new();
        for ((i, name), value) in &self.properties {
            if i == iface {
                pairs.push((
                    Value::String(name.clone()),
                    Value::Variant(Box::new(value.clone())),
                ));
            }
        }

        let reply = Message::method_return(msg.serial)
            .with_body(vec![Value::Dict(pairs)]);
        self.conn.send_msg(&reply)?;
        Ok(true)
    }

    /// Run the event loop. Dispatches method calls to handlers.
    /// Calls `idle_fn` on every timeout cycle.
    /// Passes signals to `signal_fn`.
    /// Exits cleanly when `stop_flag` is set to true.
    pub fn run<F, S>(
        &mut self,
        idle_ms: u16,
        mut idle_fn: F,
        mut signal_fn: S,
    ) -> Result<(), LinbusError>
    where
        F: FnMut(&mut Self),
        S: FnMut(&Message),
    {
        loop {
            if self.stop_flag.load(Ordering::Relaxed) {
                break;
            }

            match self.conn.recv_blocking(idle_ms) {
                Ok(Some(msg)) => {
                    match msg.msg_type {
                        MessageType::Signal => signal_fn(&msg),
                        MessageType::MethodCall => { self.dispatch_one(&msg)?; }
                        _ => {}
                    }
                }
                Ok(None) => {
                    idle_fn(self);
                }
                Err(_) => {
                    idle_fn(self);
                }
            }
        }

        Ok(())
    }
}

impl Drop for Dispatcher {
    fn drop(&mut self) {
        // Best-effort cleanup: release names and remove match rules
        for name in &self.owned_names {
            if let Err(e) = self.conn.release_name(name) {
                log::debug!("Failed to release name {}: {}", name, e);
            }
        }
        for rule in &self.match_rules {
            if let Err(e) = self.conn.remove_match(rule) {
                log::debug!("Failed to remove match rule: {}", e);
            }
        }
    }
}
