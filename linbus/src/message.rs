use crate::error::LinbusError;
use crate::marshal;
use crate::unmarshal::Reader;
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MessageType {
    MethodCall,
    MethodReturn,
    Error,
    Signal,
}

impl MessageType {
    fn from_u8(v: u8) -> Result<Self, LinbusError> {
        match v {
            1 => Ok(Self::MethodCall),
            2 => Ok(Self::MethodReturn),
            3 => Ok(Self::Error),
            4 => Ok(Self::Signal),
            _ => Err(LinbusError::ProtocolError(format!("unknown msg type {}", v))),
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Self::MethodCall => 1,
            Self::MethodReturn => 2,
            Self::Error => 3,
            Self::Signal => 4,
        }
    }
}

// Header field codes
const FIELD_PATH: u8 = 1;
const FIELD_INTERFACE: u8 = 2;
const FIELD_MEMBER: u8 = 3;
const FIELD_ERROR_NAME: u8 = 4;
const FIELD_REPLY_SERIAL: u8 = 5;
const FIELD_DESTINATION: u8 = 6;
const FIELD_SENDER: u8 = 7;
const FIELD_SIGNATURE: u8 = 8;

#[derive(Debug, Clone)]
pub struct Message {
    pub msg_type: MessageType,
    pub flags: u8,
    pub serial: u32,
    pub reply_serial: Option<u32>,
    pub path: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
    pub error_name: Option<String>,
    pub destination: Option<String>,
    pub sender: Option<String>,
    pub signature: Option<String>,
    pub body: Vec<Value>,
}

impl Message {
    pub fn method_call(dest: &str, path: &str, iface: &str, member: &str) -> Self {
        Self {
            msg_type: MessageType::MethodCall,
            flags: 0,
            serial: 0,
            reply_serial: None,
            path: Some(path.into()),
            interface: Some(iface.into()),
            member: Some(member.into()),
            error_name: None,
            destination: Some(dest.into()),
            sender: None,
            signature: None,
            body: Vec::new(),
        }
    }

    pub fn method_return(reply_serial: u32) -> Self {
        Self {
            msg_type: MessageType::MethodReturn,
            flags: 0,
            serial: 0,
            reply_serial: Some(reply_serial),
            path: None,
            interface: None,
            member: None,
            error_name: None,
            destination: None,
            sender: None,
            signature: None,
            body: Vec::new(),
        }
    }

    pub fn signal(path: &str, iface: &str, member: &str) -> Self {
        Self {
            msg_type: MessageType::Signal,
            flags: 1, // NO_REPLY_EXPECTED
            serial: 0,
            reply_serial: None,
            path: Some(path.into()),
            interface: Some(iface.into()),
            member: Some(member.into()),
            error_name: None,
            destination: None,
            sender: None,
            signature: None,
            body: Vec::new(),
        }
    }

    pub fn error(reply_serial: u32, error_name: &str, message: &str) -> Self {
        Self {
            msg_type: MessageType::Error,
            flags: 1,
            serial: 0,
            reply_serial: Some(reply_serial),
            path: None,
            interface: None,
            member: None,
            error_name: Some(error_name.into()),
            destination: None,
            sender: None,
            signature: Some("s".into()),
            body: vec![Value::String(message.into())],
        }
    }

    pub fn with_body(mut self, body: Vec<Value>) -> Self {
        self.body = body;
        self
    }
}

