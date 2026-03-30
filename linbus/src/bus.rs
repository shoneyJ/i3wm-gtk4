use crate::conn::Connection;
use crate::error::LinbusError;
use crate::message::Message;
use crate::value::Value;

impl Connection {
    /// Claim a well-known bus name (e.g. "org.freedesktop.Notifications").
    /// Uses REPLACE_EXISTING | DO_NOT_QUEUE flags to take over from existing owners.
    pub fn request_name(&mut self, name: &str) -> Result<(), LinbusError> {
        // Flags: ALLOW_REPLACEMENT=0x1 | REPLACE_EXISTING=0x2 | DO_NOT_QUEUE=0x4
        let flags = 0x1 | 0x2 | 0x4;
        let msg = Message::method_call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
        ).with_body(vec![
            Value::String(name.into()),
            Value::U32(flags),
        ]);

        let reply = self.call(&msg, 5000)?;
        let code = reply.body.first().and_then(|v| v.as_u32()).unwrap_or(0);
        // 1 = PRIMARY_OWNER, 4 = ALREADY_OWNER — both are success
        if code != 1 && code != 4 {
            return Err(LinbusError::NameRequestFailed(code));
        }
        Ok(())
    }

    /// Release a previously claimed bus name.
    pub fn release_name(&mut self, name: &str) -> Result<(), LinbusError> {
        let msg = Message::method_call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "ReleaseName",
        ).with_body(vec![Value::String(name.into())]);
        self.call(&msg, 2000)?;
        Ok(())
    }

    /// Register a match rule for signal filtering.
    pub fn add_match(&mut self, rule: &str) -> Result<(), LinbusError> {
        let msg = Message::method_call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "AddMatch",
        ).with_body(vec![Value::String(rule.into())]);

        self.call(&msg, 5000)?;
        Ok(())
    }

    /// Remove a previously added match rule.
    pub fn remove_match(&mut self, rule: &str) -> Result<(), LinbusError> {
        let msg = Message::method_call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RemoveMatch",
        ).with_body(vec![Value::String(rule.into())]);
        self.call(&msg, 2000)?;
        Ok(())
    }
}
