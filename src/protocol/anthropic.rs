//! Anthropic Messages API, impersonated well enough for Claude Code.
//!
//! Endpoints the CLI actually uses: `/v1/messages`, `/v1/messages/count_tokens`
//! and `/v1/models`. Anything else 404s and is logged under `CLACKER_DEBUG=1`.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use tiny_http::Request;

use super::{Protocol, Reply};
use crate::brain::{self, Block, Message, Stop, Turn};

pub const MODEL: &str = "hackernews-1";

pub struct Anthropic;

impl Protocol for Anthropic {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn respond(&self, req: &Request, body: &str) -> Reply {
        let path = req.url().split('?').next().unwrap_or("");
        match path {
            "/v1/messages" => messages(body),
            "/v1/messages/count_tokens" => {
                Reply::json(200, json!({"input_tokens": body.len() / 4}))
            }
            // Connectivity probe the CLI makes before its first request.
            "/api/hello" => Reply::json(200, json!({})),
            "/v1/models" => Reply::json(
                200,
                json!({
                    "data": [{
                        "type": "model",
                        "id": MODEL,
                        "display_name": "Hacker News",
                        "created_at": "2026-01-01T00:00:00Z",
                    }],
                    "has_more": false,
                }),
            ),
            other => {
                debug_log(&format!("unhandled {} {other}", req.method()));
                Reply::json(
                    404,
                    json!({"type": "error", "error": {"type": "not_found_error", "message": format!("clacker doesn't serve {other}")}}),
                )
            }
        }
    }
}

fn messages(body: &str) -> Reply {
    let Ok(request) = serde_json::from_str::<Value>(body) else {
        return Reply::json(
            400,
            json!({"type": "error", "error": {"type": "invalid_request_error", "message": "bad JSON"}}),
        );
    };

    let turn = parse_turn(&request);
    debug_log(&format!(
        "turn: {} tools, {} messages, last={}",
        turn.tools.len(),
        turn.messages.len(),
        match turn.messages.last() {
            Some(crate::brain::Message::User(t)) => format!("user({:?})", &t.chars().take(60).collect::<String>()),
            Some(crate::brain::Message::Assistant(t)) => format!("assistant({:?})", &t.chars().take(40).collect::<String>()),
            Some(crate::brain::Message::ToolResult(t)) => format!("tool_result({:?})", &t.chars().take(40).collect::<String>()),
            None => "none".to_string(),
        }
    ));
    let reply = brain::respond(&turn);
    let input_tokens = body.len() / 4;

    if request["stream"].as_bool().unwrap_or(false) {
        Reply::Sse(stream_frames(&reply, input_tokens))
    } else {
        Reply::json(200, message_object(&reply, input_tokens))
    }
}

