use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::{Comment, Feed, Story};
use crate::hn::article::html_to_text;

const FIREBASE: &str = "https://hacker-news.firebaseio.com/v0";
const ALGOLIA: &str = "https://hn.algolia.com/api/v1/search";

pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent("clacker/0.1 (https://github.com/)")
            .build()
            .new_agent()
    })
}

pub fn get_json(url: &str) -> Result<Value, String> {
    let body = agent()
        .get(url)
        .call()
        .map_err(|e| format!("{url}: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("{url}: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("{url}: bad JSON: {e}"))
}

/// Story ids for a feed, in HN's own ranking order.
pub fn feed_ids(feed: Feed) -> Result<Vec<u64>, String> {
    let url = format!("{FIREBASE}/{}stories.json", feed.slug());
    let v = get_json(&url)?;
    Ok(v.as_array()
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default())
}

pub fn item(id: u64) -> Result<Value, String> {
    get_json(&format!("{FIREBASE}/item/{id}.json"))
}

/// Fetch a batch of items at once. HN's API is one-item-per-request, so a
/// serial fetch of 10 stories is ~10 round trips of visible lag.
pub fn stories(ids: &[u64]) -> Vec<Story> {
    let fetched: Vec<Option<Story>> = std::thread::scope(|scope| {
        let handles: Vec<_> = ids
            .iter()
            .map(|&id| scope.spawn(move || item(id).ok().map(|v| story_from(&v))))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
    });
    fetched.into_iter().flatten().collect()
}

pub fn story(id: u64) -> Result<Story, String> {
    Ok(story_from(&item(id)?))
}

fn story_from(v: &Value) -> Story {
    Story {
        id: v["id"].as_u64().unwrap_or(0),
        title: v["title"].as_str().unwrap_or("(untitled)").to_string(),
        url: v["url"].as_str().map(str::to_string),
        by: v["by"].as_str().unwrap_or("anonymous").to_string(),
        score: v["score"].as_u64().unwrap_or(0),
        descendants: v["descendants"].as_u64().unwrap_or(0),
        age: relative_age(v["time"].as_u64().unwrap_or(0)),
        text: v["text"].as_str().map(html_to_text),
    }
}

/// Walk a story's comment tree breadth-first, capped so a 900-comment thread
/// doesn't turn into 900 HTTP requests.
pub fn comments(story_id: u64, limit: usize, max_depth: usize) -> Result<Vec<Comment>, String> {
    let root = item(story_id)?;
    let mut out = Vec::new();
    let mut level: Vec<u64> = kids_of(&root);
    let mut depth = 0;

    while depth < max_depth && !level.is_empty() && out.len() < limit {
        let take = (limit - out.len()).min(level.len());
        let batch: Vec<Value> = std::thread::scope(|scope| {
            let handles: Vec<_> = level[..take]
                .iter()
                .map(|&id| scope.spawn(move || item(id).ok()))
                .collect();
            handles.into_iter().filter_map(|h| h.join().unwrap_or(None)).collect()
        });

        let mut next = Vec::new();
        for v in batch {
            if v["deleted"].as_bool().unwrap_or(false) || v["dead"].as_bool().unwrap_or(false) {
                continue;
            }
            let Some(text) = v["text"].as_str() else { continue };
            next.extend(kids_of(&v));
            out.push(Comment {
                by: v["by"].as_str().unwrap_or("anonymous").to_string(),
                text: html_to_text(text),
                depth,
            });
        }
        level = next;
        depth += 1;
    }
    Ok(out)
}

fn kids_of(v: &Value) -> Vec<u64> {
    v["kids"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

pub fn search(query: &str, limit: usize) -> Result<Vec<Story>, String> {
    let url = format!("{ALGOLIA}?tags=story&hitsPerPage={limit}&query={}", encode(query));
    let v = get_json(&url)?;
    let hits = v["hits"].as_array().cloned().unwrap_or_default();
    Ok(hits
        .iter()
        .map(|h| Story {
            id: h["objectID"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
            title: h["title"].as_str().unwrap_or("(untitled)").to_string(),
            url: h["url"].as_str().map(str::to_string),
            by: h["author"].as_str().unwrap_or("anonymous").to_string(),
            score: h["points"].as_u64().unwrap_or(0),
            descendants: h["num_comments"].as_u64().unwrap_or(0),
            age: relative_age(h["created_at_i"].as_u64().unwrap_or(0)),
            text: None,
        })
        .collect())
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn relative_age(unix: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let secs = now.saturating_sub(unix);
    match secs {
        0..=5400 => format!("{}m", (secs / 60).max(1)),
        5401..=172_800 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}
