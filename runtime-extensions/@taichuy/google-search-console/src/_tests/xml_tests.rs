use crate::{
    model::{deterministic_account_id, format_utc_date, format_utc_datetime},
    pkce::{code_challenge_s256, sha256, validate_redirect_uri},
    sitemap_xml::{parse_sitemap_xml, validate_sitemap_fetch_url, ParsedSitemap},
};
use std::collections::BTreeSet;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn sha256_and_pkce_match_standard_vectors() {
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    // RFC 7636 Appendix B test vector
    assert_eq!(
        code_challenge_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn deterministic_ids_and_utc_dates_are_stable() {
    let a = deterministic_account_id("10823456789");
    let b = deterministic_account_id("10823456789");
    let c = deterministic_account_id("99999999999");
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(a.len(), 36);
    assert_eq!(format_utc_date(0), "1970-01-01");
    assert_eq!(format_utc_datetime(1763800000), "2025-11-22T08:26:40Z");
}

#[test]
fn redirect_uri_validation_enforces_exact_settings_route() {
    assert_eq!(
        validate_redirect_uri("https://1flowbase.taichuy.cn/settings/google-search-console").unwrap(),
        "https://1flowbase.taichuy.cn/settings/google-search-console"
    );
    assert_eq!(
        validate_redirect_uri("http://localhost:3100/settings/google-search-console").unwrap(),
        "http://localhost:3100/settings/google-search-console"
    );
    assert_eq!(
        validate_redirect_uri("http://127.0.0.1:3100/settings/google-search-console").unwrap(),
        "http://127.0.0.1:3100/settings/google-search-console"
    );
    for bad in [
        "http://1flowbase.taichuy.cn/settings/google-search-console",
        "https://1flowbase.taichuy.cn/settings/other",
        "https://1flowbase.taichuy.cn/settings/google-search-console?x=1",
        "https://1flowbase.taichuy.cn/settings/google-search-console#frag",
        "https://user:pass@1flowbase.taichuy.cn/settings/google-search-console",
    ] {
        assert!(validate_redirect_uri(bad).is_err(), "expected rejection for {bad}");
    }
}

#[test]
fn parses_urlset_and_sitemapindex_with_namespaces_cdata_and_entities() {
    let urlset = r#"<?xml version="1.0" encoding="UTF-8"?>
    <!-- generated -->
    <sm:urlset xmlns:sm="http://www.sitemaps.org/schemas/sitemap/0.9">
      <sm:url>
        <sm:loc><![CDATA[https://1flowbase.taichuy.com/docs/intro?a=1&b=2]]></sm:loc>
        <sm:lastmod>2026-11-20</sm:lastmod>
        <sm:changefreq>weekly</sm:changefreq>
        <sm:priority>0.8</sm:priority>
      </sm:url>
      <sm:url>
        <sm:loc>https://1flowbase.taichuy.com/blog?tag=ai&amp;page=2</sm:loc>
      </sm:url>
    </sm:urlset>"#;
    match parse_sitemap_xml(urlset).unwrap() {
        ParsedSitemap::Urlset(entries) => {
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].loc, "https://1flowbase.taichuy.com/docs/intro?a=1&b=2");
            assert_eq!(entries[0].lastmod.as_deref(), Some("2026-11-20"));
            assert_eq!(entries[1].loc, "https://1flowbase.taichuy.com/blog?tag=ai&page=2");
        }
        _ => panic!("expected urlset"),
    }

    let index = r#"<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
      <sitemap><loc>https://1flowbase.taichuy.com/sitemap-pages.xml</loc><lastmod>2026-11-21</lastmod></sitemap>
    </sitemapindex>"#;
    match parse_sitemap_xml(index).unwrap() {
        ParsedSitemap::SitemapIndex(entries) => {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].loc, "https://1flowbase.taichuy.com/sitemap-pages.xml");
        }
        _ => panic!("expected sitemapindex"),
    }
    assert!(parse_sitemap_xml("<html>not a sitemap</html>").is_err());
}

#[test]
fn sitemap_ssrf_validation_blocks_internal_hosts_and_unrelated_domains() {
    let empty = BTreeSet::new();
    assert!(validate_sitemap_fetch_url(
        "https://1flowbase.taichuy.com/sitemap.xml",
        "https://1flowbase.taichuy.com/",
        &empty
    ).is_ok());
    assert!(validate_sitemap_fetch_url(
        "https://docs.taichuy.com/sitemap.xml",
        "sc-domain:taichuy.com",
        &empty
    ).is_ok());
    for bad in [
        "http://127.0.0.1/sitemap.xml",
        "http://169.254.169.254/latest/meta-data",
        "https://localhost/sitemap.xml",
        "https://metadata.google.internal/sitemap.xml",
        "https://1flowbase.taichuy.com:8443/sitemap.xml",
        "https://evil.example/sitemap.xml",
    ] {
        assert!(
            validate_sitemap_fetch_url(bad, "https://1flowbase.taichuy.com/", &empty).is_err(),
            "expected SSRF block for {bad}"
        );
    }
}
