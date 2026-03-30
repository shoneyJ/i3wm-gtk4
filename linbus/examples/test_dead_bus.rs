use linbus::{Connection, Message, Value};

fn main() {
    let mut conn = Connection::session().expect("connect failed");
    eprintln!("Connected as {}", conn.unique_name());

    // Try calling a dead bus name
    let msg = Message::method_call(
        ":1.99999",
        "/test",
        "org.freedesktop.DBus.Properties",
        "Get",
    ).with_body(vec![
        Value::String("org.kde.StatusNotifierItem".into()),
        Value::String("Title".into()),
    ]);

    eprintln!("Sending to dead bus name...");
    match conn.call(&msg, 3000) {
        Ok(reply) => eprintln!("Got reply: {:?}", reply.body),
        Err(e) => eprintln!("Got error (expected): {}", e),
    }
    eprintln!("Done!");
}
