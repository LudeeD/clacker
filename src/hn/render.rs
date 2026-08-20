//! Story/comment -> markdown. Everything the harness ends up displaying is
//! built here.

use super::{Comment, Story};

/// Hidden markers appended to rendered output. The brain reads them back out
/// of the transcript to resolve "3", "comments", or "more" — which is why
/// clacker needs no server-side session state at all. Markdown comments don't
/// render, so the user never sees them.
const LIST_MARKER: &str = "<!-- clacker:list ";
const STORY_MARKER: &str = "<!-- clacker:story ";

pub struct ListMarker {
    pub feed: String,
    pub offset: usize,
    pub ids: Vec<u64>,
}

fn list_marker(feed: &str, offset: usize, ids: &[u64]) -> String {
    let joined: Vec<String> = ids.iter().map(u64::to_string).collect();
    format!("{LIST_MARKER}feed={feed} offset={offset} ids={} -->", joined.join(","))
}

fn story_marker(id: u64) -> String {
    format!("{STORY_MARKER}id={id} -->")
}

/// Parse the last listing marker in a block of text.
pub fn parse_list(text: &str) -> Option<ListMarker> {
    let body = last_marker_body(text, LIST_MARKER)?;
    let mut marker = ListMarker { feed: "top".into(), offset: 0, ids: Vec::new() };
    for field in body.split_whitespace() {
        let (key, value) = field.split_once('=')?;
        match key {
            "feed" => marker.feed = value.to_string(),
            "offset" => marker.offset = value.parse().ok()?,
            "ids" => marker.ids = value.split(',').filter_map(|s| s.parse().ok()).collect(),
            _ => {}
        }
    }
    (!marker.ids.is_empty()).then_some(marker)
}

/// Parse the last single-story marker in a block of text.
pub fn parse_story(text: &str) -> Option<u64> {
    let body = last_marker_body(text, STORY_MARKER)?;
    body.strip_prefix("id=")?.trim().parse().ok()
}

/// Remove every marker from text that will be displayed. Markers still live
/// in the tool results the harness keeps in the transcript, which is where the
/// brain reads them from — the assistant message doesn't need to repeat them.
pub fn strip_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("<!-- clacker:") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(rel) => rest = &rest[start + rel + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim_end().to_string()
}

fn last_marker_body<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let start = text.rfind(prefix)? + prefix.len();
    let rest = &text[start..];
    Some(rest[..rest.find("-->")?].trim())
}

pub fn listing(feed: &str, heading: &str, stories: &[Story], start_index: usize) -> String {
    let mut out = format!("**{heading}**\n\n");
    for (i, s) in stories.iter().enumerate() {
        let n = start_index + i + 1;
        out.push_str(&format!("{n}. **{}**\n", s.title));
        out.push_str(&format!(
            "   {} points · {} comments · by {} · {} ago{}\n",
            s.score,
            s.descendants,
            s.by,
            s.age,
            s.url
                .as_deref()
                .and_then(domain)
                .map(|d| format!(" · {d}"))
                .unwrap_or_default()
        ));
    }
    let ids: Vec<u64> = stories.iter().map(|s| s.id).collect();
    out.push('\n');
    out.push_str(&list_marker(feed, start_index, &ids));
    out
}

pub fn article(story: &Story, body: &str) -> String {
    let mut out = format!("**{}**\n", story.title);
    out.push_str(&format!(
        "{} points · {} comments · by {} · {} ago\n",
        story.score, story.descendants, story.by, story.age
    ));
    if let Some(url) = &story.url {
        out.push_str(&format!("{url}\n"));
    }
    out.push_str("\n---\n\n");
    out.push_str(body);
    out.push_str(&format!("\n\n{}", story_marker(story.id)));
    out
}

pub fn thread(story: &Story, comments: &[Comment]) -> String {
    let mut out = format!("**Comments on: {}**\n", story.title);
    out.push_str(&format!(
        "{} points · {} comments · by {}\n\n",
        story.score, story.descendants, story.by
    ));

    if comments.is_empty() {
        out.push_str("_No comments yet._\n");
    }
    for c in comments {
        let indent = "  ".repeat(c.depth);
        out.push_str(&format!("{indent}**{}**\n", c.by));
        for line in c.text.lines() {
            out.push_str(&format!("{indent}{line}\n"));
        }
        out.push('\n');
    }
    out.push_str(&story_marker(story.id));
    out
}

fn domain(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split('/').next()?;
    Some(host.trim_start_matches("www.").to_string())
}
