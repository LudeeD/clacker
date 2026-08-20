//! Deliberately crude readability: enough to make a linked page skimmable.

use super::api::agent;

pub fn fetch(url: &str) -> Result<String, String> {
    let mut resp = agent()
        .get(url)
        .header("accept", "text/html,text/plain;q=0.9,*/*;q=0.8")
        .call()
        .map_err(|e| format!("couldn't fetch {url}: {e}"))?;

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body = resp
        .body_mut()
        .with_config()
        .limit(4 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| format!("couldn't read {url}: {e}"))?;

    if content_type.contains("text/html") || body.trim_start().starts_with('<') {
        Ok(extract(&body))
    } else {
        Ok(body)
    }
}

/// Prefer paragraph text; fall back to the whole document when a page turns
/// out to be one giant div soup.
pub fn extract(html: &str) -> String {
    let cleaned = strip_elements(html, &["script", "style", "noscript", "svg", "head"]);
    let paragraphs: Vec<String> = block_texts(&cleaned, "p")
        .into_iter()
        .filter(|p| p.len() > 40)
        .collect();

    if paragraphs.len() >= 3 {
        paragraphs.join("\n\n")
    } else {
        html_to_text(&cleaned)
    }
}

/// Drop whole elements, tags and contents alike.
fn strip_elements(html: &str, tags: &[&str]) -> String {
    let mut out = html.to_string();
    for tag in tags {
        let close = format!("</{tag}>");
        let mut from = 0;
        while let Some(open) = find_tag(&out, tag, from) {
            // ASCII-lowercased so byte offsets keep matching `out`.
            let lower = out.to_ascii_lowercase();
            match lower[open.start..].find(&close) {
                Some(rel) => {
                    let end = open.start + rel + close.len();
                    out.replace_range(open.start..end, " ");
                    from = open.start;
                }
                // Unclosed: drop just the tag, never the rest of the document.
                None => {
                    out.replace_range(open.start..open.body, " ");
                    from = open.start + 1;
                }
            }
        }
    }
    out
}

struct TagMatch {
    /// Byte offset of the `<`.
    start: usize,
    /// Byte offset just past the `>`, i.e. where the element's body begins.
    body: usize,
}

/// Find `<tag …>`, requiring a real tag-name boundary so `<head>` doesn't
/// match `<header>` and `<p>` doesn't match `<pre>`.
fn find_tag(html: &str, tag: &str, from: usize) -> Option<TagMatch> {
    let lower = html.to_ascii_lowercase();
    let needle = format!("<{tag}");
    let mut cursor = from;

    while let Some(rel) = lower[cursor..].find(&needle) {
        let start = cursor + rel;
        let after = start + needle.len();
        let boundary = lower[after..]
            .chars()
            .next()
            .is_none_or(|c| c == '>' || c == '/' || c.is_whitespace());
        if boundary {
            if let Some(gt) = lower[start..].find('>') {
                return Some(TagMatch { start, body: start + gt + 1 });
            }
        }
        cursor = start + 1;
    }
    None
}

/// Text content of every `<tag>…</tag>` block, in document order.
fn block_texts(html: &str, tag: &str) -> Vec<String> {
    let close = format!("</{tag}>");
    let mut out = Vec::new();
    let mut cursor = 0;

    while let Some(open) = find_tag(html, tag, cursor) {
        let lower = html.to_ascii_lowercase();
        let body_end = match lower[open.body..].find(&close) {
            Some(rel) => open.body + rel,
            None => html.len(),
        };
        let text = html_to_text(&html[open.body..body_end]);
        if !text.is_empty() {
            out.push(text);
        }
        cursor = body_end.max(open.body);
    }
    out
}

/// Strip tags, decode the entities that actually show up, collapse whitespace.
/// HN comment bodies come through here too — they arrive as HTML fragments
/// where `<p>` carries the only structure worth keeping.
pub fn html_to_text(html: &str) -> String {
    collapse(&decode_entities(&strip_tags(html)))
}

