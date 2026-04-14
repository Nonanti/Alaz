//! Content ingestion module — fetch URLs and store as KnowledgeItems.
//!
//! Provides a pipeline to:
//! 1. Fetch web content via HTTP
//! 2. Extract readable text with HTML-to-markdown conversion (powered by scraper/html5ever)
//! 3. Store as a `KnowledgeItem` with kind="reference"

use alaz_core::models::CreateKnowledge;
use alaz_core::{AlazError, Result};
use alaz_db::repos::{KnowledgeRepo, ProjectRepo};
use ego_tree::NodeRef;
use scraper::{Html, Node, Selector};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::time::Duration;
use tracing::{debug, warn};

/// Maximum allowed content length (500 KB).
const MAX_CONTENT_LENGTH: usize = 500 * 1024;
/// Minimum content length to be considered valid.
const MIN_CONTENT_LENGTH: usize = 100;
/// HTTP request timeout.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// User-Agent header for requests.
const USER_AGENT: &str = "Alaz/1.0 (Knowledge Ingestion Bot)";

/// Request to ingest a URL into the knowledge base.
#[derive(Debug, Deserialize)]
pub struct IngestRequest {
    pub url: String,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
    pub title_override: Option<String>,
}

/// Result of a successful ingestion.
#[derive(Debug, Serialize)]
pub struct IngestResult {
    pub knowledge_id: String,
    pub title: String,
    pub content_length: usize,
    pub source_url: String,
}

/// Check if a URL points to a private/internal network address.
/// Blocks localhost, private IP ranges, and link-local addresses.
fn is_private_url(url: &str) -> bool {
    let host = url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");

    // Localhost variants
    if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "0.0.0.0" {
        return true;
    }

    // Private IPv4 ranges
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified();
    }

    // Common internal hostnames
    if host.ends_with(".local") || host.ends_with(".internal") || host.ends_with(".lan") {
        return true;
    }

    false
}

/// Fetches web content and stores it as knowledge items.
pub struct ContentIngester {
    pool: PgPool,
    http_client: reqwest::Client,
}

impl ContentIngester {
    /// Create a new ingester with the given database pool.
    pub fn new(pool: PgPool) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build HTTP client");

