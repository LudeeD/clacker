//! Hacker News core. Harness-agnostic: nothing in here knows what a harness is.

pub mod api;
pub mod article;
pub mod render;

#[derive(Clone, Copy, PartialEq)]
pub enum Feed {
    Top,
    New,
    Best,
    Ask,
    Show,
    Job,
}

impl Feed {
    pub fn parse(s: &str) -> Option<Feed> {
        Some(match s {
            "top" | "front" | "frontpage" => Feed::Top,
            "new" => Feed::New,
            "best" => Feed::Best,
            "ask" => Feed::Ask,
            "show" => Feed::Show,
            "job" | "jobs" => Feed::Job,
            _ => return None,
        })
    }

    /// The path segment the Firebase API uses: `topstories.json`, etc.
    pub fn slug(self) -> &'static str {
        match self {
            Feed::Top => "top",
            Feed::New => "new",
            Feed::Best => "best",
            Feed::Ask => "ask",
            Feed::Show => "show",
            Feed::Job => "job",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Feed::Top => "the front page",
            Feed::New => "new stories",
            Feed::Best => "the best stories",
            Feed::Ask => "Ask HN",
            Feed::Show => "Show HN",
            Feed::Job => "the job board",
        }
    }
}

#[derive(Default, Clone)]
pub struct Story {
    pub id: u64,
    pub title: String,
    pub url: Option<String>,
    pub by: String,
    pub score: u64,
    pub descendants: u64,
    pub age: String,
    /// Self-post body (Ask HN and friends), already converted to plain text.
    pub text: Option<String>,
}

pub struct Comment {
    pub by: String,
    pub text: String,
    pub depth: usize,
}
