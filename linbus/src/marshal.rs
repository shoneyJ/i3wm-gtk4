use crate::signature;
use crate::value::Value;

/// Pad buffer to alignment boundary.
fn pad_to(buf: &mut Vec<u8>, alignment: usize) {
    let pad = (alignment - (buf.len() % alignment)) % alignment;
    buf.extend(std::iter::repeat(0u8).take(pad));
}

/// Marshal a single Value into the buffer according to its type.
pub fn marshal_value(buf: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Byte(v) => buf.push(*v),
        Value::Bool(v) => {
            pad_to(buf, 4);
            buf.extend_from_slice(&(*v as u32).to_le_bytes());
        }
        Value::I16(v) => {
            pad_to(buf, 2);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Value::U16(v) => {
            pad_to(buf, 2);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Value::I32(v) => {
            pad_to(buf, 4);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Value::U32(v) => {
            pad_to(buf, 4);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Value::I64(v) => {
            pad_to(buf, 8);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Value::U64(v) => {
            pad_to(buf, 8);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Value::F64(v) => {
            pad_to(buf, 8);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        Value::String(s) | Value::ObjectPath(s) => {
            pad_to(buf, 4);
            buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
            buf.extend_from_slice(s.as_bytes());
            buf.push(0); // NUL terminator
        }
        Value::Signature(s) => {
            buf.push(s.len() as u8);
            buf.extend_from_slice(s.as_bytes());
            buf.push(0);
        }
        Value::Array(elems) | Value::TypedArray(_, elems) => {
            marshal_array(buf, elems);
        }
        Value::Dict(pairs) => {
            marshal_dict(buf, pairs);
        }
        Value::Struct(fields) => {
            pad_to(buf, 8);
            for field in fields {
                marshal_value(buf, field);
            }
        }
        Value::Variant(inner) => {
            let sig = inner.signature();
            // Signature: u8 length + bytes + NUL
            buf.push(sig.len() as u8);
            buf.extend_from_slice(sig.as_bytes());
            buf.push(0);
            marshal_value(buf, inner);
        }
        Value::DictEntry(pair) => {
            pad_to(buf, 8);
            marshal_value(buf, &pair.0);
            marshal_value(buf, &pair.1);
        }
    }
}

fn marshal_array(buf: &mut Vec<u8>, elems: &[Value]) {
    pad_to(buf, 4);
    // Reserve space for the u32 byte-length
    let length_pos = buf.len();
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Pad to element alignment before writing elements
    if let Some(first) = elems.first() {
        let elem_align = signature::alignment_of(&first.signature());
        pad_to(buf, elem_align);
    }

    let data_start = buf.len();
    for elem in elems {
        marshal_value(buf, elem);
    }
    let data_len = (buf.len() - data_start) as u32;

    // Patch the length
    buf[length_pos..length_pos + 4].copy_from_slice(&data_len.to_le_bytes());
}

fn marshal_dict(buf: &mut Vec<u8>, pairs: &[(Value, Value)]) {
    pad_to(buf, 4);
    let length_pos = buf.len();
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Dict entries always align to 8
    pad_to(buf, 8);
    let data_start = buf.len();

    for (key, val) in pairs {
        pad_to(buf, 8);
        marshal_value(buf, key);
        marshal_value(buf, val);
    }

    let data_len = (buf.len() - data_start) as u32;
    buf[length_pos..length_pos + 4].copy_from_slice(&data_len.to_le_bytes());
}

/// Marshal a list of values into a body buffer, returning the body bytes and signature.
pub fn marshal_body(values: &[Value]) -> (Vec<u8>, String) {
    let mut buf = Vec::new();
    let mut sig = String::new();
    for v in values {
        sig.push_str(&v.signature());
        marshal_value(&mut buf, v);
    }
    (buf, sig)
}