        Self { pool, http_client }
    }

    /// Ingest a URL: fetch, extract, and store as a KnowledgeItem.
    pub async fn ingest_url(&self, req: IngestRequest) -> Result<IngestResult> {
        // Validate URL format
        if !req.url.starts_with("http://") && !req.url.starts_with("https://") {
            return Err(AlazError::Validation(
                "URL must start with http:// or https://".into(),
            ));
        }

        if is_private_url(&req.url) {
            return Err(AlazError::Validation(
                "cannot ingest from private/internal network addresses".into(),
            ));
        }

        // Check for duplicate URL
        self.check_duplicate(&req.url).await?;

        // Fetch and extract
        let (auto_title, content) = Self::fetch_and_extract(&self.http_client, &req.url).await?;

        let title = req.title_override.unwrap_or(auto_title);
        let content_length = content.len();

        // Resolve project
        let project_id = if let Some(ref name) = req.project {
            Some(ProjectRepo::get_or_create(&self.pool, name, None).await?.id)
        } else {
            None
        };

        // Store as KnowledgeItem
        let create = CreateKnowledge {
            title: title.clone(),
            content: content.clone(),
            description: Some(format!("Ingested from {}", req.url)),
            kind: Some("reference".into()),
            language: None,
            file_path: None,
            project: req.project.clone(),
            tags: req.tags,
            valid_from: None,
            valid_until: None,
            source: Some("web_ingest".into()),
            source_metadata: Some(serde_json::json!({
                "url": req.url,
                "ingested_at": chrono::Utc::now().to_rfc3339(),
            })),
        };

        let item = KnowledgeRepo::create(&self.pool, &create, project_id.as_deref()).await?;

        debug!(
            knowledge_id = %item.id,
            url = %req.url,
            content_length,
            "URL ingested successfully"
        );

        Ok(IngestResult {
            knowledge_id: item.id,
            title,
            content_length,
            source_url: req.url,
        })
    }

    /// Fetch and extract content without storing (useful for preview).
    ///
    /// Returns `(title, markdown_content)`.
    pub async fn fetch_content(url: &str) -> Result<(String, String)> {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| AlazError::ServiceUnavailable(format!("HTTP client error: {e}")))?;

        Self::fetch_and_extract(&client, url).await
    }

    /// Internal: fetch URL and extract readable content.
    async fn fetch_and_extract(client: &reqwest::Client, url: &str) -> Result<(String, String)> {
        if is_private_url(url) {
            return Err(AlazError::Validation(
                "cannot fetch from private/internal network addresses".into(),
            ));
        }

        let response = client.get(url).send().await.map_err(|e| {
            warn!(url, error = %e, "failed to fetch URL");
            AlazError::ServiceUnavailable(format!("failed to fetch URL: {e}"))
        })?;

        let status = response.status();
        if !status.is_success() {
            return Err(AlazError::Validation(format!("URL returned HTTP {status}")));
        }

        let html = response.text().await.map_err(|e| {
            AlazError::ServiceUnavailable(format!("failed to read response body: {e}"))
        })?;

        if html.len() > MAX_CONTENT_LENGTH {
            return Err(AlazError::Validation(format!(
                "content too large: {} bytes (max {})",
                html.len(),
                MAX_CONTENT_LENGTH
            )));
        }

        let title = extract_title(&html).unwrap_or_else(|| "Untitled".to_string());
        let content = html_to_markdown(&html);

        if content.len() < MIN_CONTENT_LENGTH {
            return Err(AlazError::Validation(format!(
                "extracted content too short: {} chars (min {MIN_CONTENT_LENGTH})",
                content.len()
            )));
        }

        Ok((title, content))
    }

    /// Check if a KnowledgeItem with the same source URL already exists.
    async fn check_duplicate(&self, url: &str) -> Result<()> {
        let exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM knowledge_items
                WHERE source_metadata->>'url' = $1
            )
            "#,
        )
        .bind(url)
        .fetch_one(&self.pool)
        .await?;

        if exists {
            return Err(AlazError::Duplicate(format!("URL already ingested: {url}")));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// HTML extraction helpers (powered by scraper / html5ever)
// ---------------------------------------------------------------------------

/// Tags whose entire subtree should be skipped during conversion.
const NOISE_TAGS: &[&str] = &[
    "script", "style", "nav", "footer", "header", "aside", "noscript", "svg",
];

/// Extract the page title from HTML.
///
/// Priority: `<title>` tag → first `<h1>` → first non-empty text line → `None`.
pub(crate) fn extract_title(html: &str) -> Option<String> {
    let doc = Html::parse_document(html);

    // Try <title>
    if let Ok(sel) = Selector::parse("title")
        && let Some(el) = doc.select(&sel).next()
    {
        let t = collect_text_from_element(el).trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }

    // Try first <h1>
    if let Ok(sel) = Selector::parse("h1")
        && let Some(el) = doc.select(&sel).next()
    {
        let t = collect_text_from_element(el).trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }

    // Fallback: first non-empty text line from the whole document
    let text = collect_all_text(&doc);
    text.lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(200).collect())
}

/// Convert HTML to a simplified markdown representation.
///
/// 1. Parse with html5ever via `scraper`
/// 2. Locate best content container (`<article>` → `<main>` → `<body>`)
/// 3. Recursively walk the DOM, converting elements to markdown
/// 4. Clean up whitespace
pub(crate) fn html_to_markdown(html: &str) -> String {
    let doc = Html::parse_document(html);

    // Find the best content root
    let root_node: NodeRef<'_, Node> = find_content_root(&doc);

    // Walk the tree and convert
    let md = node_to_markdown(root_node, false);

    clean_whitespace(&md)
}

/// Extract the inner HTML of the preferred content container.
///
/// Preference: `<article>` → `<main>` → `<body>` → entire document.
#[allow(dead_code)]
fn extract_main_content(html: &str) -> String {
    let doc = Html::parse_document(html);
    let root: NodeRef<'_, Node> = find_content_root(&doc);

    // Collect inner HTML from the chosen root
    let mut out = String::new();
    for child in root.children() {
        collect_inner_html(child, &mut out);
    }
    out
}

/// Strip all HTML tags, keeping only text content.
///
/// Uses the html5ever parser so it handles malformed HTML correctly.
#[allow(dead_code)]
pub(crate) fn strip_tags(html: &str) -> String {
    let fragment = Html::parse_fragment(html);
    collect_all_text(&fragment)
}

