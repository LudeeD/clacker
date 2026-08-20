//! Wire formats. One module per provider API we impersonate.
//!
//! Adding a harness means adding a `Protocol` here (its provider's wire
//! format) plus a `Harness` that points that provider's base-URL env var at
//! us. Neither the brain nor the HN core changes.

pub mod anthropic;

use tiny_http::Request;

pub enum Reply {
    /// A complete response body with a content type.
    Full { status: u16, content_type: &'static str, body: Vec<u8> },
    /// Server-sent events, streamed frame by frame.
    Sse(Vec<String>),
}

impl Reply {
    pub fn json(status: u16, value: serde_json::Value) -> Reply {
        Reply::Full {
            status,
            content_type: "application/json",
            body: value.to_string().into_bytes(),
        }
    }
}

pub trait Protocol: Send + Sync {
    /// Human-readable id, used in `--harness` errors and debug logging.
    fn id(&self) -> &'static str;

    /// Handle a request. `body` is the already-read request body.
    fn respond(&self, req: &Request, body: &str) -> Reply;
}
