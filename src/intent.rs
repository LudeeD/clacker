//! What the user typed -> what to do about it. This is the whole "model".

use crate::hn::Feed;

pub enum Action {
    /// A feed listing. `offset` is how far down the ranking to start.
    List { feed: Feed, offset: usize },
    /// The next page of whatever listing is on screen.
    More,
    /// Read story number N as displayed; `None` means the current story.
    Read { index: Option<usize> },
    /// Comments on story number N as displayed; `None` means the current story.
    Comments { index: Option<usize> },
    Search { query: String },
    Help,
}

pub fn route(prompt: &str) -> Action {
    let text = prompt.trim().to_lowercase();
    let words: Vec<&str> = text.split_whitespace().collect();

    if words.is_empty() {
        return Action::List { feed: Feed::Top, offset: 0 };
    }

    if text.contains("help") || text.contains("what can you") {
        return Action::Help;
    }

    if matches!(words[0], "more" | "next") {
        return Action::More;
    }

    if let Some(rest) = strip_prefix_word(&text, &["search", "find", "look"]) {
        let query = rest.trim_start_matches("for ").trim();
        if !query.is_empty() {
            return Action::Search { query: query.to_string() };
        }
    }

    if text.contains("comment") || text.contains("discussion") || text.contains("thread") {
        return Action::Comments { index: first_number(&words).or_else(|| ordinal(&words)) };
    }

    // A bare number, or "open 3" / "read the third one".
    if let Some(n) = first_number(&words).filter(|_| words.len() <= 6) {
        return Action::Read { index: Some(n) };
    }
    if let Some(n) = ordinal(&words) {
        return Action::Read { index: Some(n) };
    }
    if matches!(words[0], "open" | "read" | "article") {
        return Action::Read { index: None };
    }

    for (needles, feed) in [
        (["ask hn", "ask"].as_slice(), Feed::Ask),
        (["show hn", "show"].as_slice(), Feed::Show),
        (["job", "jobs", "hiring", "who's hiring"].as_slice(), Feed::Job),
        (["newest", "new"].as_slice(), Feed::New),
        (["best"].as_slice(), Feed::Best),
        (["top", "front page", "frontpage", "news", "hacker news"].as_slice(), Feed::Top),
    ] {
        if needles.iter().any(|n| contains_word(&text, n)) {
            return Action::List { feed, offset: 0 };
        }
    }

    // Anything else that reads like a question becomes a search.
    if words.len() > 2 {
        return Action::Search { query: prompt.trim().to_string() };
    }

    Action::List { feed: Feed::Top, offset: 0 }
}

fn strip_prefix_word<'a>(text: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes
        .iter()
        .find_map(|p| text.strip_prefix(&format!("{p} ")))
}

/// Substring match that respects word boundaries, so "new" doesn't fire on
/// "renewable" and "ask" doesn't fire on "basketball".
fn contains_word(text: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = text[from..].find(needle) {
        let start = from + rel;
        let end = start + needle.len();
        let before_ok = start == 0 || !text[..start].ends_with(|c: char| c.is_alphanumeric());
        let after_ok = end == text.len() || !text[end..].starts_with(|c: char| c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn first_number(words: &[&str]) -> Option<usize> {
    words
        .iter()
        .find_map(|w| w.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok())
        .filter(|&n: &usize| n > 0)
}

fn ordinal(words: &[&str]) -> Option<usize> {
    words.iter().find_map(|word| {
        Some(match *word {
            "first" => 1,
            "second" => 2,
            "third" => 3,
            "fourth" => 4,
            "fifth" => 5,
            "sixth" => 6,
            "seventh" => 7,
            "eighth" => 8,
            "ninth" => 9,
            "tenth" => 10,
            _ => return None,
        })
    })
}