// ---------------------------------------------------------------------------
// DOM walking
// ---------------------------------------------------------------------------

/// Find the best content root node in a parsed document.
///
/// Tries `<article>`, `<main>`, `<body>` in order, falls back to the document root.
fn find_content_root(doc: &Html) -> NodeRef<'_, Node> {
    for tag in &["article", "main", "body"] {
        if let Ok(sel) = Selector::parse(tag)
            && let Some(el) = doc.select(&sel).next()
        {
            return doc.tree.get(el.id()).expect("element must exist in tree");
        }
    }
    doc.tree.root()
}

/// Recursively convert a DOM node to markdown.
///
/// `in_pre` tracks whether we're inside a `<pre>` block (preserve whitespace, no
/// further markdown conversion).
fn node_to_markdown(node: NodeRef<'_, Node>, in_pre: bool) -> String {
    match node.value() {
        Node::Text(text) => {
            let s: &str = text;
            if in_pre {
                s.to_string()
            } else {
                // Collapse runs of whitespace to a single space (normal HTML behavior)
                collapse_whitespace(s)
            }
        }
        Node::Element(elem) => {
            let tag = elem.name.local.as_ref();

            // Skip noise subtrees entirely
            if NOISE_TAGS.contains(&tag) {
                return String::new();
            }

            // --- <pre> blocks: preserve literal content ---
            if tag == "pre" {
                let (lang, code) = extract_pre_content(node);
                return format!("\n\n```{lang}\n{code}\n```\n\n");
            }

            // --- <br> ---
            if tag == "br" {
                return "\n".to_string();
            }

            // --- <img> ---
            if tag == "img" {
                let alt = elem.attr("alt").unwrap_or("");
                let src = elem.attr("src").unwrap_or("");
                return format!("![{alt}]({src})");
            }

            // Recurse into children
            let children_md: String = node
                .children()
                .map(|child| node_to_markdown(child, in_pre))
                .collect();

            match tag {
                // Headings
                "h1" => format!("\n\n# {}\n\n", children_md.trim()),
                "h2" => format!("\n\n## {}\n\n", children_md.trim()),
                "h3" => format!("\n\n### {}\n\n", children_md.trim()),
                "h4" => format!("\n\n#### {}\n\n", children_md.trim()),
                "h5" => format!("\n\n##### {}\n\n", children_md.trim()),
                "h6" => format!("\n\n###### {}\n\n", children_md.trim()),

                // Paragraphs
                "p" => format!("\n\n{}\n\n", children_md.trim()),

                // Inline formatting
                "strong" | "b" => format!("**{}**", children_md.trim()),
                "em" | "i" => format!("*{}*", children_md.trim()),
                "code" => format!("`{}`", children_md.trim()),

                // Links
                "a" => {
                    let href = elem.attr("href").unwrap_or("");
                    let text = children_md.trim();
                    if href.is_empty() || text.is_empty() {
                        text.to_string()
                    } else {
                        format!("[{text}]({href})")
                    }
                }

                // Lists
                "ul" | "ol" => format!("\n{}\n", children_md),
                "li" => format!("\n- {}", children_md.trim()),

                // Block-level & other: pass through children
                _ => children_md,
            }
        }
        Node::Document | Node::Fragment => {
            // Top-level container — recurse into children
            node.children()
                .map(|child| node_to_markdown(child, in_pre))
                .collect()
        }
        // Comments, doctypes, processing instructions → skip
        _ => String::new(),
    }
}

/// Extract content and optional language hint from a `<pre>` node.
///
/// Handles `<pre><code class="language-xxx">…</code></pre>` and plain `<pre>…</pre>`.
fn extract_pre_content(pre_node: NodeRef<'_, Node>) -> (String, String) {
    // Look for a direct <code> child element
    for child in pre_node.children() {
        if let Node::Element(elem) = child.value()
            && elem.name.local.as_ref() == "code"
        {
            let lang = extract_language_from_class(elem);
            let text = collect_text_recursive(child);
            return (lang, text);
        }
    }
    // No <code> wrapper — just get all text
    let text = collect_text_recursive(pre_node);
    (String::new(), text)
}

/// Extract language hint from a `class="language-xxx"` attribute.
fn extract_language_from_class(elem: &scraper::node::Element) -> String {
    let Some(cls) = elem.attr("class") else {
        return String::new();
    };
    for part in cls.split_whitespace() {
        if let Some(lang) = part.strip_prefix("language-") {
            return lang.to_string();
        }
    }
    String::new()
}

