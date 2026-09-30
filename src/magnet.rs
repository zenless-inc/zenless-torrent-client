//! Lightweight magnet-link parsing and building.
//!
//! librqbit does the real work; this module exists so the UI and the local API
//! can validate links, show a name / info hash before metadata arrives and
//! build "Copy magnet link" URLs.

use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MagnetInfo {
    /// 40 lower-case hex characters.
    pub info_hash: String,
    pub name: Option<String>,
    pub trackers: Vec<String>,
}

/// True for anything that looks like a magnet link or a bare info hash.
pub fn looks_like_magnet(s: &str) -> bool {
    let s = s.trim();
    s.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("magnet:")) || is_hex_hash(s)
}

fn is_hex_hash(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parses `magnet:?xt=urn:btih:…&dn=…&tr=…` (hex or base32 info hash) or a
/// bare 40-character hex info hash.
pub fn parse(input: &str) -> Result<MagnetInfo, String> {
    let s = input.trim();
    if is_hex_hash(s) {
        return Ok(MagnetInfo {
            info_hash: s.to_ascii_lowercase(),
            name: None,
            trackers: Vec::new(),
        });
    }
    let query = s
        .get(..7)
        .filter(|p| p.eq_ignore_ascii_case("magnet:"))
        .map(|_| &s[7..])
        .ok_or_else(|| "not a magnet link".to_owned())?;
    let query = query.strip_prefix('?').unwrap_or(query);

    let mut info_hash = None;
    let mut name = None;
    let mut trackers = Vec::new();
    let mut saw_v2 = false;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode_component(value);
        match key.to_ascii_lowercase().as_str() {
            "xt" => {
                let lower = value.to_ascii_lowercase();
                if let Some(hash) = lower.strip_prefix("urn:btih:") {
                    info_hash = Some(normalize_hash(&value[value.len() - hash.len()..])?);
                } else if lower.starts_with("urn:btmh:") {
                    saw_v2 = true;
                }
            }
            "dn" if !value.trim().is_empty() => name = Some(value.trim().to_owned()),
            "tr" if !value.is_empty() => {
                if !trackers.contains(&value) {
                    trackers.push(value)
                }
            }
            _ => {}
        }
    }
    let info_hash = info_hash.ok_or_else(|| {
        if saw_v2 {
            "BitTorrent v2-only magnet links are not supported yet".to_owned()
        } else {
            "the magnet link has no info hash (xt=urn:btih:…)".to_owned()
        }
    })?;
    Ok(MagnetInfo {
        info_hash,
        name,
        trackers,
    })
}

/// Percent-decodes a query component, treating `+` as a space.
fn decode_component(v: &str) -> String {
    let v = v.replace('+', " ");
    percent_decode_str(&v).decode_utf8_lossy().into_owned()
}

/// Hex (40) or base32 (32) info hash → 40 lower-case hex characters.
fn normalize_hash(h: &str) -> Result<String, String> {
    let h = h.trim();
    if is_hex_hash(h) {
        return Ok(h.to_ascii_lowercase());
    }
    if h.len() == 32 {
        if let Some(bytes) = base32_decode(h) {
            return Ok(bytes.iter().map(|b| format!("{b:02x}")).collect());
        }
    }
    Err(format!("invalid info hash {h:?}"))
}

/// RFC 4648 base32 (no padding) → bytes.
fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let mut buffer: u64 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c.to_ascii_uppercase() {
            c @ b'A'..=b'Z' => c - b'A',
            c @ b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        };
        buffer = (buffer << 5) | v as u64;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Characters escaped in magnet query values.
const QUERY: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b']')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'|');

/// Builds a magnet link.
pub fn build(info_hash: &str, name: Option<&str>, trackers: &[String]) -> String {
    let mut s = format!("magnet:?xt=urn:btih:{}", info_hash.to_ascii_lowercase());
    if let Some(name) = name.filter(|n| !n.is_empty()) {
        s.push_str("&dn=");
        s.extend(utf8_percent_encode(name, QUERY));
    }
    for tr in trackers {
        s.push_str("&tr=");
        s.extend(utf8_percent_encode(tr, QUERY));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const BBB: &str = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny&tr=udp%3A%2F%2Fexplodie.org%3A6969&tr=wss%3A%2F%2Ftracker.btorrent.xyz";

    #[test]
    fn parses_hex_magnet() {
        let m = parse(BBB).unwrap();
        assert_eq!(m.info_hash, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
        assert_eq!(m.name.as_deref(), Some("Big Buck Bunny"));
        assert_eq!(
            m.trackers,
            vec!["udp://explodie.org:6969", "wss://tracker.btorrent.xyz"]
        );
    }

    #[test]
    fn parses_base32_and_case() {
        // base32 of dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c
        let m = parse("MAGNET:?xt=urn:btih:3WBFL3G4PSSV7MF37AJSHWDQMLNR63I4").unwrap();
        assert_eq!(m.info_hash, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
        assert_eq!(m.name, None);
    }

    #[test]
    fn parses_bare_hash() {
        let m = parse("DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C").unwrap();
        assert_eq!(m.info_hash, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
        assert!(looks_like_magnet("DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C"));
    }

    #[test]
    fn rejects_bad_links() {
        assert!(parse("https://example.com").is_err());
        assert!(parse("magnet:?dn=nothing").is_err());
        assert!(parse("magnet:?xt=urn:btih:1234").is_err());
        assert!(
            parse("magnet:?xt=urn:btmh:1220abcd")
                .unwrap_err()
                .contains("v2")
        );
        assert!(!looks_like_magnet("hello"));
    }

    #[test]
    fn build_roundtrip() {
        let trackers = vec!["udp://tracker.example:1337/announce".to_owned()];
        let link = build(
            "DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C",
            Some("Big Buck Bunny & friends"),
            &trackers,
        );
        assert!(link.starts_with("magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn="));
        let back = parse(&link).unwrap();
        assert_eq!(back.name.as_deref(), Some("Big Buck Bunny & friends"));
        assert_eq!(back.trackers, trackers);
    }
}
