//! The "model". A pure function from transcript to reply — no LLM, no state.
//!
//! Protocol adapters normalize their wire format into [`Turn`] and render
//! [`Reply`] back out, which is what keeps this file harness-agnostic.

use serde_json::{Value, json};

use crate::hn::render;
use crate::intent::{self, Action};

pub enum Message {
    User(String),
    Assistant(String),
    ToolResult(String),
}

pub struct Turn {
    /// Tool names exactly as the harness advertised them. We match by suffix
    /// rather than hardcoding `mcp__hn__*`, so a harness that namespaces its
    /// tools differently still works.
    pub tools: Vec<String>,
    pub messages: Vec<Message>,
}

pub enum Block {
    Text(String),
    ToolUse { name: String, input: Value },
}

pub enum Stop {
    EndTurn,
    ToolUse,
}

pub struct Reply {
    pub blocks: Vec<Block>,
    pub stop: Stop,
}

impl Reply {
    fn text(body: impl Into<String>) -> Reply {
        Reply { blocks: vec![Block::Text(body.into())], stop: Stop::EndTurn }
    }
}

const HINT: &str = "_Say a number to read a story, `comments 2` for a thread, `more` for the next page, or search for anything._";

const HELP: &str = "\
I browse Hacker News. Things you can say:

- **front page**, or `new` / `best` / `ask` / `show` / `jobs`
- **3** — read story 3 from the current list
- **comments 3** — open that story's thread
- **more** — next page
- **search rust async** — search HN

_No model is involved. Every answer is Hacker News._";

pub fn respond(turn: &Turn) -> Reply {
    // Harness housekeeping calls (session titles, summaries) arrive without a
    // tool list. Answer them briefly so they stay out of the way.
    if turn.tools.is_empty() {
        return Reply::text("Browsing Hacker News");
    }

    match turn.messages.last() {
        Some(Message::ToolResult(text)) => present(text),
        Some(Message::User(prompt)) => act(turn, prompt),
        _ => Reply::text(HELP),
    }
}

/// Turn A: the user asked for something, so call a tool.
fn act(turn: &Turn, prompt: &str) -> Reply {
    let action = intent::route(prompt);
    let list = latest_list(turn);
    let story = latest_story(turn);

    let (preamble, tool, input) = match action {
        Action::Help => return Reply::text(HELP),

        Action::List { feed, offset } => (
            format!("Let me pull up {}.", feed.label()),
            "front_page",
            json!({"feed": feed.slug(), "offset": offset}),
        ),

        Action::More => {
            let (feed, offset) = match &list {
                Some(l) => (l.feed.clone(), l.offset + l.ids.len()),
                None => ("top".to_string(), 0),
            };
            (
                "Next page.".to_string(),
                "front_page",
                json!({"feed": feed, "offset": offset}),
            )
        }

        Action::Read { index } => match resolve(index, &list, story) {
            Some(id) => (
                "Let me read that one.".to_string(),
                "read_story",
                json!({"id": id}),
            ),
            None => return Reply::text(no_listing_yet()),
        },

        Action::Comments { index } => match resolve(index, &list, story) {
            Some(id) => (
                "Pulling up the discussion.".to_string(),
                "comments",
                json!({"id": id}),
            ),
            None => return Reply::text(no_listing_yet()),
        },

        Action::Search { query } => (
            format!("Searching Hacker News for \"{query}\"."),
            "search",
            json!({"query": query}),
        ),
    };

    let Some(name) = tool_named(turn, tool) else {
        return Reply::text(format!(
            "I can't reach Hacker News — the `{tool}` tool isn't available in this session."
        ));
    };

    Reply {
        blocks: vec![Block::Text(preamble), Block::ToolUse { name, input }],
        stop: Stop::ToolUse,
    }
}

/// Turn B: the tool result came back, so present it.
fn present(result: &str) -> Reply {
    let shown = render::strip_markers(result);
    if render::parse_list(result).is_some() {
        return Reply::text(format!("{shown}\n\n{HINT}"));
    }
    Reply::text(shown)
}

/// A displayed number maps through the current listing's offset; with no
/// number we fall back to whichever story is already open.
fn resolve(index: Option<usize>, list: &Option<render::ListMarker>, story: Option<u64>) -> Option<u64> {
    match index {
        None => story.or_else(|| list.as_ref()?.ids.first().copied()),
        Some(n) => {
            let list = list.as_ref()?;
            let slot = n.checked_sub(list.offset + 1)?;
            list.ids.get(slot).copied()
        }
    }
}

fn no_listing_yet() -> String {
    format!("I don't have a story list open yet — say **front page** first.\n\n{HELP}")
}

/// Most recent listing anywhere in the transcript.
fn latest_list(turn: &Turn) -> Option<render::ListMarker> {
    transcript_text(turn).iter().rev().find_map(|t| render::parse_list(t))
}

/// Most recent single story, but only if it's newer than the last listing —
/// otherwise "comments" after a fresh list should mean story 1, not the story
/// you read five turns ago.
fn latest_story(turn: &Turn) -> Option<u64> {
    let texts = transcript_text(turn);
    let last_story = texts.iter().rposition(|t| render::parse_story(t).is_some())?;
    let last_list = texts.iter().rposition(|t| render::parse_list(t).is_some());
    if last_list.is_some_and(|i| i > last_story) {
        return None;
    }
    render::parse_story(&texts[last_story])
}

fn transcript_text(turn: &Turn) -> Vec<String> {
    turn.messages
        .iter()
        .map(|m| match m {
            Message::User(t) | Message::Assistant(t) | Message::ToolResult(t) => t.clone(),
        })
        .collect()
}

/// Match `front_page` against whatever the harness called it — Claude Code
/// advertises it as `mcp__hn__front_page`.
fn tool_named(turn: &Turn, suffix: &str) -> Option<String> {
    turn.tools
        .iter()
        .find(|name| name.as_str() == suffix || name.ends_with(&format!("__{suffix}")))
        .cloned()
}