/// Serialize a message to bytes.
pub fn marshal_message(msg: &Message, serial: u32) -> Vec<u8> {
    let (body_bytes, body_sig) = marshal::marshal_body(&msg.body);

    // Build header fields array
    let mut fields: Vec<Value> = Vec::new();

    fn field(code: u8, val: Value) -> Value {
        Value::Struct(vec![Value::Byte(code), Value::Variant(Box::new(val))])
    }

    if let Some(ref p) = msg.path {
        fields.push(field(FIELD_PATH, Value::ObjectPath(p.clone())));
    }
    if let Some(ref i) = msg.interface {
        fields.push(field(FIELD_INTERFACE, Value::String(i.clone())));
    }
    if let Some(ref m) = msg.member {
        fields.push(field(FIELD_MEMBER, Value::String(m.clone())));
    }
    if let Some(ref e) = msg.error_name {
        fields.push(field(FIELD_ERROR_NAME, Value::String(e.clone())));
    }
    if let Some(rs) = msg.reply_serial {
        fields.push(field(FIELD_REPLY_SERIAL, Value::U32(rs)));
    }
    if let Some(ref d) = msg.destination {
        fields.push(field(FIELD_DESTINATION, Value::String(d.clone())));
    }
    if !body_sig.is_empty() {
        fields.push(field(FIELD_SIGNATURE, Value::Signature(body_sig)));
    }

    // Build the message directly into the output buffer so alignment is correct
    let mut buf = Vec::with_capacity(128 + body_bytes.len());

    // Fixed header (12 bytes)
    buf.push(b'l'); // little-endian
    buf.push(msg.msg_type.to_u8());
    buf.push(msg.flags);
    buf.push(1); // protocol version
    buf.extend_from_slice(&(body_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(&serial.to_le_bytes());

    // Header fields array: u32 byte-length, then struct elements
    // Reserve space for the array length
    let array_len_pos = buf.len(); // offset 12
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Marshal each field struct directly into buf (alignment relative to msg start)
    let array_data_start = buf.len(); // offset 16
    for field_val in &fields {
        marshal::marshal_value(&mut buf, field_val);
    }
    let array_data_len = (buf.len() - array_data_start) as u32;
    buf[array_len_pos..array_len_pos + 4].copy_from_slice(&array_data_len.to_le_bytes());

    // Pad header to 8-byte boundary
    let pad = (8 - (buf.len() % 8)) % 8;
    buf.extend(std::iter::repeat(0u8).take(pad));

    // Body
    buf.extend_from_slice(&body_bytes);

    buf
}

/// Read a message from a byte buffer. Returns (message, total_bytes_consumed).
pub fn read_message(data: &[u8]) -> Result<(Message, usize), LinbusError> {
    if data.len() < 16 {
        return Err(LinbusError::ProtocolError("message too short".into()));
    }

    let endian = data[0];
    if endian != b'l' && endian != b'B' {
        return Err(LinbusError::ProtocolError(format!("bad endian byte: {}", endian)));
    }
    // We only handle little-endian for now
    if endian != b'l' {
        return Err(LinbusError::ProtocolError("big-endian not supported".into()));
    }

    let msg_type = MessageType::from_u8(data[1])?;
    let flags = data[2];
    let _version = data[3];
    let body_length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let serial = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

    // Parse header fields array starting at offset 12
    let mut reader = Reader::new(data);
    reader.pos = 12;

    let header_fields_len = reader.read_u32()? as usize;
    let header_fields_end = reader.pos + header_fields_len;

    let mut path = None;
    let mut interface = None;
    let mut member = None;
    let mut error_name = None;
    let mut reply_serial = None;
    let mut destination = None;
    let mut sender = None;
    let mut signature = None;

    while reader.pos < header_fields_end {
        reader.align_to(8); // each header field struct aligns to 8
        if reader.pos >= header_fields_end { break; }
        let code = reader.read_byte()?;
        // variant: signature + value
        let field_sig = reader.read_signature()?;
        let val = reader.read_value(&field_sig)?;

        match code {
            FIELD_PATH => path = val.as_str().map(|s| s.to_string()),
            FIELD_INTERFACE => interface = val.as_str().map(|s| s.to_string()),
            FIELD_MEMBER => member = val.as_str().map(|s| s.to_string()),
            FIELD_ERROR_NAME => error_name = val.as_str().map(|s| s.to_string()),
            FIELD_REPLY_SERIAL => reply_serial = val.as_u32(),
            FIELD_DESTINATION => destination = val.as_str().map(|s| s.to_string()),
            FIELD_SENDER => sender = val.as_str().map(|s| s.to_string()),
            FIELD_SIGNATURE => {
                if let Value::Signature(s) = val {
                    signature = Some(s);
                }
            }
            _ => {} // unknown field, skip
        }
    }

    // Body starts after header padded to 8 bytes
    let total_header = 12 + 4 + header_fields_len;
    let padded_header = total_header + (8 - (total_header % 8)) % 8;
    let body_start = padded_header;

    // Parse body
    let body = if let Some(ref sig) = signature {
        let mut body_reader = Reader::new(&data[body_start..]);
        body_reader.read_body(sig)?
    } else {
        Vec::new()
    };

    let total = body_start + body_length;

    Ok((
        Message {
            msg_type,
            flags,
            serial,
            reply_serial,
            path,
            interface,
            member,
            error_name,
            destination,
            sender,
            signature,
            body,
        },
        total,
    ))
}

/// Minimum bytes needed to determine the full message length.
/// Returns None if not enough data yet.
pub fn message_length(data: &[u8]) -> Option<usize> {
    if data.len() < 16 {
        return None;
    }
    let body_length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let header_fields_len = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    let total_header = 12 + 4 + header_fields_len;
    let padded_header = total_header + (8 - (total_header % 8)) % 8;
    Some(padded_header + body_length)
}
