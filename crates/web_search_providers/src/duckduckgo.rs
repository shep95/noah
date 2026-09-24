use std::sync::Arc;

use anyhow::{Result, bail};
use cloud_llm_client::{WebSearchResponse, WebSearchResult};
use futures::AsyncReadExt as _;
use gpui::{App, AppContext as _, Task};
use http_client::{AsyncBody, HttpClient, Method, Request};
use web_search::{WebSearchProvider, WebSearchProviderId};

pub const DUCKDUCKGO_WEB_SEARCH_PROVIDER_ID: &str = "duckduckgo";

const SEARCH_URL: &str = "https://html.duckduckgo.com/html/";
const MAX_RESULTS: usize = 10;

/// Searches through DuckDuckGo's HTML endpoint, which needs no account or API
/// key, so web search works for anyone who installs noah.
pub struct DuckDuckGoWebSearchProvider {
    http_client: Arc<dyn HttpClient>,
}

impl DuckDuckGoWebSearchProvider {
    pub fn new(http_client: Arc<dyn HttpClient>) -> Self {
        Self { http_client }
    }
}

impl WebSearchProvider for DuckDuckGoWebSearchProvider {
    fn id(&self) -> WebSearchProviderId {
        WebSearchProviderId(DUCKDUCKGO_WEB_SEARCH_PROVIDER_ID.into())
    }

    fn search(&self, query: String, cx: &mut App) -> Task<Result<WebSearchResponse>> {
        let http_client = self.http_client.clone();
        cx.background_spawn(async move { perform_search(http_client, query).await })
    }
}

async fn perform_search(
    http_client: Arc<dyn HttpClient>,
    query: String,
) -> Result<WebSearchResponse> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("q", &query)
        .finish();
    let request = Request::builder()
        .method(Method::POST)
        .uri(SEARCH_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(AsyncBody::from(body))?;
    let mut response = http_client.send(request).await?;
    let status = response.status();
    let mut html = String::new();
    response.body_mut().read_to_string(&mut html).await?;

    if !status.is_success() {
        bail!("DuckDuckGo search failed with status {status}");
    }
    let results = parse_results(&html);
    // DuckDuckGo answers suspected automated traffic with a challenge page
    // instead of an error status.
    if results.is_empty() && html.contains("anomaly") {
        bail!("DuckDuckGo is rate-limiting searches right now. Try again in a minute.");
    }
    Ok(WebSearchResponse { results })
}

fn parse_results(html: &str) -> Vec<WebSearchResult> {
    let mut results = Vec::new();
    let mut rest = html;
    while let Some(marker) = rest.find("class=\"result__a\"") {
        let Some(tag_start) = rest[..marker].rfind("<a") else {
            break;
        };
        let Some(tag_length) = rest[marker..].find('>') else {
            break;
        };
        let tag = &rest[tag_start..marker + tag_length];
        let after_tag = &rest[marker + tag_length + 1..];
        let title_end = after_tag.find("</a>").unwrap_or(after_tag.len());
        let title = clean_text(&after_tag[..title_end]);
        let after_title = &after_tag[title_end..];

        let next_result = after_title
            .find("class=\"result__a\"")
            .unwrap_or(after_title.len());
        let text = snippet(&after_title[..next_result]).unwrap_or_default();
        rest = after_title;

        let Some(url) = attribute(tag, "href").and_then(|href| resolve_url(&href)) else {
            continue;
        };
        if title.is_empty() {
            continue;
        }
        results.push(WebSearchResult { title, url, text });
        if results.len() == MAX_RESULTS {
            break;
        }
    }
    results
}

fn snippet(html: &str) -> Option<String> {
    let marker = html.find("class=\"result__snippet\"")?;
    let tag_start = html[..marker].rfind('<')?;
    let tag_name: String = html[tag_start + 1..]
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .collect();
    let content_start = marker + html[marker..].find('>')? + 1;
    let closing_tag = format!("</{tag_name}>");
    let content_end = html[content_start..]
        .find(&closing_tag)
        .map_or(html.len(), |offset| content_start + offset);
    Some(clean_text(&html[content_start..content_end]))
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = start + tag[start..].find('"')?;
    Some(decode_entities(&tag[start..end]))
}

/// Result links point at DuckDuckGo's redirector (`/l/?uddg=<target>`); the
/// real destination is the `uddg` parameter. Ad links go through `y.js` and are
/// dropped.
fn resolve_url(href: &str) -> Option<String> {
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };
    let parsed = url::Url::parse(&absolute).ok()?;
    let target = if parsed.domain() == Some("duckduckgo.com") {
        if parsed.path() != "/l/" {
            return None;
        }
        let (_, target) = parsed.query_pairs().find(|(key, _)| key == "uddg")?;
        url::Url::parse(&target).ok()?
    } else {
        parsed
    };
    matches!(target.scheme(), "http" | "https").then(|| target.to_string())
}

fn clean_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let entity_and_rest = &rest[start..];
        let Some(end) = entity_and_rest.find(';').filter(|end| *end <= 10) else {
            decoded.push('&');
            rest = &entity_and_rest[1..];
            continue;
        };
        let entity = &entity_and_rest[1..end];
        let character = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|decimal| decimal.parse().ok()))
                .and_then(char::from_u32),
        };
        match character {
            Some(character) => {
                decoded.push(character);
                rest = &entity_and_rest[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = &entity_and_rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"
<div class="result results_links results_links_deep result--ad">
  <h2 class="result__title"><a rel="nofollow" class="result__a" href="https://duckduckgo.com/y.js?ad_domain=example.com&amp;u3=x">Sponsored thing</a></h2>
  <a class="result__snippet" href="https://duckduckgo.com/y.js?x">An ad.</a>
</div>
<div class="result results_links results_links_deep web-result ">
  <h2 class="result__title">
    <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.rs%2Ftokio%2Flatest%2Ftokio%2Ftask%2Ffn.spawn_blocking.html&amp;rut=abc">spawn_<b>blocking</b> in tokio::task - Rust</a>
  </h2>
  <a class="result__snippet" href="//duckduckgo.com/l/?uddg=x">Runs the provided closure on a thread where blocking is acceptable &amp; won&#x27;t stall the runtime.</a>
</div>
<div class="result results_links results_links_deep web-result ">
  <h2 class="result__title"><a class="result__a" rel="nofollow" href="https://tokio.rs/tokio/tutorial">Tutorial | Tokio</a></h2>
  <div class="result__snippet">Tokio is an <b>asynchronous</b> runtime.</div>
</div>
"#;

    #[test]
    fn parses_organic_results_and_skips_ads() {
        let results = parse_results(PAGE);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "spawn_blocking in tokio::task - Rust");
        assert_eq!(
            results[0].url,
            "https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html"
        );
        assert_eq!(
            results[0].text,
            "Runs the provided closure on a thread where blocking is acceptable & won't stall the runtime."
        );
        assert_eq!(results[1].url, "https://tokio.rs/tokio/tutorial");
        assert_eq!(results[1].text, "Tokio is an asynchronous runtime.");
    }

    #[test]
    fn rejects_non_web_urls() {
        assert_eq!(resolve_url("javascript:alert(1)"), None);
        assert_eq!(
            resolve_url("//duckduckgo.com/l/?uddg=file%3A%2F%2F%2Fetc%2Fpasswd"),
            None
        );
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode_entities("a &amp; b &#39;c&#x27; &bogus; &"), "a & b 'c' &bogus; &");
    }
}
