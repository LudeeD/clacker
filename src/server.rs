//! Local HTTP server the harness talks to instead of the real provider.

use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tiny_http::{Header, Response, Server};

use crate::protocol::anthropic;
use crate::protocol::{Protocol, Reply};

/// Pause between SSE frames. Without it the whole reply lands in one paint and
/// the harness never shows it streaming in.
const FRAME_DELAY: Duration = Duration::from_millis(4);

const WORKERS: usize = 4;

pub struct Running {
    pub addr: SocketAddr,
}

/// Bind on an ephemeral loopback port and serve in the background.
pub fn spawn(protocol: Box<dyn Protocol>) -> std::io::Result<Running> {
    let server = Server::http("127.0.0.1:0")
        .map_err(|e| std::io::Error::other(format!("couldn't bind a local port: {e}")))?;
    let addr = match server.server_addr() {
        tiny_http::ListenAddr::IP(addr) => addr,
        other => return Err(std::io::Error::other(format!("unexpected listen addr: {other:?}"))),
    };

    let server = Arc::new(server);
    let protocol: Arc<dyn Protocol> = Arc::from(protocol);

    for _ in 0..WORKERS {
        let server = Arc::clone(&server);
        let protocol = Arc::clone(&protocol);
        std::thread::spawn(move || {
            while let Ok(mut request) = server.recv() {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                anthropic::debug_log(&format!("{} {}", request.method(), request.url()));
                let reply = protocol.respond(&request, &body);
                let _ = deliver(request, reply);
            }
        });
    }

    Ok(Running { addr })
}

fn deliver(request: tiny_http::Request, reply: Reply) -> std::io::Result<()> {
    match reply {
        Reply::Full { status, content_type, body } => {
            let header = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
                .expect("static header");
            request.respond(Response::from_data(body).with_status_code(status).with_header(header))
        }
        // SSE needs frame-by-frame flushing, so we write the response by hand.
        // Chunked encoding is what tells the client where the body ends —
        // without it the client blocks waiting for the connection to close.
        Reply::Sse(frames) => {
            let mut writer = request.into_writer();
            writer.write_all(
                b"HTTP/1.1 200 OK\r\n\
                  Content-Type: text/event-stream\r\n\
                  Cache-Control: no-cache\r\n\
                  Transfer-Encoding: chunked\r\n\r\n",
            )?;
            writer.flush()?;

            for frame in frames {
                write!(writer, "{:X}\r\n", frame.len())?;
                writer.write_all(frame.as_bytes())?;
                writer.write_all(b"\r\n")?;
                writer.flush()?;
                std::thread::sleep(FRAME_DELAY);
            }
            writer.write_all(b"0\r\n\r\n")?;
            writer.flush()
        }
    }
}
