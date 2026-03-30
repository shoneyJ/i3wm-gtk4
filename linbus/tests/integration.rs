use linbus::{Connection, Message, Value};
use linbus::signature;
use linbus::marshal;
use linbus::unmarshal::Reader;

#[test]
fn test_signature_parsing() {
    let types = signature::split_signature("su").unwrap();
    assert_eq!(types, vec!["s", "u"]);

    let types = signature::split_signature("a{sv}").unwrap();
    assert_eq!(types, vec!["a{sv}"]);

    let types = signature::split_signature("(iiibiiay)").unwrap();
    assert_eq!(types, vec!["(iiibiiay)"]);

    let types = signature::split_signature("ua{sv}av").unwrap();
    assert_eq!(types, vec!["u", "a{sv}", "av"]);

    assert_eq!(signature::alignment_of("i"), 4);
    assert_eq!(signature::alignment_of("(ii)"), 8);
    assert_eq!(signature::alignment_of("a{sv}"), 4);
    assert_eq!(signature::alignment_of("y"), 1);
    assert_eq!(signature::alignment_of("t"), 8);
}

#[test]
fn test_marshal_unmarshal_roundtrip() {
    let values = vec![
        Value::String("hello".into()),
        Value::U32(42),
        Value::Bool(true),
        Value::I32(-7),
        Value::Byte(0xff),
        Value::Array(vec![Value::String("a".into()), Value::String("b".into())]),
    ];

    let (body, sig) = marshal::marshal_body(&values);
    assert_eq!(sig, "subiyas");

    let mut reader = Reader::new(&body);
    let parsed = reader.read_body(&sig).unwrap();

    assert_eq!(parsed.len(), values.len());
    assert_eq!(parsed[0].as_str(), Some("hello"));
    assert_eq!(parsed[1].as_u32(), Some(42));
    assert_eq!(parsed[2].as_bool(), Some(true));
    assert_eq!(parsed[3].as_i32(), Some(-7));
    assert_eq!(parsed[4].as_u8(), Some(0xff));

    let arr = parsed[5].as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0].as_str(), Some("a"));
    assert_eq!(arr[1].as_str(), Some("b"));
}

#[test]
fn test_marshal_unmarshal_variant() {
    let values = vec![Value::Variant(Box::new(Value::String("wrapped".into())))];
    let (body, sig) = marshal::marshal_body(&values);
    assert_eq!(sig, "v");

    let mut reader = Reader::new(&body);
    let parsed = reader.read_body(&sig).unwrap();
    let inner = parsed[0].as_variant().unwrap();
    assert_eq!(inner.as_str(), Some("wrapped"));
}

#[test]
fn test_marshal_unmarshal_dict() {
    let values = vec![Value::Dict(vec![
        (Value::String("key1".into()), Value::Variant(Box::new(Value::I32(100)))),
        (Value::String("key2".into()), Value::Variant(Box::new(Value::Bool(false)))),
    ])];

    let (body, sig) = marshal::marshal_body(&values);
    assert_eq!(sig, "a{sv}");

    let mut reader = Reader::new(&body);
    let parsed = reader.read_body(&sig).unwrap();
    let map = parsed[0].to_string_dict().unwrap();
    assert_eq!(map.len(), 2);

    let v1 = map.get("key1").unwrap().as_variant().unwrap();
    assert_eq!(v1.as_i32(), Some(100));
    let v2 = map.get("key2").unwrap().as_variant().unwrap();
    assert_eq!(v2.as_bool(), Some(false));
}

#[test]
fn test_marshal_unmarshal_struct() {
    let values = vec![Value::Struct(vec![
        Value::I32(1),
        Value::String("two".into()),
        Value::Bool(true),
    ])];

    let (body, sig) = marshal::marshal_body(&values);
    assert_eq!(sig, "(isb)");

    let mut reader = Reader::new(&body);
    let parsed = reader.read_body(&sig).unwrap();
    let fields = parsed[0].as_struct_fields().unwrap();
    assert_eq!(fields[0].as_i32(), Some(1));
    assert_eq!(fields[1].as_str(), Some("two"));
    assert_eq!(fields[2].as_bool(), Some(true));
}

#[test]
fn test_connect_session() {
    let conn = Connection::session().expect("Failed to connect to session bus");
    let name = conn.unique_name();
    assert!(name.starts_with(':'), "unique name should start with ':', got: {}", name);
    println!("Connected with unique name: {}", name);
}

#[test]
fn test_method_call_get_id() {
    let mut conn = Connection::session().expect("connect failed");
    let msg = Message::method_call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetId",
    );
    let reply = conn.call(&msg, 5000).expect("GetId call failed");
    let id = reply.body.first().and_then(|v| v.as_str()).expect("expected string");
    assert!(!id.is_empty(), "bus id should not be empty");
    println!("Bus ID: {}", id);
}

#[test]
fn test_request_name() {
    let mut conn = Connection::session().expect("connect failed");
    conn.request_name("com.linbus.test").expect("request_name failed");
    println!("Successfully claimed com.linbus.test");
}
