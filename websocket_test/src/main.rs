use ws::{listen, Handler, Message, Result, Sender};

struct Server {
    out: Sender,
}

impl Handler for Server {
    fn on_message(&mut self, msg: Message) -> Result<()> {
        println!("Server received: {}", msg);
        // Echo the message back
        self.out.send(format!("Echo: {}", msg))
    }
}

fn main() {
    // Start WebSocket server on port 3012
    println!("WebSocket server starting on ws://127.0.0.1:3012");
    listen("127.0.0.1:3012", |out| Server { out }).unwrap();
}