/// Wire format -> the brain's neutral view of the conversation.
fn parse_turn(request: &Value) -> Turn {
    let tools = request["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|t| t["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let messages = request["messages"]
        .as_array()
        .map(|msgs| msgs.iter().filter_map(parse_message).collect())
        .unwrap_or_default();

    Turn { tools, messages }
}

fn parse_message(msg: &Value) -> Option<Message> {
    let role = msg["role"].as_str()?;
    // Mid-conversation `system` messages are operator context the harness
    // injects, never something the person typed.
    if role == "system" {
        return None;
    }
    let content = &msg["content"];

    if let Some(text) = content.as_str() {
        let text = strip_reminders(text);
        if text.is_empty() {
            return None;
        }
        return Some(match role {
            "assistant" => Message::Assistant(text),
            _ => Message::User(text),
        });
    }

    let blocks = content.as_array()?;
    // A user message carrying tool results is the harness reporting back, not
    // the person typing.
    let tool_results: Vec<String> = blocks
        .iter()
        .filter(|b| b["type"] == "tool_result")
        .map(|b| block_text(&b["content"]))
        .collect();
    if !tool_results.is_empty() {
        return Some(Message::ToolResult(tool_results.join("\n\n")));
    }

    let text: Vec<String> = blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .map(strip_reminders)
        .filter(|t| !t.is_empty())
        .collect();
    if text.is_empty() {
        return None;
    }
    Some(match role {
        "assistant" => Message::Assistant(text.join("\n")),
        _ => Message::User(text.join("\n")),
    })
}

/// Claude Code splices `<system-reminder>` blocks into user messages. They're
/// harness plumbing, not the prompt, and would otherwise swamp the real text.
fn strip_reminders(text: &str) -> String {
    const OPEN: &str = "<system-reminder>";
    const CLOSE: &str = "</system-reminder>";

    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        rest = match rest[start..].find(CLOSE) {
            Some(rel) => &rest[start + rel + CLOSE.len()..],
            // Unterminated: everything after the marker is plumbing.
            None => "",
        };
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// `tool_result.content` is a string in some clients, a block array in others.
fn block_text(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    content
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn content_blocks(reply: &brain::Reply) -> Vec<Value> {
    reply
        .blocks
        .iter()
        .map(|b| match b {
            Block::Text(text) => json!({"type": "text", "text": text}),
            Block::ToolUse { name, input } => {
                json!({"type": "tool_use", "id": tool_use_id(), "name": name, "input": input})
            }
        })
        .collect()
}

fn message_object(reply: &brain::Reply, input_tokens: usize) -> Value {
    let content = content_blocks(reply);
    let output_tokens: usize = content.iter().map(|b| b.to_string().len() / 4).sum();
    json!({
        "id": message_id(),
        "type": "message",
        "role": "assistant",
        "model": MODEL,
        "content": content,
        "stop_reason": stop_reason(&reply.stop),
        "stop_sequence": null,
        "usage": {"input_tokens": input_tokens, "output_tokens": output_tokens},
    })
}

/// The SSE sequence Claude Code expects: message_start, then per-block
/// start/delta/stop, then message_delta and message_stop.
fn stream_frames(reply: &brain::Reply, input_tokens: usize) -> Vec<String> {
    let mut frames = Vec::new();
    let mut output_tokens = 0;

    frames.push(frame(
        "message_start",
        json!({
            "type": "message_start",
            "message": {
                "id": message_id(),
                "type": "message",
                "role": "assistant",
                "model": MODEL,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {"input_tokens": input_tokens, "output_tokens": 0},
            }
        }),
    ));
    frames.push(frame("ping", json!({"type": "ping"})));

    for (index, block) in reply.blocks.iter().enumerate() {
        match block {
            Block::Text(text) => {
                frames.push(frame(
                    "content_block_start",
                    json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {"type": "text", "text": ""}
                    }),
                ));
                for chunk in chunks(text, 60) {
                    output_tokens += chunk.len() / 4;
                    frames.push(frame(
                        "content_block_delta",
                        json!({
                            "type": "content_block_delta",
                            "index": index,
                            "delta": {"type": "text_delta", "text": chunk}
                        }),
                    ));
                }
            }
            Block::ToolUse { name, input } => {
                frames.push(frame(
                    "content_block_start",
                    json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {
                            "type": "tool_use",
                            "id": tool_use_id(),
                            "name": name,
                            "input": {}
                        }
                    }),
                ));
                let serialized = input.to_string();
                output_tokens += serialized.len() / 4;
                frames.push(frame(
                    "content_block_delta",
                    json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {"type": "input_json_delta", "partial_json": serialized}
                    }),
                ));
            }
        }
        frames.push(frame(
            "content_block_stop",
            json!({"type": "content_block_stop", "index": index}),
        ));
    }

    frames.push(frame(
        "message_delta",
        json!({
            "type": "message_delta",
            "delta": {"stop_reason": stop_reason(&reply.stop), "stop_sequence": null},
            "usage": {"output_tokens": output_tokens}
        }),
    ));
    frames.push(frame("message_stop", json!({"type": "message_stop"})));
    frames
}

fn frame(event: &str, data: Value) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

fn stop_reason(stop: &Stop) -> &'static str {
    match stop {
        Stop::EndTurn => "end_turn",
        Stop::ToolUse => "tool_use",
    }
}

/// Split on char boundaries, preferring to break at a space so words don't
/// visibly tear as they stream in.
fn chunks(text: &str, size: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;

    while start < chars.len() {
        let hard_end = (start + size).min(chars.len());
        let end = if hard_end == chars.len() {
            hard_end
        } else {
            chars[start..hard_end]
                .iter()
                .rposition(|c| c.is_whitespace())
                .map(|rel| start + rel + 1)
                .unwrap_or(hard_end)
        };
        out.push(chars[start..end].iter().collect());
        start = end;
    }
    out
}

fn next_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn message_id() -> String {
    format!("msg_clacker{:08}", next_id())
}

fn tool_use_id() -> String {
    format!("toolu_clacker{:08}", next_id())
}

pub fn debug_log(line: &str) {
    if std::env::var_os("CLACKER_DEBUG").is_some() {
        eprintln!("[clacker] {line}");
    }
}
