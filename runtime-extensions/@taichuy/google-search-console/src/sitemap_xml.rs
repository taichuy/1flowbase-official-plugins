use crate::{
    gsc::{self, encode_path_segment},
    oauth::HostContext,
    required_string, store, Failure, Payload, Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
};
use url::Url;

const MAX_XML_BYTES: usize = 8 * 1024 * 1024;
const USER_AGENT: &str = "1flowbase-google-search-console/0.1.0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SitemapEntry {
    pub loc: String,
    pub lastmod: Option<String>,
    pub changefreq: Option<String>,
    pub priority: Option<String>,
    pub source_sitemap: Option<String>,
}

impl SitemapEntry {
    pub fn to_json(&self) -> Value {
        json!({
            "loc": self.loc,
            "lastmod": self.lastmod,
            "changefreq": self.changefreq,
            "priority": self.priority,
            "source_sitemap": self.source_sitemap,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParsedSitemap {
    Urlset(Vec<SitemapEntry>),
    SitemapIndex(Vec<SitemapEntry>),
}

pub(crate) async fn fetch_entries(ctx: &HostContext, input: Payload) -> Result<Value> {
    let account_id = required_string(&input.path, "account_id")?;
    let site_url = required_string(&input.body, "site_url")?.trim();
    let feedpath = required_string(&input.body, "feedpath")?.trim();
    let follow_index = input
        .body
        .get("follow_index")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let max_entries = parse_bounded_usize(&input.body, "max_entries", 500, 1, 5000)?;
    let max_child_sitemaps = parse_bounded_usize(&input.body, "max_child_sitemaps", 5, 1, 20)?;

    let account = store::get_account(&ctx.emitter, account_id).await?;
    ensure_verified_site(ctx, &account, site_url).await?;
    let registered_sitemaps = fetch_registered_sitemaps(ctx, &account, site_url).await?;

    validate_sitemap_fetch_url(feedpath, site_url, &registered_sitemaps)?;
    let root_xml = fetch_xml_text(ctx, feedpath).await?;
    let parsed = parse_sitemap_xml(&root_xml)?;

    match parsed {
        ParsedSitemap::Urlset(mut entries) => {
            let total_found = entries.len();
            let truncated = total_found > max_entries;
            entries.truncate(max_entries);
            for entry in &mut entries {
                entry.source_sitemap = Some(feedpath.to_owned());
            }
            Ok(json!({
                "kind": "urlset",
                "site_url": site_url,
                "feedpath": feedpath,
                "total_found": total_found,
                "returned_count": entries.len(),
                "truncated": truncated,
                "child_sitemaps": [],
                "entries": entries.iter().map(SitemapEntry::to_json).collect::<Vec<_>>(),
            }))
        }
        ParsedSitemap::SitemapIndex(children) => {
            let child_urls: Vec<String> = children.iter().map(|c| c.loc.clone()).collect();
            if !follow_index {
                let total_found = children.len();
                let truncated = total_found > max_entries;
                let returned: Vec<Value> = children
                    .into_iter()
                    .take(max_entries)
                    .map(|mut e| {
                        e.source_sitemap = Some(feedpath.to_owned());
                        e.to_json()
                    })
                    .collect();
                return Ok(json!({
                    "kind": "sitemapindex",
                    "site_url": site_url,
                    "feedpath": feedpath,
                    "total_found": total_found,
                    "returned_count": returned.len(),
                    "truncated": truncated,
                    "child_sitemaps": child_urls,
                    "entries": returned,
                }));
            }

            let mut collected = Vec::new();
            let mut total_found = 0usize;
            let mut truncated = children.len() > max_child_sitemaps;
            for child in children.into_iter().take(max_child_sitemaps) {
                if validate_sitemap_fetch_url(&child.loc, site_url, &registered_sitemaps).is_err() {
                    continue;
                }
                let child_xml = fetch_xml_text(ctx, &child.loc).await?;
                if let Ok(ParsedSitemap::Urlset(mut sub_entries)) = parse_sitemap_xml(&child_xml) {
                    total_found += sub_entries.len();
                    for mut entry in sub_entries.drain(..) {
                        if collected.len() >= max_entries {
                            truncated = true;
                            break;
                        }
                        entry.source_sitemap = Some(child.loc.clone());
                        collected.push(entry);
                    }
                }
                if collected.len() >= max_entries {
                    truncated = true;
                    break;
                }
            }
            Ok(json!({
                "kind": "sitemapindex",
                "site_url": site_url,
                "feedpath": feedpath,
                "total_found": total_found,
                "returned_count": collected.len(),
                "truncated": truncated,
                "child_sitemaps": child_urls,
                "entries": collected.iter().map(SitemapEntry::to_json).collect::<Vec<_>>(),
            }))
        }
    }
}

fn parse_bounded_usize(
    body: &Value,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => {
            let n = v.as_u64().ok_or(Failure("invalid_input"))?;
            let n = usize::try_from(n).map_err(|_| Failure("invalid_input"))?;
            if !(min..=max).contains(&n) {
                return Err(Failure("invalid_input"));
            }
            Ok(n)
        }
    }
}

async fn ensure_verified_site(
    ctx: &HostContext,
    account: &crate::model::Account,
    site_url: &str,
) -> Result<()> {
    if store::find_site_for_account(&ctx.emitter, &account.account_id, site_url)
        .await?
        .is_some()
    {
        return Ok(());
    }
    let url = format!(
        "https://www.googleapis.com/webmasters/v3/sites/{}",
        encode_path_segment(site_url)
    );
    let _ = gsc::gsc_request(ctx, account, "GET", &url, None).await?;
    Ok(())
}

async fn fetch_registered_sitemaps(
    ctx: &HostContext,
    account: &crate::model::Account,
    site_url: &str,
) -> Result<BTreeSet<String>> {
    let url = format!(
        "https://www.googleapis.com/webmasters/v3/sites/{}/sitemaps",
        encode_path_segment(site_url)
    );
    let data = gsc::gsc_request(ctx, account, "GET", &url, None).await?;
    Ok(data
        .get("sitemap")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("path").and_then(Value::as_str))
                .map(|s| s.trim().to_owned())
                .collect()
        })
        .unwrap_or_default())
}

