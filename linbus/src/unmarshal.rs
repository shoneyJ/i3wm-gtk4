use crate::error::LinbusError;
use crate::signature;
use crate::value::Value;

/// Reader that tracks position for alignment calculations.
pub struct Reader<'a> {
    data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn check(&self, n: usize) -> Result<(), LinbusError> {
        if self.pos + n > self.data.len() {
            Err(LinbusError::ProtocolError(format!(
                "read past end: pos={} need={} have={}",
                self.pos, n, self.data.len()
            )))
        } else {
            Ok(())
        }
    }

    pub fn align_to(&mut self, alignment: usize) {
        let pad = (alignment - (self.pos % alignment)) % alignment;
        self.pos += pad;
    }

    pub fn read_byte(&mut self) -> Result<u8, LinbusError> {
        self.check(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn read_u16(&mut self) -> Result<u16, LinbusError> {
        self.align_to(2);
        self.check(2)?;
        let v = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    pub fn read_i16(&mut self) -> Result<i16, LinbusError> {
        Ok(self.read_u16()? as i16)
    }

    pub fn read_u32(&mut self) -> Result<u32, LinbusError> {
        self.align_to(4);
        self.check(4)?;
        let v = u32::from_le_bytes([
            self.data[self.pos], self.data[self.pos + 1],
            self.data[self.pos + 2], self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    pub fn read_i32(&mut self) -> Result<i32, LinbusError> {
        Ok(self.read_u32()? as i32)
    }

    pub fn read_u64(&mut self) -> Result<u64, LinbusError> {
        self.align_to(8);
        self.check(8)?;
        let v = u64::from_le_bytes([
            self.data[self.pos], self.data[self.pos + 1],
            self.data[self.pos + 2], self.data[self.pos + 3],
            self.data[self.pos + 4], self.data[self.pos + 5],
            self.data[self.pos + 6], self.data[self.pos + 7],
        ]);
        self.pos += 8;
        Ok(v)
    }

    pub fn read_i64(&mut self) -> Result<i64, LinbusError> {
        Ok(self.read_u64()? as i64)
    }

    pub fn read_f64(&mut self) -> Result<f64, LinbusError> {
        Ok(f64::from_bits(self.read_u64()?))
    }

    pub fn read_string(&mut self) -> Result<String, LinbusError> {
        let len = self.read_u32()? as usize;
        self.check(len + 1)?; // +1 for NUL
        let s = std::str::from_utf8(&self.data[self.pos..self.pos + len])
            .map_err(|e| LinbusError::ProtocolError(format!("invalid utf8: {}", e)))?;
        self.pos += len + 1; // skip NUL
        Ok(s.to_string())
    }

    pub fn read_object_path(&mut self) -> Result<String, LinbusError> {
        self.read_string()
    }

    pub fn read_signature(&mut self) -> Result<String, LinbusError> {
        let len = self.read_byte()? as usize;
        self.check(len + 1)?;
        let s = std::str::from_utf8(&self.data[self.pos..self.pos + len])
            .map_err(|e| LinbusError::ProtocolError(format!("invalid sig utf8: {}", e)))?;
        self.pos += len + 1;
        Ok(s.to_string())
    }

    pub fn read_bool(&mut self) -> Result<bool, LinbusError> {
        let v = self.read_u32()?;
        Ok(v != 0)
    }

    /// Read a value given its type signature.
    pub fn read_value(&mut self, sig: &str) -> Result<Value, LinbusError> {
        let first = sig.as_bytes().first()
            .ok_or_else(|| LinbusError::InvalidSignature("empty sig".into()))?;

        match first {
            b'y' => Ok(Value::Byte(self.read_byte()?)),
            b'b' => Ok(Value::Bool(self.read_bool()?)),
            b'n' => Ok(Value::I16(self.read_i16()?)),
            b'q' => Ok(Value::U16(self.read_u16()?)),
            b'i' => Ok(Value::I32(self.read_i32()?)),
            b'u' => Ok(Value::U32(self.read_u32()?)),
            b'x' => Ok(Value::I64(self.read_i64()?)),
            b't' => Ok(Value::U64(self.read_u64()?)),
            b'd' => Ok(Value::F64(self.read_f64()?)),
            b's' => Ok(Value::String(self.read_string()?)),
            b'o' => Ok(Value::ObjectPath(self.read_object_path()?)),
            b'g' => Ok(Value::Signature(self.read_signature()?)),
            b'v' => self.read_variant(),
            b'a' => {
                if sig.len() > 1 && sig.as_bytes()[1] == b'{' {
                    self.read_dict(sig)
                } else {
                    self.read_array(sig)
                }
            }
            b'(' => self.read_struct(sig),
            b'h' => Ok(Value::U32(self.read_u32()?)), // UNIX_FD index
            _ => Err(LinbusError::InvalidSignature(format!("unknown: {}", sig))),
        }
    }

    fn read_variant(&mut self) -> Result<Value, LinbusError> {
        let sig = self.read_signature()?;
        let inner = self.read_value(&sig)?;
        Ok(Value::Variant(Box::new(inner)))
    }

    fn read_array(&mut self, sig: &str) -> Result<Value, LinbusError> {
        let elem_sig = signature::array_element_sig(sig)?;
        let byte_len = self.read_u32()? as usize;

        // Align to element alignment
        let elem_align = signature::alignment_of(&elem_sig);
        self.align_to(elem_align);

        let end = self.pos + byte_len;
        let mut elems = Vec::new();
        while self.pos < end {
            elems.push(self.read_value(&elem_sig)?);
        }
        Ok(Value::Array(elems))
    }

    pub fn read_dict(&mut self, sig: &str) -> Result<Value, LinbusError> {
        // sig is either "a{sv}" (array of dict entries) or "{sv}" (single dict entry)
        let (key_sig, val_sig) = if sig.starts_with('a') {
            let elem_sig = signature::array_element_sig(sig)?;
            signature::parse_dict_key_value(&elem_sig)?
        } else {
            signature::parse_dict_key_value(sig)?
        };

        let byte_len = self.read_u32()? as usize;
        self.align_to(8); // dict entries align to 8

        let end = self.pos + byte_len;
        let mut pairs = Vec::new();
        while self.pos < end {
            self.align_to(8);
            let key = self.read_value(&key_sig)?;
            let val = self.read_value(&val_sig)?;
            pairs.push((key, val));
        }
        Ok(Value::Dict(pairs))
    }

    fn read_struct(&mut self, sig: &str) -> Result<Value, LinbusError> {
        self.align_to(8);
        let field_sigs = signature::parse_struct_fields(sig)?;
        let mut fields = Vec::with_capacity(field_sigs.len());
        for fs in &field_sigs {
            fields.push(self.read_value(fs)?);
        }
        Ok(Value::Struct(fields))
    }

    /// Read body values given a signature string.
    pub fn read_body(&mut self, sig: &str) -> Result<Vec<Value>, LinbusError> {
        if sig.is_empty() {
            return Ok(Vec::new());
        }
        let types = signature::split_signature(sig)?;
        let mut values = Vec::with_capacity(types.len());
        for t in &types {
            values.push(self.read_value(t)?);
        }
        Ok(values)
    }
}
