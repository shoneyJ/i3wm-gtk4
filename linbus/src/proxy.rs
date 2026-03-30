use crate::conn::Connection;
use crate::error::LinbusError;
use crate::message::Message;
use crate::value::Value;
use std::os::fd::OwnedFd;

/// Client-side proxy for calling methods on a remote D-Bus object.
pub struct Proxy<'a> {
    conn: &'a mut Connection,
    destination: String,
    path: String,
    interface: String,
}

impl<'a> Proxy<'a> {
    pub fn new(conn: &'a mut Connection, dest: &str, path: &str, iface: &str) -> Self {
        Self {
            conn,
            destination: dest.into(),
            path: path.into(),
            interface: iface.into(),
        }
    }

    /// Call a method and wait for the reply. Returns the body values.
    pub fn call(&mut self, method: &str, body: Vec<Value>) -> Result<Vec<Value>, LinbusError> {
        let msg = Message::method_call(&self.destination, &self.path, &self.interface, method)
            .with_body(body);
        let reply = self.conn.call(&msg, 3000)?;
        Ok(reply.body)
    }

    /// Call a method, also receiving file descriptors in the reply.
    pub fn call_with_fds(&mut self, method: &str, body: Vec<Value>) -> Result<(Vec<Value>, Vec<OwnedFd>), LinbusError> {
        let msg = Message::method_call(&self.destination, &self.path, &self.interface, method)
            .with_body(body);
        let (reply, fds) = self.conn.call_with_fds(&msg, 10000)?;
        Ok((reply.body, fds))
    }

    /// Read a property via org.freedesktop.DBus.Properties.Get.
    pub fn get_property(&mut self, property_name: &str) -> Result<Value, LinbusError> {
        let msg = Message::method_call(
            &self.destination,
            &self.path,
            "org.freedesktop.DBus.Properties",
            "Get",
        ).with_body(vec![
            Value::String(self.interface.clone()),
            Value::String(property_name.into()),
        ]);

        let reply = self.conn.call(&msg, 3000)?;
        // Properties.Get returns a single variant
        reply.body.into_iter().next()
            .ok_or_else(|| LinbusError::ProtocolError("empty Properties.Get reply".into()))
    }
}
