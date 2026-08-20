//! Stdio MCP server (`clacker mcp`). The harness spawns this itself, so the
//! tool rows you see in the transcript are genuinely dispatched by the harness.
//!
//! All HN network access lives in this process — the fake model never fetches
//! anything, it only reads back what the harness hands it in tool results.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::hn::{Feed, api, article, render};

const DEFAULT_LIMIT: usize = 10;
const MAX_ARTICLE_CHARS: usize = 12_000;
const COMMENT_LIMIT: usize = 25;
const COMMENT_DEPTH: usize = 3;

pub fn run() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // Notifications carry no id and expect no reply.
        let Some(id) = req.get("id").cloned() else {
            continue;
        };

        let method = req["method"].as_str().unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(json!({}));
        let response = match dispatch(method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(message) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": message}})
            }
        };

        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

fn dispatch(method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "initialize" => Ok(json!({
            // Echo the client's version back rather than pinning one.
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-03-26"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "hn", "version": env!("CARGO_PKG_VERSION")},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call(name, &args) {
                Ok(text) => Ok(json!({"content": [{"type": "text", "text": text}]})),
                // A failed fetch is a tool error, not a protocol error: the
                // harness should render it and let the user try something else.
                Err(e) => Ok(json!({
                    "content": [{"type": "text", "text": e}],
                    "isError": true,
                })),
            }
        }
        other => Err(format!("unknown method: {other}")),
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "front_page",
            "description": "List stories from a Hacker News feed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "feed": {
                        "type": "string",
                        "enum": ["top", "new", "best", "ask", "show", "job"],
                        "description": "Which feed to list. Defaults to top."
                    },
                    "offset": {"type": "integer", "description": "Rank to start from."},
                    "limit": {"type": "integer", "description": "How many stories."}
                }
            }
        },
        {
            "name": "read_story",
            "description": "Fetch the article a story links to, as readable text.",
            "inputSchema": {
                "type": "object",
                "properties": {"id": {"type": "integer", "description": "HN story id."}},
                "required": ["id"]
            }
        },
        {
            "name": "comments",
            "description": "Fetch the comment thread for a story.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "integer", "description": "HN story id."},
                    "limit": {"type": "integer", "description": "Max comments."}
                },
                "required": ["id"]
            }
        },
        {
            "name": "search",
            "description": "Search Hacker News stories by keyword.",
            "inputSchema": {
                "type": "object",
                "properties": {"query": {"type": "string"}},
                "required": ["query"]
            }
        }
    ])
}

fn call(name: &str, args: &Value) -> Result<String, String> {
    match name {
        "front_page" => {
            let feed = args["feed"]
                .as_str()
                .and_then(Feed::parse)
                .unwrap_or(Feed::Top);
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let limit = args["limit"].as_u64().unwrap_or(DEFAULT_LIMIT as u64) as usize;
            let limit = limit.clamp(1, 30);

            let ids = api::feed_ids(feed)?;
            let page: Vec<u64> = ids.into_iter().skip(offset).take(limit).collect();
            if page.is_empty() {
                return Err("That's the end of the feed.".into());
            }
            let stories = api::stories(&page);
            Ok(render::listing(feed.slug(), feed.label(), &stories, offset))
        }
        "read_story" => {
            let id = args["id"].as_u64().ok_or("read_story needs an id")?;
            let story = api::story(id)?;
            let body = match (&story.text, &story.url) {
                (Some(text), _) if !text.is_empty() => text.clone(),
                (_, Some(url)) => article::truncate(&article::fetch(url)?, MAX_ARTICLE_CHARS),
                _ => "_This story has no linked article._".to_string(),
            };
            Ok(render::article(&story, &body))
        }
        "comments" => {
            let id = args["id"].as_u64().ok_or("comments needs an id")?;
            let limit = args["limit"].as_u64().unwrap_or(COMMENT_LIMIT as u64) as usize;
            let story = api::story(id)?;
            let thread = api::comments(id, limit.clamp(1, 60), COMMENT_DEPTH)?;
            Ok(render::thread(&story, &thread))
        }
        "search" => {
            let query = args["query"].as_str().ok_or("search needs a query")?;
            let stories = api::search(query, DEFAULT_LIMIT)?;
            if stories.is_empty() {
                return Err(format!("Nothing on HN matches \"{query}\"."));
            }
            Ok(render::listing("search", &format!("Search: {query}"), &stories, 0))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}