/// Replace each tag with the whitespace it stands for: block elements become
/// paragraph breaks, inline elements become nothing (so `</i>.` doesn't turn
/// into `" ."`), anything unrecognized becomes a single space.
fn strip_tags(html: &str) -> String {
    const BLOCK: [&str; 12] = [
        "p", "div", "li", "tr", "h1", "h2", "h3", "h4", "h5", "blockquote", "pre", "section",
    ];
    const INLINE: [&str; 14] = [
        "a", "b", "i", "em", "strong", "code", "span", "u", "s", "sup", "sub", "small", "abbr",
        "mark",
    ];

    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0;

    while let Some(rel) = lower[cursor..].find('<') {
        let start = cursor + rel;
        let Some(gt) = lower[start..].find('>') else { break };
        let end = start + gt + 1;

        out.push_str(&html[cursor..start]);
        let name = lower[start + 1..end - 1]
            .trim_start_matches('/')
            .split(|c: char| c == ' ' || c == '/' || c.is_whitespace())
            .next()
            .unwrap_or("");

        if BLOCK.contains(&name) {
            out.push_str("\n\n");
        } else if name == "br" {
            out.push('\n');
        } else if !INLINE.contains(&name) {
            out.push(' ');
        }
        cursor = end;
    }
    out.push_str(&html[cursor..]);
    out
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;

    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let replacement = match entity {
            "amp" => "&".to_string(),
            "lt" => "<".to_string(),
            "gt" => ">".to_string(),
            "quot" => "\"".to_string(),
            "apos" | "#39" | "#x27" => "'".to_string(),
            "nbsp" => " ".to_string(),
            "mdash" => "—".to_string(),
            "ndash" => "–".to_string(),
            "hellip" => "…".to_string(),
            e if e.starts_with("#x") || e.starts_with("#X") => {
                u32::from_str_radix(&e[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
                    .map(String::from)
                    .unwrap_or_else(|| format!("&{e};"))
            }
            e if e.starts_with('#') => e[1..]
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .map(String::from)
                .unwrap_or_else(|| format!("&{e};")),
            e => format!("&{e};"),
        };
        out.push_str(&replacement);
        rest = &rest[semi + 1..];
    }
    out.push_str(rest);
    out
}

fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0;

    for line in s.lines() {
        let trimmed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run == 1 && !out.is_empty() {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(&trimmed);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// Cap a long article so one story can't blow out the harness's context.
pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars).collect();
    let end = cut.rfind("\n\n").or_else(|| cut.rfind(' ')).unwrap_or(cut.len());
    format!("{}\n\n… (truncated)", &cut[..end].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_the_named_element() {
        // `<head>` must not match `<header>` — and an unclosed match must not
        // swallow the rest of the document.
        let html = "<head><title>x</title></head><header>Nav</header><p>Body text</p>";
        let cleaned = strip_elements(html, &["head"]);
        assert!(cleaned.contains("Body text"), "got: {cleaned}");
        assert!(cleaned.contains("Nav"), "got: {cleaned}");
        assert!(!cleaned.contains("<title>"), "got: {cleaned}");
    }

    #[test]
    fn paragraph_blocks_ignore_similarly_named_tags() {
        let html = "<pre>code</pre><p>first</p><p>second</p>";
        assert_eq!(block_texts(html, "p"), vec!["first", "second"]);
    }

    #[test]
    fn tags_become_text_with_paragraph_breaks() {
        let html = "<p>One line.</p><p>Two <i>lines</i>.<br>Same para.</p>";
        assert_eq!(html_to_text(html), "One line.\n\nTwo lines.\nSame para.");
    }

    #[test]
    fn decodes_the_entities_hn_actually_sends() {
        assert_eq!(
            html_to_text("a &amp; b &gt; c &#39;d&#39; &mdash; &#x41;"),
            "a & b > c 'd' — A"
        );
    }

    #[test]
    fn falls_back_to_full_text_when_there_are_few_paragraphs() {
        let html = "<html><body><div>Only a div, no paragraphs at all here.</div></body></html>";
        assert_eq!(extract(html), "Only a div, no paragraphs at all here.");
    }

    #[test]
    fn truncation_lands_on_a_boundary() {
        let text = "word ".repeat(100);
        let cut = truncate(&text, 50);
        assert!(cut.ends_with("… (truncated)"));
        assert!(cut.len() < text.len());
    }
}

