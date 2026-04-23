/// Domain → category classifier (T-0a-003).
/// Deterministic: same domain always produces same category.
/// Static table, no network, no LLM, no state between calls.

pub struct Classified {
    pub domain: String,
    pub category: String,
}

/// Extract domain and assign category from a URL.
/// Returns `domain = "unknown"` and `category = "other"` if the URL is malformed.
pub fn classify(url: &str) -> Classified {
    let domain = extract_domain(url);
    let category = lookup_category(&domain).to_string();
    Classified { domain, category }
}

fn extract_domain(url: &str) -> String {
    let s = if let Some(p) = url.find("://") { &url[p + 3..] } else { url };
    let host = s.split(|c| matches!(c, '/' | '?' | '#')).next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        "unknown".to_string()
    } else {
        host.to_lowercase()
    }
}

/// Two-pass lookup: exact domain first, then strip one subdomain level, then two.
fn lookup_category(domain: &str) -> &'static str {
    if let Some(cat) = exact_lookup(domain) {
        return cat;
    }
    if let Some(root) = domain.find('.').map(|i| &domain[i + 1..]) {
        if let Some(cat) = exact_lookup(root) {
            return cat;
        }
        if let Some(root2) = root.find('.').map(|i| &root[i + 1..]) {
            if let Some(cat) = exact_lookup(root2) {
                return cat;
            }
        }
    }
    "other"
}

fn exact_lookup(d: &str) -> Option<&'static str> {
    Some(match d {
        // development
        "github.com" | "gitlab.com" | "bitbucket.org" | "stackoverflow.com"
        | "stackexchange.com" | "npmjs.com" | "crates.io" | "pypi.org"
        | "docs.rs" | "pkg.go.dev" | "codepen.io" | "replit.com"
        | "jsfiddle.net" | "codesandbox.io" | "leetcode.com"
        | "hackerrank.com" | "codewars.com" | "rust-lang.org"
        | "golang.org" | "python.org" | "developer.apple.com"
        | "developer.android.com" | "developer.mozilla.org"
        | "hub.docker.com" | "registry.hub.docker.com" => "development",

        // notes
        "notion.so" | "notionhq.com" | "obsidian.md" | "roamresearch.com"
        | "craft.do" | "evernote.com" | "onenote.com" | "bear.app"
        | "logseq.com" | "remnote.com" | "workflowy.com" => "notes",

        // design
        "figma.com" | "dribbble.com" | "behance.net" | "sketch.com"
        | "invisionapp.com" | "zeplin.io" | "canva.com" | "adobe.com"
        | "framer.com" | "webflow.com" | "storybook.js.org"
        | "coolors.co" | "fontawesome.com" | "fonts.google.com" => "design",

        // video
        "youtube.com" | "youtu.be" | "vimeo.com" | "twitch.tv"
        | "netflix.com" | "dailymotion.com" | "wistia.com" | "loom.com"
        | "screencast.com" => "video",

        // productivity — note: google.com root catches all unmatched google.* subdomains
        "google.com" | "airtable.com" | "trello.com" | "asana.com"
        | "monday.com" | "linear.app" | "atlassian.com" | "slack.com"
        | "discord.com" | "zoom.us" | "microsoft.com" | "office.com"
        | "outlook.com" | "clickup.com" | "basecamp.com"
        | "todoist.com" | "ticktick.com" => "productivity",

        // articles
        "medium.com" | "substack.com" | "dev.to" | "hashnode.com"
        | "hackernoon.com" | "techcrunch.com" | "theverge.com"
        | "wired.com" | "news.ycombinator.com" | "lobste.rs"
        | "indiehackers.com" | "smashingmagazine.com" | "css-tricks.com"
        | "alistapart.com" | "increment.com" => "articles",

        // social
        "twitter.com" | "x.com" | "linkedin.com" | "reddit.com"
        | "facebook.com" | "instagram.com" | "pinterest.com"
        | "mastodon.social" | "threads.net" | "bsky.app" => "social",

        // commerce
        "amazon.com" | "gumroad.com" | "stripe.com" | "shopify.com"
        | "etsy.com" | "ebay.com" | "paypal.com" | "paddle.com"
        | "lemonsqueezy.com" | "revenuecat.com" | "fastspring.com" => "commerce",

        // research — note: scholar.google.com is listed here before google.com
        // so the exact match wins over the google.com → productivity fallback
        "arxiv.org" | "scholar.google.com" | "pubmed.ncbi.nlm.nih.gov"
        | "semanticscholar.org" | "researchgate.net" | "jstor.org"
        | "ncbi.nlm.nih.gov" | "nature.com" | "science.org"
        | "acm.org" | "ieee.org" | "springer.com" | "wiley.com"
        | "sciencedirect.com" | "plos.org" => "research",

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://github.com/user/repo"), "github.com");
        assert_eq!(extract_domain("https://www.notion.so/page"), "notion.so");
        assert_eq!(extract_domain("https://mail.google.com/mail/u/0/"), "mail.google.com");
        assert_eq!(extract_domain("https://scholar.google.com/scholar?q=rust"), "scholar.google.com");
        assert_eq!(extract_domain("not-a-url"), "not-a-url");
    }

    #[test]
    fn test_classify() {
        assert_eq!(classify("https://github.com/foo/bar").category, "development");
        assert_eq!(classify("https://notion.so/page").category, "notes");
        assert_eq!(classify("https://scholar.google.com/").category, "research");
        assert_eq!(classify("https://mail.google.com/").category, "productivity");
        assert_eq!(classify("https://unknown-domain.xyz/").category, "other");
    }
}