pub(crate) fn validate_sitemap_fetch_url(
    feedpath: &str,
    site_url: &str,
    registered_sitemaps: &BTreeSet<String>,
) -> Result<()> {
    let parsed = Url::parse(feedpath).map_err(|_| Failure("sitemap_url_forbidden"))?;
    if !matches!(parsed.scheme(), "https" | "http")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || parsed.port().is_some()
    {
        return Err(Failure("sitemap_url_forbidden"));
    }
    let host = parsed
        .host_str()
        .map(|h| h.to_ascii_lowercase())
        .ok_or(Failure("sitemap_url_forbidden"))?;
    if is_disallowed_host(&host) {
        return Err(Failure("sitemap_url_forbidden"));
    }
    if registered_sitemaps.contains(feedpath) {
        return Ok(());
    }
    if let Some(domain) = site_url.strip_prefix("sc-domain:") {
        let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
        if domain.is_empty() || is_disallowed_host(&domain) {
            return Err(Failure("sitemap_url_forbidden"));
        }
        if host == domain || host.ends_with(&format!(".{domain}")) {
            return Ok(());
        }
        return Err(Failure("sitemap_url_forbidden"));
    }
    let property = Url::parse(site_url).map_err(|_| Failure("sitemap_url_forbidden"))?;
    let prop_host = property
        .host_str()
        .map(|h| h.to_ascii_lowercase())
        .ok_or(Failure("sitemap_url_forbidden"))?;
    if parsed.scheme() == property.scheme()
        && host == prop_host
        && property.port() == parsed.port()
        && parsed.path().starts_with(property.path())
    {
        return Ok(());
    }
    Err(Failure("sitemap_url_forbidden"))
}

fn is_disallowed_host(host: &str) -> bool {
    let h = host.trim_matches(|c| c == '[' || c == ']');
    h.is_empty()
        || h.parse::<IpAddr>().is_ok()
        || h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h.ends_with('.')
        || !h.contains('.')
}

async fn fetch_xml_text(ctx: &HostContext, url: &str) -> Result<String> {
    let mut headers = BTreeMap::new();
    headers.insert("user-agent".to_owned(), USER_AGENT.to_owned());
    headers.insert(
        "accept".to_owned(),
        "application/xml, text/xml;q=0.9, */*;q=0.1".to_owned(),
    );
    headers.insert("accept-encoding".to_owned(), "identity".to_owned());
    let resp = ctx.http_request("GET", url.to_owned(), headers, None).await?;
    if !(200..300).contains(&resp.status) {
        return Err(Failure("sitemap_fetch_failed"));
    }
    let bytes = STANDARD
        .decode(&resp.body_base64)
        .map_err(|_| Failure("sitemap_fetch_failed"))?;
    if bytes.len() > MAX_XML_BYTES {
        return Err(Failure("sitemap_too_large"));
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return Err(Failure("sitemap_gzip_unsupported"));
    }
    String::from_utf8(bytes).map_err(|_| Failure("sitemap_invalid_xml"))
}