/// Collect all text content from a node and its descendants (raw, no collapsing).
fn collect_text_recursive(node: NodeRef<'_, Node>) -> String {
    let mut out = String::new();
    for child in node.children() {
        match child.value() {
            Node::Text(t) => {
                let s: &str = t;
                out.push_str(s);
            }
            Node::Element(_) => out.push_str(&collect_text_recursive(child)),
            _ => {}
        }
    }
    out
}

/// Collect all visible text from an `ElementRef`.
fn collect_text_from_element(el: scraper::ElementRef<'_>) -> String {
    el.text().collect::<Vec<_>>().join("")
}

/// Collect all text nodes from an entire parsed document/fragment.
fn collect_all_text(doc: &Html) -> String {
    let mut out = String::new();
    for node in doc.tree.nodes() {
        if let Node::Text(t) = node.value() {
            let s: &str = t;
            out.push_str(s);
        }
    }
    out
}

/// Serialize a node subtree back to raw HTML (used by `extract_main_content`).
#[allow(dead_code)]
fn collect_inner_html(node: NodeRef<'_, Node>, out: &mut String) {
    match node.value() {
        Node::Text(t) => {
            let s: &str = t;
            out.push_str(s);
        }
        Node::Element(elem) => {
            let tag = elem.name.local.as_ref();
            out.push('<');
            out.push_str(tag);
            for (name, val) in elem.attrs() {
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                out.push_str(val);
                out.push('"');
            }
            out.push('>');
            for child in node.children() {
                collect_inner_html(child, out);
            }
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        _ => {}
    }
}

/// Collapse runs of ASCII whitespace into a single space.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    out
}

/// Decode common HTML entities.
#[allow(dead_code)]
fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
}

