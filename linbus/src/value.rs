use std::collections::HashMap;

/// D-Bus value — represents any type in the D-Bus type system.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Byte(u8),
    Bool(bool),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F64(f64),
    String(String),
    ObjectPath(String),
    Signature(String),
    /// Array with inferred element type from first element.
    Array(Vec<Value>),
    /// Array with explicit element signature (needed for empty arrays).
    TypedArray(std::string::String, Vec<Value>),
    Struct(Vec<Value>),
    Variant(Box<Value>),
    DictEntry(Box<(Value, Value)>),
    Dict(Vec<(Value, Value)>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) | Value::ObjectPath(s) => Some(s),
            Value::Variant(v) => v.as_str(),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Variant(v) => v.as_bool(),
            _ => None,
        }
    }

    pub fn as_u8(&self) -> Option<u8> {
        match self {
            Value::Byte(v) => Some(*v),
            Value::Variant(inner) => inner.as_u8(),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Value::I32(v) => Some(*v),
            Value::Variant(inner) => inner.as_i32(),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Value::U32(v) => Some(*v),
            Value::Variant(inner) => inner.as_u32(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::I64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::U64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(v) | Value::TypedArray(_, v) => Some(v),
            Value::Variant(inner) => inner.as_array(),
            _ => None,
        }
    }

    pub fn as_struct_fields(&self) -> Option<&[Value]> {
        match self {
            Value::Struct(v) => Some(v),
            Value::Variant(inner) => inner.as_struct_fields(),
            _ => None,
        }
    }

    pub fn as_variant(&self) -> Option<&Value> {
        match self {
            Value::Variant(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_dict_pairs(&self) -> Option<&[(Value, Value)]> {
        match self {
            Value::Dict(v) => Some(v),
            Value::Variant(inner) => inner.as_dict_pairs(),
            _ => None,
        }
    }

    /// Convert a Dict of String keys to a HashMap for convenient access.
    pub fn to_string_dict(&self) -> Option<HashMap<String, Value>> {
        let pairs = self.as_dict_pairs()?;
        let mut map = HashMap::with_capacity(pairs.len());
        for (k, v) in pairs {
            if let Some(key) = k.as_str() {
                map.insert(key.to_string(), v.clone());
            }
        }
        Some(map)
    }

    /// Returns the D-Bus type signature character(s) for this value.
    pub fn signature(&self) -> String {
        match self {
            Value::Byte(_) => "y".into(),
            Value::Bool(_) => "b".into(),
            Value::I16(_) => "n".into(),
            Value::U16(_) => "q".into(),
            Value::I32(_) => "i".into(),
            Value::U32(_) => "u".into(),
            Value::I64(_) => "x".into(),
            Value::U64(_) => "t".into(),
            Value::F64(_) => "d".into(),
            Value::String(_) => "s".into(),
            Value::ObjectPath(_) => "o".into(),
            Value::Signature(_) => "g".into(),
            Value::Array(elems) => {
                let inner = elems.first().map(|v| v.signature()).unwrap_or_else(|| "v".into());
                format!("a{}", inner)
            }
            Value::TypedArray(sig, _) => format!("a{}", sig),
            Value::Dict(pairs) => {
                let (ks, vs) = pairs.first()
                    .map(|(k, v)| (k.signature(), v.signature()))
                    .unwrap_or_else(|| ("s".into(), "v".into()));
                format!("a{{{}{}}}", ks, vs)
            }
            Value::Struct(fields) => {
                let inner: String = fields.iter().map(|f| f.signature()).collect();
                format!("({})", inner)
            }
            Value::Variant(_) => "v".into(),
            Value::DictEntry(pair) => format!("{{{}{}}}", pair.0.signature(), pair.1.signature()),
        }
    }
}