pub(crate) fn parse_sitemap_xml(xml: &str) -> Result<ParsedSitemap> {
    let cleaned = strip_comments_and_pis(xml);
    if has_tag(&cleaned, "urlset") {
        let blocks = extract_blocks(&cleaned, "url");
        let mut entries = Vec::with_capacity(blocks.len());
        for block in blocks {
            if let Some(loc) = extract_tag_text(block, "loc").filter(|s| !s.is_empty()) {
                entries.push(SitemapEntry {
                    loc,
                    lastmod: extract_tag_text(block, "lastmod").filter(|s| !s.is_empty()),
                    changefreq: extract_tag_text(block, "changefreq").filter(|s| !s.is_empty()),
                    priority: extract_tag_text(block, "priority").filter(|s| !s.is_empty()),
                    source_sitemap: None,
                });
            }
        }
        return Ok(ParsedSitemap::Urlset(entries));
    }
    if has_tag(&cleaned, "sitemapindex") {
        let blocks = extract_blocks(&cleaned, "sitemap");
        let mut entries = Vec::with_capacity(blocks.len());
        for block in blocks {
            if let Some(loc) = extract_tag_text(block, "loc").filter(|s| !s.is_empty()) {
                entries.push(SitemapEntry {
                    loc,
                    lastmod: extract_tag_text(block, "lastmod").filter(|s| !s.is_empty()),
                    changefreq: None,
                    priority: None,
                    source_sitemap: None,
                });
            }
        }
        return Ok(ParsedSitemap::SitemapIndex(entries));
    }
    Err(Failure("sitemap_invalid_xml"))
}

fn strip_comments_and_pis(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while !rest.is_empty() {
        if let Some(pos) = rest.find("<!--") {
            out.push_str(&strip_pis(&rest[..pos]));
            let after_start = &rest[pos + 4..];
            if let Some(end) = after_start.find("-->") {
                rest = &after_start[end + 3..];
            } else {
                break;
            }
        } else {
            out.push_str(&strip_pis(rest));
            break;
        }
    }
    out
}

fn strip_pis(mut input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    while let Some(pos) = input.find("<?") {
        out.push_str(&input[..pos]);
        let after = &input[pos + 2..];
        if let Some(end) = after.find("?>") {
            input = &after[end + 2..];
        } else {
            return out;
        }
    }
    out.push_str(input);
    out
}

fn local_tag_matches(raw_tag: &str, expected: &str) -> bool {
    let local = raw_tag
        .rsplit_once(':')
        .map_or(raw_tag, |(_, tail)| tail);
    local.eq_ignore_ascii_case(expected)
}

fn find_open_tag(xml: &str, tag: &str, from: usize) -> Option<(usize, usize)> {
    let mut cursor = from;
    while let Some(rel_lt) = xml[cursor..].find('<') {
        let lt = cursor + rel_lt;
        let after = &xml[lt + 1..];
        if after.starts_with('/') || after.starts_with('!') || after.starts_with('?') {
            cursor = lt + 1;
            continue;
        }
        let Some(rel_gt) = after.find('>') else {
            return None;
        };
        let gt = lt + 1 + rel_gt;
        let inside = xml[lt + 1..gt].trim_end();
        let self_closing = inside.ends_with('/');
        let name = inside
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        if !self_closing && local_tag_matches(name, tag) {
            return Some((lt, gt + 1));
        }
        cursor = gt + 1;
    }
    None
}

fn find_close_tag(xml: &str, tag: &str, from: usize) -> Option<(usize, usize)> {
    let mut cursor = from;
    while let Some(rel_close) = xml[cursor..].find("</") {
        let start = cursor + rel_close;
        let after = &xml[start + 2..];
        let Some(rel_gt) = after.find('>') else {
            return None;
        };
        let gt = start + 2 + rel_gt;
        let name = xml[start + 2..gt].trim();
        if local_tag_matches(name, tag) {
            return Some((start, gt + 1));
        }
        cursor = gt + 1;
    }
    None
}

fn has_tag(xml: &str, tag: &str) -> bool {
    find_open_tag(xml, tag, 0).is_some()
}

fn extract_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let mut blocks = Vec::new();
    let mut cursor = 0usize;
    while let Some((_, body_start)) = find_open_tag(xml, tag, cursor) {
        let Some((body_end, next_cursor)) = find_close_tag(xml, tag, body_start) else {
            break;
        };
        blocks.push(&xml[body_start..body_end]);
        cursor = next_cursor;
    }
    blocks
}

fn extract_tag_text(block: &str, tag: &str) -> Option<String> {
    let (_, body_start) = find_open_tag(block, tag, 0)?;
    let (body_end, _) = find_close_tag(block, tag, body_start)?;
    let raw = block[body_start..body_end].trim();
    let unwrapped = if let Some(inner) = raw
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
    {
        inner.trim().to_owned()
    } else {
        decode_xml_entities(raw).trim().to_owned()
    };
    Some(unwrapped)
}

fn decode_xml_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';') {
            let entity = &after[..semi];
            let decoded = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                }
                _ if entity.starts_with('#') => {
                    entity[1..].parse::<u32>().ok().and_then(char::from_u32)
                }
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push(ch);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}