/// Clean up excessive whitespace while preserving markdown structure.
///
/// Collapses 3+ consecutive blank lines into a single blank line.
fn clean_whitespace(text: &str) -> String {
    let trimmed: Vec<String> = text.lines().map(|l| l.trim().to_string()).collect();

    let mut result = Vec::new();
    let mut consecutive_empty = 0;

    for line in &trimmed {
        if line.is_empty() {
            consecutive_empty += 1;
            if consecutive_empty <= 1 {
                result.push(String::new());
            }
        } else {
            consecutive_empty = 0;
            result.push(line.clone());
        }
    }

    result.join("\n").trim().to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- is_private_url --

    #[test]
    fn test_is_private_url_localhost() {
        assert!(is_private_url("http://localhost/path"));
        assert!(is_private_url("http://127.0.0.1:8080/api"));
        assert!(is_private_url("http://0.0.0.0/"));
    }

    #[test]
    fn test_is_private_url_private_ranges() {
        assert!(is_private_url("http://192.168.1.1/admin"));
        assert!(is_private_url("http://10.0.0.1/internal"));
        assert!(is_private_url("http://172.16.0.1/api"));
    }

    #[test]
    fn test_is_private_url_public() {
        assert!(!is_private_url("https://example.com/page"));
        assert!(!is_private_url("https://github.com/repo"));
        assert!(!is_private_url("http://8.8.8.8/dns"));
    }

    #[test]
    fn test_is_private_url_internal_hostnames() {
        assert!(is_private_url("http://myserver.local/api"));
        assert!(is_private_url("http://db.internal/health"));
        assert!(is_private_url("http://printer.lan/status"));
    }

    // -- strip_tags --

    #[test]
    fn test_strip_tags_basic() {
        assert_eq!(strip_tags("<p>Hello <b>world</b></p>"), "Hello world");
        assert_eq!(strip_tags("no tags here"), "no tags here");
        assert_eq!(strip_tags("<div><span>nested</span></div>"), "nested");
    }

    // -- extract_title --

    #[test]
    fn test_extract_title_from_title_tag() {
        let html = r#"<html><head><title>My Page Title</title></head><body>Content</body></html>"#;
        assert_eq!(extract_title(html).unwrap(), "My Page Title");
    }

    #[test]
    fn test_extract_title_from_h1() {
        let html = r#"<html><body><h1>Main Heading</h1><p>Content</p></body></html>"#;
        assert_eq!(extract_title(html).unwrap(), "Main Heading");
    }

    #[test]
    fn test_extract_title_fallback_to_first_line() {
        let html = r#"<html><body><p>First paragraph text</p></body></html>"#;
        assert_eq!(extract_title(html).unwrap(), "First paragraph text");
    }

    #[test]
    fn test_extract_title_prefers_title_over_h1() {
        let html =
            r#"<html><head><title>Page Title</title></head><body><h1>Heading</h1></body></html>"#;
        assert_eq!(extract_title(html).unwrap(), "Page Title");
    }

    // -- html_to_markdown --

    #[test]
    fn test_html_to_markdown_headings() {
        let html = "<h1>Title</h1><h2>Subtitle</h2><h3>Section</h3>";
        let md = html_to_markdown(html);
        assert!(md.contains("# Title"), "got: {md}");
        assert!(md.contains("## Subtitle"), "got: {md}");
        assert!(md.contains("### Section"), "got: {md}");
    }

    #[test]
    fn test_html_to_markdown_links() {
        let html = r#"<p>Visit <a href="https://example.com">Example</a> site</p>"#;
        let md = html_to_markdown(html);
        assert!(md.contains("[Example](https://example.com)"), "got: {md}");
    }

    #[test]
    fn test_html_to_markdown_code_blocks() {
        let html = r#"<pre><code class="language-rust">fn main() {}</code></pre>"#;
        let md = html_to_markdown(html);
        assert!(md.contains("```rust"), "got: {md}");
        assert!(md.contains("fn main() {}"), "got: {md}");
        assert!(md.contains("```"), "got: {md}");
    }

    #[test]
    fn test_html_to_markdown_list_items() {
        let html = "<ul><li>First</li><li>Second</li><li>Third</li></ul>";
        let md = html_to_markdown(html);
        assert!(md.contains("- First"), "got: {md}");
        assert!(md.contains("- Second"), "got: {md}");
        assert!(md.contains("- Third"), "got: {md}");
    }

    #[test]
    fn test_html_to_markdown_removes_noise_elements() {
        let html = r#"
            <html>
            <body>
                <nav>Navigation stuff</nav>
                <main><p>Actual content here</p></main>
                <footer>Footer stuff</footer>
                <script>var x = 1;</script>
                <style>.foo { color: red; }</style>
            </body>
            </html>
        "#;
        let md = html_to_markdown(html);
        assert!(
            md.contains("Actual content"),
            "should contain main content, got: {md}"
        );
        assert!(
            !md.contains("Navigation stuff"),
            "should strip nav, got: {md}"
        );
        assert!(
            !md.contains("Footer stuff"),
            "should strip footer, got: {md}"
        );
        assert!(!md.contains("var x = 1"), "should strip script, got: {md}");
        assert!(!md.contains("color: red"), "should strip style, got: {md}");
    }

    #[test]
    fn test_html_to_markdown_inline_formatting() {
        let html = "<p><strong>Bold</strong> and <em>italic</em> and <code>code</code></p>";
        let md = html_to_markdown(html);
        assert!(md.contains("**Bold**"), "got: {md}");
        assert!(md.contains("*italic*"), "got: {md}");
        assert!(md.contains("`code`"), "got: {md}");
    }

    #[test]
    fn test_html_to_markdown_images() {
        let html = r#"<p>See <img src="pic.png" alt="a photo"> here</p>"#;
        let md = html_to_markdown(html);
        assert!(md.contains("![a photo](pic.png)"), "got: {md}");
    }

    // -- decode_entities --

    #[test]
    fn test_decode_entities() {
        assert_eq!(decode_entities("&amp; &lt; &gt;"), "& < >");
        assert_eq!(decode_entities("&quot;hello&quot;"), "\"hello\"");
        assert_eq!(decode_entities("it&#39;s"), "it's");
    }

    // -- extract_main_content --

    #[test]
    fn test_extract_main_content_prefers_article() {
        let html = r#"<html><body><nav>Nav</nav><article>Article content</article></body></html>"#;
        let content = extract_main_content(html);
        assert!(
            content.contains("Article content"),
            "should prefer <article>, got: {content}"
        );
    }

    // -- clean_whitespace --

    #[test]
    fn test_clean_whitespace() {
        let input = "line1\n\n\n\n\nline2\n\n\nline3";
        let result = clean_whitespace(input);
        assert!(!result.contains("\n\n\n"));
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line3"));
    }

    // -- NEW: nested HTML --

    #[test]
    fn test_nested_html_structure() {
        let html = r#"
            <div class="wrapper">
                <article>
                    <p>Intro text with <a href="https://example.com">a <strong>bold</strong> link</a>.</p>
                    <div class="content">
                        <p>Nested paragraph inside div.</p>
                    </div>
                </article>
            </div>
        "#;
        let md = html_to_markdown(html);
        assert!(
            md.contains("[a **bold** link](https://example.com)"),
            "should handle nested inline elements inside links, got: {md}"
        );
        assert!(md.contains("Intro text"), "got: {md}");
        assert!(md.contains("Nested paragraph inside div."), "got: {md}");
    }

    // -- NEW: malformed / unclosed tags --

    #[test]
    fn test_malformed_unclosed_tags() {
        let html = r#"
            <html><body>
                <p>First paragraph
                <p>Second paragraph with <b>unclosed bold
                <p>Third paragraph
                <div><span>Nested unclosed
            </body></html>
        "#;
        let md = html_to_markdown(html);
        // html5ever repairs the DOM — all text should survive
        assert!(md.contains("First paragraph"), "got: {md}");
        assert!(md.contains("Second paragraph"), "got: {md}");
        assert!(md.contains("unclosed bold"), "got: {md}");
        assert!(md.contains("Third paragraph"), "got: {md}");
        assert!(md.contains("Nested unclosed"), "got: {md}");
    }

    // -- NEW: real-world-like page with nav/footer noise --

    #[test]
    fn test_real_world_page_with_noise() {
        let html = r#"
            <!DOCTYPE html>
            <html lang="en">
            <head>
                <meta charset="utf-8">
                <title>Blog Post — My Site</title>
                <style>body { margin: 0; }</style>
                <script>window.analytics = {};</script>
            </head>
            <body>
                <header>
                    <nav>
                        <a href="/">Home</a>
                        <a href="/about">About</a>
                    </nav>
                </header>
                <main>
                    <article>
                        <h1>Understanding Rust Lifetimes</h1>
                        <p>Lifetimes are a <strong>key concept</strong> in Rust.</p>
                        <pre><code class="language-rust">fn longest&lt;'a&gt;(x: &amp;'a str, y: &amp;'a str) -&gt; &amp;'a str {
    if x.len() &gt; y.len() { x } else { y }
}</code></pre>
                        <p>See the <a href="https://doc.rust-lang.org/book/">Rust Book</a> for more.</p>
                        <ul>
                            <li>Borrow checker</li>
                            <li>Ownership model</li>
                        </ul>
                    </article>
                </main>
                <aside>
                    <h3>Related Posts</h3>
                    <a href="/post2">Another post</a>
                </aside>
                <footer>
                    <p>&copy; 2026 My Site</p>
                    <script>trackPageView();</script>
                </footer>
            </body>
            </html>
        "#;
        let md = html_to_markdown(html);

        // Should contain the article content
        assert!(
            md.contains("# Understanding Rust Lifetimes"),
            "missing h1, got: {md}"
        );
        assert!(md.contains("**key concept**"), "missing bold, got: {md}");
        assert!(md.contains("```rust"), "missing code fence, got: {md}");
        assert!(md.contains("fn longest"), "missing code content, got: {md}");
        assert!(
            md.contains("[Rust Book](https://doc.rust-lang.org/book/)"),
            "missing link, got: {md}"
        );
        assert!(
            md.contains("- Borrow checker"),
            "missing list item, got: {md}"
        );
        assert!(
            md.contains("- Ownership model"),
            "missing list item, got: {md}"
        );

        // Should NOT contain noise
        assert!(!md.contains("Home"), "nav link leaked, got: {md}");
        assert!(!md.contains("About"), "nav link leaked, got: {md}");
        assert!(!md.contains("Related Posts"), "aside leaked, got: {md}");
        assert!(!md.contains("Another post"), "aside link leaked, got: {md}");
        assert!(!md.contains("2026 My Site"), "footer leaked, got: {md}");
        assert!(!md.contains("analytics"), "script leaked, got: {md}");
        assert!(!md.contains("trackPageView"), "script leaked, got: {md}");
        assert!(!md.contains("margin"), "style leaked, got: {md}");
    }
}
