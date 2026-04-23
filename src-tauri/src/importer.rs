/// Bookmark Importer — bootstrap data for Phase 0a demo (T-0a-002).
/// Reads Chrome JSON bookmarks or Netscape HTML bookmark exports from the
/// local filesystem. No network, no observer, one discrete pass per call.
/// Delegates classification to the Classifier (T-0a-003).

use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::{classifier, crypto, storage::{Db, NewResource}};

#[derive(Debug, Default, serde::Serialize)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
    pub sources: Vec<String>,
}

/// Import from a caller-supplied path, or auto-detect browser bookmark files.
pub fn import(path: Option<&str>, db: &Db, key: &str) -> ImportResult {
    match path {
        Some(p) => {
            let pb = PathBuf::from(p);
            match import_from_path(&pb, db, key) {
                Ok(mut r) => {
                    r.sources.push(p.to_string());
                    r
                }
                Err(e) => ImportResult {
                    errors: vec![e],
                    ..Default::default()
                },
            }
        }
        None => import_auto(db, key),
    }
}

fn import_auto(db: &Db, key: &str) -> ImportResult {
    let mut result = ImportResult::default();
    let sources = detect_sources();

    if sources.is_empty() {
        result.errors.push(
            "No browser bookmark files found automatically. \
             Export bookmarks to HTML and pass the file path."
                .into(),
        );
        return result;
    }

    for source in &sources {
        match import_from_path(source, db, key) {
            Ok(r) => {
                result.imported += r.imported;
                result.skipped += r.skipped;
                result.errors.extend(r.errors);
                result.sources.push(source.to_string_lossy().into_owned());
            }
            Err(e) => result
                .errors
                .push(format!("{}: {e}", source.display())),
        }
    }
    result
}

pub fn import_from_path(path: &Path, db: &Db, key: &str) -> Result<ImportResult, String> {
    if !path.exists() {
        return Err("file not found".into());
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if name == "Bookmarks" {
        import_chrome_json(path, db, key)
    } else if ext == "html" || ext == "htm" {
        import_html(path, db, key)
    } else {
        Err(format!("unsupported format: expected Chrome 'Bookmarks' JSON or .html export"))
    }
}

/// Detect Chrome / Edge / Brave bookmark files on this machine.
fn detect_sources() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Some(local) = dirs_next::data_local_dir() else {
        return found;
    };

    let candidates = [
        "Google/Chrome/User Data/Default/Bookmarks",
        "Microsoft/Edge/User Data/Default/Bookmarks",
        "BraveSoftware/Brave-Browser/User Data/Default/Bookmarks",
    ];
    for rel in &candidates {
        let p = local.join(rel);
        if p.exists() {
            found.push(p);
        }
    }
    found
}

// ── Chrome JSON ───────────────────────────────────────────────────────────────

fn import_chrome_json(path: &Path, db: &Db, key: &str) -> Result<ImportResult, String> {
    let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;

    let mut result = ImportResult::default();
    if let Some(roots) = json.get("roots").and_then(|r| r.as_object()) {
        for (_, node) in roots {
            collect_chrome_nodes(node, db, key, &mut result);
        }
    }
    Ok(result)
}

fn collect_chrome_nodes(node: &serde_json::Value, db: &Db, key: &str, r: &mut ImportResult) {
    match node.get("type").and_then(|t| t.as_str()) {
        Some("url") => {
            let url = node.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let title = node.get("name").and_then(|n| n.as_str()).unwrap_or("");
            // date_added: microseconds since Windows FILETIME epoch (1601-01-01)
            let captured_at = node
                .get("date_added")
                .and_then(|d| d.as_str())
                .map(chrome_date_to_unix)
                .unwrap_or(0);
            if url.starts_with("http://") || url.starts_with("https://") {
                insert_bookmark(url, title, captured_at, db, key, r);
            }
        }
        Some("folder") => {
            if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
                for child in children {
                    collect_chrome_nodes(child, db, key, r);
                }
            }
        }
        _ => {}
    }
}

/// Convert Chrome's date_added (microseconds since 1601-01-01) to Unix seconds.
fn chrome_date_to_unix(date_added: &str) -> i64 {
    // Difference between Windows FILETIME epoch and Unix epoch in microseconds
    const DELTA_US: i64 = 11_644_473_600_000_000;
    date_added
        .parse::<i64>()
        .map(|us| (us - DELTA_US) / 1_000_000)
        .unwrap_or(0)
        .max(0)
}

// ── Netscape HTML export ──────────────────────────────────────────────────────

fn import_html(path: &Path, db: &Db, key: &str) -> Result<ImportResult, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(import_html_content(&content, db, key))
}

/// Parse Netscape HTML bookmark content passed directly as a string.
/// Used when the frontend reads the file and sends the content via IPC.
pub fn import_html_content(content: &str, db: &Db, key: &str) -> ImportResult {
    let lower = content.to_lowercase();
    let mut result = ImportResult::default();
    let mut pos = 0;

    loop {
        let Some(rel) = lower[pos..].find("<a ") else {
            break;
        };
        let tag_start = pos + rel;

        let Some(close_rel) = content[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + close_rel;
        let tag = &content[tag_start..=tag_end];
        let tag_l = &lower[tag_start..=tag_end];

        if let Some(url) = attr_value(tag, tag_l, "href") {
            let after = tag_end + 1;
            let title_len = lower[after..].find("</a>").unwrap_or(0);
            let title = html_decode(content[after..after + title_len].trim());
            // ADD_DATE is Unix seconds; use 0 if absent (no timestamp in HTML exports)
            let captured_at = attr_value(tag, tag_l, "add_date")
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0)
                .max(0);

            if url.starts_with("http://") || url.starts_with("https://") {
                insert_bookmark(&url, &title, captured_at, db, key, &mut result);
            }
            pos = after + title_len + 4; // past </a>
        } else {
            pos = tag_end + 1;
        }
    }

    result
}

fn attr_value(tag: &str, tag_lower: &str, name: &str) -> Option<String> {
    let pattern = format!(" {}=\"", name);
    let start = tag_lower.find(&pattern)? + pattern.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn html_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

// ── Shared insert ─────────────────────────────────────────────────────────────

fn insert_bookmark(url: &str, title: &str, captured_at: i64, db: &Db, key: &str, result: &mut ImportResult) {
    let classified = classifier::classify(url);
    // UUID v5 derived from URL — same URL always produces same UUID, enabling
    // idempotent re-import without a URL index on the encrypted column.
    let uuid = Uuid::new_v5(&Uuid::NAMESPACE_URL, url.as_bytes()).to_string();
    let display_title = if title.is_empty() { &classified.domain } else { title };

    let new = NewResource {
        uuid,
        url: crypto::encrypt(url, key),
        title: crypto::encrypt(display_title, key),
        domain: classified.domain,
        category: classified.category,
        captured_at,
    };

    match db.insert_or_ignore(&new) {
        Ok(true) => result.imported += 1,
        Ok(false) => result.skipped += 1,
        Err(e) => result.errors.push(format!("insert error: {e}")),
    }
}
