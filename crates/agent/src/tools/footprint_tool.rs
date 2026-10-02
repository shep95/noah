use std::collections::HashSet;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use futures::{AsyncReadExt as _, FutureExt as _};
use gpui::{App, AppContext as _, SharedString, Task};
use http_client::{AsyncBody, HttpClientWithUrl};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::sandboxing::{NetworkRequest, SandboxRequest};
use crate::{AgentTool, ToolCallEventStream, ToolInput};

/// Looks a name up the way a stranger would: the same query through several
/// search engines and countries at once, with no cookies and no sign-in, so
/// the results are what the public sees, not what the person's own history
/// shapes. Use it before depending on something: a company, a package
/// author, a domain, a person's public footprint when they ask about their
/// own. Results come back grouped by site, with which engine and country
/// found each one. Quote what you rely on and say which results you did not
/// open.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FootprintToolInput {
    /// What to look up, as you'd type it into a search box. Put a name in
    /// quotes for an exact match.
    pub query: String,
    /// Two-letter country codes to search from (`us`, `de`, `jp`, `br`...).
    /// Defaults to `us`; at most six.
    #[serde(default)]
    pub countries: Vec<String>,
}

pub struct FootprintTool {
    http_client: Arc<HttpClientWithUrl>,
}

impl FootprintTool {
    pub fn new(http_client: Arc<HttpClientWithUrl>) -> Self {
        Self { http_client }
    }
}

const MAX_COUNTRIES: usize = 6;
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_RESULTS: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    DuckDuckGo,
    Bing,
    Mojeek,
}

impl Engine {
    const ALL: [Engine; 3] = [Engine::DuckDuckGo, Engine::Bing, Engine::Mojeek];

    fn name(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "duckduckgo",
            Engine::Bing => "bing",
            Engine::Mojeek => "mojeek",
        }
    }

    fn host(self) -> &'static str {
        match self {
            Engine::DuckDuckGo => "html.duckduckgo.com",
            Engine::Bing => "www.bing.com",
            Engine::Mojeek => "www.mojeek.com",
        }
    }

    fn url(self, query: &str, country: &str) -> String {
        let query: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        match self {
            Engine::DuckDuckGo => {
                format!("https://html.duckduckgo.com/html/?q={query}&kl={country}-{country}")
            }
            Engine::Bing => format!("https://www.bing.com/search?q={query}&cc={country}"),
            Engine::Mojeek => format!("https://www.mojeek.com/search?q={query}&reg={country}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    pub url: String,
    pub title: String,
}

fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(out.split_whitespace().collect::<Vec<_>>().join(" ").as_str())
}

/// DuckDuckGo's html results link through `/l/?uddg=<target>`; the target is
/// the result.
fn unwrap_redirect(href: &str) -> Option<String> {
    let href = decode_entities(href);
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href
    };
    let parsed = url::Url::parse(&absolute).ok()?;
    if parsed.path().starts_with("/l/")
        && let Some((_, target)) = parsed.query_pairs().find(|(key, _)| key == "uddg")
    {
        return Some(target.into_owned());
    }
    Some(absolute)
}

/// Result links from an engine's page: absolute http(s) links off the
/// engine's own domain, with their link text.
pub(crate) fn parse_results(html: &str, engine_host: &str) -> Vec<Found> {
    let Ok(anchor) = regex::Regex::new(r#"(?is)<a\b[^>]*?href="([^"]+)"[^>]*>(.*?)</a>"#) else {
        return Vec::new();
    };
    let engine_domain = engine_host.trim_start_matches("www.").trim_start_matches("html.");
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for capture in anchor.captures_iter(html) {
        let (Some(href), Some(text)) = (capture.get(1), capture.get(2)) else {
            continue;
        };
        let Some(url) = unwrap_redirect(href.as_str()) else {
            continue;
        };
        let Ok(parsed) = url::Url::parse(&url) else {
            continue;
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            continue;
        }
        let Some(host) = parsed.host_str() else {
            continue;
        };
        let own = host.ends_with(engine_domain)
            || host.ends_with("microsoft.com")
            || host.ends_with("bing.net")
            || host.ends_with("duckduckgo.com");
        let title = strip_tags(text.as_str());
        if own || title.chars().count() < 3 || !seen.insert(url.clone()) {
            continue;
        }
        found.push(Found { url, title });
    }
    found
}

fn site_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.trim_start_matches("www.").to_string()))
        .unwrap_or_default()
}

