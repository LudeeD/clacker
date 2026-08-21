//! Readability extraction: pull the article out of a page and render it as
//! markdown, which is what the harness displays.

use dom_smoothie::Readability;

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
        Ok(extract(&body, url))
    } else {
        Ok(body)
    }
}

/// Readability picks the article out of the page; htmd renders it as markdown.
/// When there's no article to find — a directory page, a paywall stub — we
/// fall back to flattening the whole document.
pub fn extract(html: &str, url: &str) -> String {
    readable(html, url).unwrap_or_else(|| html_to_text(html))
}

fn readable(html: &str, url: &str) -> Option<String> {
    // `url` resolves relative links; a bad one is just a failed extraction.
    let mut readability = Readability::new(html, Some(url), None).ok()?;
    let article = readability.parse().ok()?;
    let markdown = htmd::convert(&article.content).ok()?;
    (!markdown.trim().is_empty()).then_some(markdown)
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

    const PAGE: &str = "<html><head><title>T</title></head><body>\
        <nav><a href='/'>Home</a> <a href='/about'>About</a></nav>\
        <article><h2>A heading</h2>\
        <p>The first paragraph of the article, long enough to be scored as real content by readability.</p>\
        <p>A second paragraph, also long enough that the extractor treats this element as the article body.</p>\
        <ul><li>a bullet</li></ul></article>\
        <footer>Copyright nobody</footer></body></html>";

    #[test]
    fn extraction_drops_boilerplate() {
        let out = extract(PAGE, "https://example.com/post");
        assert!(out.contains("The first paragraph"), "got: {out}");
        assert!(!out.contains("Copyright nobody"), "got: {out}");
        assert!(!out.contains("About"), "got: {out}");
    }

    #[test]
    fn extraction_keeps_structure_as_markdown() {
        let out = extract(PAGE, "https://example.com/post");
        assert!(out.contains("## A heading"), "got: {out}");
        assert!(out.contains("*   a bullet"), "got: {out}");
    }

    #[test]
    fn falls_back_to_flat_text_when_there_is_no_article() {
        let html = "<html><body><div>Nothing here.</div></body></html>";
        assert_eq!(extract(html, "https://example.com/"), "Nothing here.");
    }

    #[test]
    fn truncation_lands_on_a_boundary() {
        let text = "word ".repeat(100);
        let cut = truncate(&text, 50);
        assert!(cut.ends_with("… (truncated)"));
        assert!(cut.len() < text.len());
    }
}


