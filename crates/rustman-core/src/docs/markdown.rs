use super::{auth_text, body_text, curl_snippet, method_str};
use crate::models::collection::{Collection, Item};
use crate::models::http::HttpRequest;

/// Generates Markdown API documentation from a [`Collection`].
pub struct MarkdownDocGenerator;

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if c == ' ' {
            out.push('-');
        }
    }
    out
}

impl MarkdownDocGenerator {
    pub fn generate(collection: &Collection) -> String {
        let mut out = format!("# {}\n\n", collection.name);
        if let Some(d) = collection.description.as_deref().filter(|d| !d.trim().is_empty()) {
            out.push_str(d.trim());
            out.push_str("\n\n");
        }
        out.push_str("## Table of Contents\n\n");
        let mut toc = String::new();
        let mut body = String::new();
        let mut seen = std::collections::HashMap::new();
        Self::walk(&collection.items, 0, &mut toc, &mut body, &mut seen);
        if toc.is_empty() {
            out.push_str("_No endpoints._\n");
        } else {
            out.push_str(&toc);
            out.push('\n');
            out.push_str(&body);
        }
        out
    }

    /// GitHub-style unique anchor for a heading text.
    fn anchor(heading: &str, seen: &mut std::collections::HashMap<String, usize>) -> String {
        let base = slug(heading);
        let n = seen.entry(base.clone()).or_insert(0);
        let a = if *n == 0 { base } else { format!("{base}-{n}") };
        *n += 1;
        a
    }

    fn walk(
        items: &[Item],
        depth: usize,
        toc: &mut String,
        body: &mut String,
        seen: &mut std::collections::HashMap<String, usize>,
    ) {
        let indent = "  ".repeat(depth);
        for item in items {
            match item {
                Item::Folder(f) => {
                    let heading = f.name.clone();
                    let a = Self::anchor(&heading, seen);
                    toc.push_str(&format!("{indent}- [{}](#{a})\n", f.name));
                    let hashes = "#".repeat((depth + 2).min(6));
                    body.push_str(&format!("{hashes} {}\n\n", f.name));
                    if let Some(d) = f.description.as_deref().filter(|d| !d.trim().is_empty()) {
                        body.push_str(d.trim());
                        body.push_str("\n\n");
                    }
                    Self::walk(&f.items, depth + 1, toc, body, seen);
                }
                Item::Request(r) => {
                    let heading = format!("{} {}", method_str(r.method), r.url);
                    let a = Self::anchor(&heading, seen);
                    toc.push_str(&format!("{indent}- [{} `{}`](#{a})\n", r.name, method_str(r.method)));
                    Self::request(r, depth, body);
                }
            }
        }
    }

    fn request(r: &HttpRequest, depth: usize, out: &mut String) {
        let hashes = "#".repeat((depth + 2).min(6));
        let sub = "#".repeat((depth + 3).min(6));
        out.push_str(&format!("{hashes} {} {}\n\n", method_str(r.method), r.url));
        out.push_str(&format!("**{}**\n\n", r.name));

        out.push_str(&format!("{sub} Headers\n\n"));
        if r.headers.is_empty() {
            out.push_str("_None_\n\n");
        } else {
            out.push_str("| Key | Value | Description |\n|-----|-------|-------------|\n");
            for h in &r.headers {
                let d = if h.enabled { "" } else { "disabled" };
                out.push_str(&format!("| {} | {} | {d} |\n", cell(&h.key), cell(&h.value)));
            }
            out.push('\n');
        }

        out.push_str(&format!("{sub} Query Params\n\n"));
        if r.query_params.is_empty() {
            out.push_str("_None_\n\n");
        } else {
            out.push_str("| Key | Value |\n|-----|-------|\n");
            for q in &r.query_params {
                out.push_str(&format!("| {} | {} |\n", cell(&q.key), cell(&q.value)));
            }
            out.push('\n');
        }

        out.push_str(&format!("{sub} Auth\n\n{}\n\n", auth_text(&r.auth)));

        if let Some((lang, text)) = body_text(&r.body) {
            out.push_str(&format!("{sub} Request Body\n\n```{lang}\n{text}\n```\n\n"));
        }

        out.push_str(&format!(
            "{sub} cURL\n\n```bash\n{}\n```\n\n",
            curl_snippet(r)
        ));
    }
}