async fn search(
    http_client: Arc<HttpClientWithUrl>,
    url: String,
) -> anyhow::Result<String> {
    let mut response = http_client.get(&url, AsyncBody::default(), true).await?;
    anyhow::ensure!(
        response.status().is_success(),
        "status {}",
        response.status().as_u16()
    );
    let mut body = Vec::new();
    response
        .body_mut()
        .take(MAX_BODY_BYTES as u64)
        .read_to_end(&mut body)
        .await?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

impl AgentTool for FootprintTool {
    type Input = FootprintToolInput;
    type Output = String;

    const NAME: &'static str = "footprint";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Search
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => format!("footprint: {}", input.query).into(),
            Err(_) => "footprint".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let http_client = self.http_client.clone();
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|error| error.to_string())?;
            let query = input.query.trim().to_string();
            if query.is_empty() {
                return Err("give something to look up".to_string());
            }
            let mut countries: Vec<String> = input
                .countries
                .iter()
                .map(|country| country.trim().to_ascii_lowercase())
                .filter(|country| country.len() == 2 && country.chars().all(|c| c.is_ascii_alphabetic()))
                .collect();
            countries.dedup();
            countries.truncate(MAX_COUNTRIES);
            if countries.is_empty() {
                countries.push("us".to_string());
            }
            let hosts: Vec<&str> = Engine::ALL.iter().map(|engine| engine.host()).collect();
            for host in &hosts {
                cx.update(|cx| crate::trust::check_host_allowed(&format!("https://{host}/"), cx))
                    .map_err(|error| format!("{error:#}"))?;
            }
            let authorize = cx.update(|cx| {
                let context = crate::ToolPermissionContext::new(Self::NAME, vec![query.clone()]);
                event_stream.authorize(
                    format!(
                        "look up \"{query}\" on {} from {}",
                        hosts.join(", "),
                        countries.join(", ")
                    ),
                    context,
                    cx,
                )
            });
            futures::select! {
                result = authorize.fuse() => result.map_err(|error| error.to_string())?,
                _ = event_stream.cancelled_by_user().fuse() => return Err("cancelled".to_string()),
            };
            let unsandboxed = cx.update(|cx| {
                !crate::sandboxing::sandboxing_enabled(cx)
                    || event_stream.unsandboxed_access_granted(cx)
            });
            if !unsandboxed {
                let patterns = hosts
                    .iter()
                    .map(|host| super::fetch_tool::host_pattern_for_url(host))
                    .collect::<anyhow::Result<Vec<_>>>()
                    .map_err(|error| format!("{error:#}"))?;
                let grant = cx.update(|cx| {
                    event_stream.authorize_sandbox(
                        SandboxRequest {
                            network: NetworkRequest::Hosts(patterns),
                            ..Default::default()
                        },
                        String::new(),
                        cx,
                    )
                });
                grant.await.map_err(|error| error.to_string())?;
                for host in &hosts {
                    let url = format!("https://{host}/");
                    cx.background_spawn(async move {
                        super::fetch_tool::verify_host_not_forbidden(&url)
                    })
                    .await
                    .map_err(|error| format!("{error:#}"))?;
                }
            }
            let mut searches = Vec::new();
            for country in &countries {
                for engine in Engine::ALL {
                    let http_client = http_client.clone();
                    let url = engine.url(&query, country);
                    let country = country.clone();
                    searches.push(cx.background_spawn(async move {
                        let result = search(http_client, url).await;
                        (engine, country, result)
                    }));
                }
            }
            let mut by_url: Vec<(Found, Vec<String>)> = Vec::new();
            let mut failures = Vec::new();
            for search in searches {
                let (engine, country, result) = search.await;
                match result {
                    Ok(html) => {
                        for found in parse_results(&html, engine.host()) {
                            let tag = format!("{} {}", engine.name(), country);
                            match by_url.iter_mut().find(|(existing, _)| existing.url == found.url) {
                                Some((_, tags)) => tags.push(tag),
                                None => by_url.push((found, vec![tag])),
                            }
                        }
                    }
                    Err(error) => failures.push(format!("{} {country}: {error:#}", engine.name())),
                }
            }
            if by_url.is_empty() {
                return Err(format!(
                    "no engine returned results for \"{query}\"{}",
                    if failures.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", failures.join("; "))
                    }
                ));
            }
            by_url.sort_by(|left, right| right.1.len().cmp(&left.1.len()));
            by_url.truncate(MAX_RESULTS);
            let mut sites: Vec<(String, Vec<(Found, Vec<String>)>)> = Vec::new();
            for (found, tags) in by_url {
                let site = site_of(&found.url);
                match sites.iter_mut().find(|(name, _)| *name == site) {
                    Some((_, list)) => list.push((found, tags)),
                    None => sites.push((site, vec![(found, tags)])),
                }
            }
            let mut out = format!(
                "\"{query}\" as the public sees it, from {} ({} sites):\n",
                countries.join(", "),
                sites.len()
            );
            for (site, results) in sites {
                out.push_str(&format!("\n## {site}\n"));
                for (found, tags) in results {
                    out.push_str(&format!(
                        "- [{}]({}) — found by {}\n",
                        found.title,
                        found.url,
                        tags.join(", ")
                    ));
                }
            }
            if !failures.is_empty() {
                out.push_str(&format!("\nengines that didn't answer: {}\n", failures.join("; ")));
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duckduckgo_redirects_are_unwrapped_and_engine_links_dropped() {
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fabout&amp;rut=x">About <b>Example</b></a>
        <a href="https://duckduckgo.com/settings">Settings</a>
        <a href="https://news.example.com/a">A story</a>
        <a href="https://news.example.com/a">A story</a>"#;
        let found = parse_results(html, "html.duckduckgo.com");
        assert_eq!(
            found,
            vec![
                Found {
                    url: "https://example.org/about".into(),
                    title: "About Example".into()
                },
                Found {
                    url: "https://news.example.com/a".into(),
                    title: "A story".into()
                },
            ]
        );
    }

    #[test]
    fn bing_links_to_its_own_pages_are_not_results() {
        let html = r#"<a href="https://www.bing.com/images">Images</a><a href="https://go.microsoft.com/x">Privacy</a><a href="https://shop.example.de/">Shop &amp; more</a>"#;
        let found = parse_results(html, "www.bing.com");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Shop & more");
        assert_eq!(site_of(&found[0].url), "shop.example.de");
    }
}
