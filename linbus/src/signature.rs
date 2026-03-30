use crate::error::LinbusError;

/// Split a D-Bus type signature string into individual complete types.
/// e.g. "su" → ["s", "u"], "a{sv}" → ["a{sv}"], "(iiibiiay)" → ["(iiibiiay)"]
pub fn split_signature(sig: &str) -> Result<Vec<String>, LinbusError> {
    let bytes = sig.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let (token, consumed) = parse_single_type(bytes, i)?;
        result.push(token);
        i += consumed;
    }
    Ok(result)
}

fn parse_single_type(bytes: &[u8], start: usize) -> Result<(String, usize), LinbusError> {
    if start >= bytes.len() {
        return Err(LinbusError::InvalidSignature("unexpected end".into()));
    }
    match bytes[start] {
        b'y' | b'b' | b'n' | b'q' | b'i' | b'u' | b'x' | b't' | b'd'
        | b's' | b'o' | b'g' | b'v' | b'h' => {
            Ok((String::from(bytes[start] as char), 1))
        }
        b'a' => {
            if start + 1 >= bytes.len() {
                return Err(LinbusError::InvalidSignature("array without element type".into()));
            }
            if bytes[start + 1] == b'{' {
                // dict: a{...}
                let (inner, consumed) = parse_dict_entry(bytes, start + 1)?;
                Ok((format!("a{}", inner), 1 + consumed))
            } else {
                let (inner, consumed) = parse_single_type(bytes, start + 1)?;
                Ok((format!("a{}", inner), 1 + consumed))
            }
        }
        b'(' => {
            let mut depth = 1;
            let mut end = start + 1;
            while end < bytes.len() && depth > 0 {
                if bytes[end] == b'(' { depth += 1; }
                if bytes[end] == b')' { depth -= 1; }
                end += 1;
            }
            if depth != 0 {
                return Err(LinbusError::InvalidSignature("unmatched '('".into()));
            }
            let s = std::str::from_utf8(&bytes[start..end])
                .map_err(|_| LinbusError::InvalidSignature("invalid utf8".into()))?;
            Ok((s.to_string(), end - start))
        }
        b'{' => {
            // Dict entry — same bracket matching as struct but with {}
            let (inner, consumed) = parse_dict_entry(bytes, start)?;
            Ok((inner, consumed))
        }
        _ => Err(LinbusError::InvalidSignature(
            format!("unknown type code '{}'", bytes[start] as char),
        )),
    }
}

fn parse_dict_entry(bytes: &[u8], start: usize) -> Result<(String, usize), LinbusError> {
    if bytes[start] != b'{' {
        return Err(LinbusError::InvalidSignature("expected '{'".into()));
    }
    let mut depth = 1;
    let mut end = start + 1;
    while end < bytes.len() && depth > 0 {
        if bytes[end] == b'{' { depth += 1; }
        if bytes[end] == b'}' { depth -= 1; }
        end += 1;
    }
    if depth != 0 {
        return Err(LinbusError::InvalidSignature("unmatched '{'".into()));
    }
    let s = std::str::from_utf8(&bytes[start..end])
        .map_err(|_| LinbusError::InvalidSignature("invalid utf8".into()))?;
    Ok((s.to_string(), end - start))
}

/// Returns the alignment requirement for a given signature type.
pub fn alignment_of(sig: &str) -> usize {
    match sig.as_bytes().first() {
        Some(b'y') => 1,  // BYTE
        Some(b'b') => 4,  // BOOLEAN (u32)
        Some(b'n' | b'q') => 2,  // INT16 / UINT16
        Some(b'i' | b'u' | b'h') => 4,  // INT32 / UINT32 / UNIX_FD
        Some(b'x' | b't' | b'd') => 8,  // INT64 / UINT64 / DOUBLE
        Some(b's' | b'o') => 4,  // STRING / OBJECT_PATH
        Some(b'g') => 1,  // SIGNATURE
        Some(b'a') => 4,  // ARRAY (length u32)
        Some(b'(') => 8,  // STRUCT
        Some(b'{') => 8,  // DICT_ENTRY
        Some(b'v') => 1,  // VARIANT (starts with signature byte)
        _ => 1,
    }
}

/// Parse the struct fields inside a "(...)".
/// Input should include the parens: "(isu)"
pub fn parse_struct_fields(sig: &str) -> Result<Vec<String>, LinbusError> {
    if !sig.starts_with('(') || !sig.ends_with(')') {
        return Err(LinbusError::InvalidSignature(format!("not a struct: {}", sig)));
    }
    split_signature(&sig[1..sig.len() - 1])
}

/// Parse dict entry key and value types from "{kv}".
pub fn parse_dict_key_value(sig: &str) -> Result<(String, String), LinbusError> {
    if !sig.starts_with('{') || !sig.ends_with('}') {
        return Err(LinbusError::InvalidSignature(format!("not a dict entry: {}", sig)));
    }
    let inner = &sig[1..sig.len() - 1];
    let types = split_signature(inner)?;
    if types.len() != 2 {
        return Err(LinbusError::InvalidSignature(
            format!("dict entry must have 2 types, got {}", types.len()),
        ));
    }
    Ok((types[0].clone(), types[1].clone()))
}

/// Get the element signature of an array. "ai" → "i", "a{sv}" → "{sv}", "a(ii)" → "(ii)"
pub fn array_element_sig(sig: &str) -> Result<String, LinbusError> {
    if !sig.starts_with('a') || sig.len() < 2 {
        return Err(LinbusError::InvalidSignature(format!("not an array: {}", sig)));
    }
    let types = split_signature(&sig[1..])?;
    if types.len() != 1 {
        return Err(LinbusError::InvalidSignature(
            format!("array element sig parsed to {} types", types.len()),
        ));
    }
    Ok(types[0].clone())
}
