use linbus::Connection;

fn main() {
    let conn = Connection::session().expect("Failed to connect to session bus");
    println!("Connected to session bus!");
    println!("Unique name: {}", conn.unique_name());
}
