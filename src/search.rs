//! Torrent search across public indexer APIs.
//!
//! [`search`] queries the configured providers, tolerates individual
//! failures, and returns merged hits sorted by seeders. Each hit carries a
//! ready-to-add magnet link built from the indexer's info hash.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Per-request timeout for indexer queries.
const TIMEOUT: Duration = Duration::from_secs(10);

const APIBAY: &str = "apibay";

/// Open trackers appended to built magnets so peers are found quickly even
/// without DHT.
const TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
];

/// One search hit, ready to hand to the engine as a magnet link.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Display title from the indexer.
    pub title: String,
    /// Content size in bytes (0 when unknown).
    pub size: u64,
    pub seeders: u64,
    pub leechers: u64,
    /// Short name of the indexer this hit came from.
    pub source: &'static str,
    /// Magnet link built from the info hash, title, and open trackers.
    pub magnet: String,
}

/// Outcome of one search: merged hits plus any providers that failed.
#[derive(Debug)]
pub struct SearchOutcome {
    /// The query this outcome answers, so stale responses can be dropped.
    pub query: String,
    /// Hits from all providers, sorted by seeders descending.
    pub results: Vec<SearchResult>,
    /// Names of providers whose request or parse failed.
    pub failed: Vec<&'static str>,
}

/// Query all providers and merge their hits.
pub async fn search(query: String) -> SearchOutcome {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("kist/", env!("CARGO_PKG_VERSION")))
        .build();
    let client = match client {
        Ok(c) => c,
        Err(_) => {
            return SearchOutcome {
                query,
                results: Vec::new(),
                failed: vec![APIBAY],
            };
        }
    };

    let mut results = Vec::new();
    let mut failed = Vec::new();
    match search_apibay(&client, &query).await {
        Ok(mut hits) => results.append(&mut hits),
        Err(_) => failed.push(APIBAY),
    }
    results.sort_by_key(|r| std::cmp::Reverse(r.seeders));
    SearchOutcome {
        query,
        results,
        failed,
    }
}

/// apibay serializes numbers as JSON strings; accept either form.
fn de_u64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Num(u64),
        Text(String),
    }
    Ok(match Raw::deserialize(d)? {
        Raw::Num(n) => n,
        Raw::Text(s) => s.parse().unwrap_or(0),
    })
}

#[derive(Deserialize)]
struct ApibayHit {
    name: String,
    info_hash: String,
    #[serde(deserialize_with = "de_u64")]
    seeders: u64,
    #[serde(deserialize_with = "de_u64")]
    leechers: u64,
    #[serde(deserialize_with = "de_u64")]
    size: u64,
}

async fn search_apibay(client: &reqwest::Client, query: &str) -> Result<Vec<SearchResult>> {
    let url = format!("https://apibay.org/q.php?q={}", urlencode(query));
    let hits: Vec<ApibayHit> = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("unexpected apibay response")?;
    Ok(parse_apibay(hits))
}

fn parse_apibay(hits: Vec<ApibayHit>) -> Vec<SearchResult> {
    hits.into_iter()
        // A single all-zero info hash is apibay's "no results" marker.
        .filter(|h| h.info_hash.bytes().any(|b| b != b'0'))
        .map(|h| SearchResult {
            magnet: build_magnet(&h.info_hash, &h.name),
            title: h.name,
            size: h.size,
            seeders: h.seeders,
            leechers: h.leechers,
            source: APIBAY,
        })
        .collect()
}

/// Build a magnet link from an info hash and display name.
fn build_magnet(info_hash: &str, name: &str) -> String {
    let mut magnet = format!("magnet:?xt=urn:btih:{info_hash}&dn={}", urlencode(name));
    for tracker in TRACKERS {
        magnet.push_str("&tr=");
        magnet.push_str(&urlencode(tracker));
    }
    magnet
}

/// Percent-encode every byte outside the URL-unreserved set.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use librqbit::Magnet;

    #[tokio::test]
    #[ignore = "network probe"]
    async fn live_search_probe() {
        let outcome = search("ubuntu 24.04".to_string()).await;
        println!("failed: {:?}", outcome.failed);
        for r in outcome.results.iter().take(5) {
            println!(
                "{} | {} | s{} l{} | {}",
                r.source, r.title, r.seeders, r.leechers, r.size
            );
        }
        assert!(!outcome.results.is_empty());
    }

    #[test]
    fn urlencode_escapes_reserved_bytes() {
        assert_eq!(urlencode("abc-1.2_~"), "abc-1.2_~");
        assert_eq!(urlencode("a b&c/d"), "a%20b%26c%2Fd");
        assert_eq!(urlencode("é"), "%C3%A9");
    }

    #[test]
    fn built_magnet_parses() {
        let magnet = build_magnet(
            "cab507494d02ebb1178b38f2e9d7be299c86b862",
            "Some Name (2024)",
        );
        assert!(
            Magnet::parse(&magnet).is_ok(),
            "magnet should parse: {magnet}"
        );
    }

    #[test]
    fn apibay_no_results_marker_is_filtered() {
        let json = r#"[{"id":"0","name":"No results returned","info_hash":"0000000000000000000000000000000000000000","leechers":"0","seeders":"0","num_files":"0","size":"0"}]"#;
        let hits: Vec<ApibayHit> = serde_json::from_str(json).unwrap();
        assert!(parse_apibay(hits).is_empty());
    }

    #[test]
    fn apibay_hits_parse_from_string_numbers() {
        let json = r#"[{"id":"1","name":"ubuntu-24.04.iso","info_hash":"CAB507494D02EBB1178B38F2E9D7BE299C86B862","leechers":"3","seeders":"120","num_files":"1","size":"2168558592"}]"#;
        let hits: Vec<ApibayHit> = serde_json::from_str(json).unwrap();
        let results = parse_apibay(hits);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].seeders, 120);
        assert_eq!(results[0].size, 2_168_558_592);
        assert!(
            results[0]
                .magnet
                .starts_with("magnet:?xt=urn:btih:CAB507494D")
        );
    }
}
