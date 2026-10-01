//! Command-line parsing: `zenless-torrent.exe [--minimized] [magnet:?… | file.torrent …]`.
//!
//! After a self-update the previous version starts the new one with
//! `--updated-from <old version>` (see `shared::updater`).

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliItem {
    Magnet(String),
    File(PathBuf),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliArgs {
    pub minimized: bool,
    pub items: Vec<CliItem>,
    /// `--updated-from <version>`: started by the previous version after an update.
    pub updated_from: Option<String>,
}

/// Parses the arguments *after* the program name. Unknown flags are ignored so
/// that shell integrations passing extra switches never break the app.
pub fn parse<I, S>(args: I) -> CliArgs
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = CliArgs::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg.as_ref().trim().trim_matches('"');
        if arg.is_empty() {
            continue;
        }
        let lower = arg.to_ascii_lowercase();
        if matches!(lower.as_str(), "--minimized" | "-m" | "/minimized" | "--hidden") {
            out.minimized = true;
        } else if lower == "--updated-from" {
            // Consumes the version, so it is never mistaken for a .torrent path.
            out.updated_from = args
                .next()
                .map(|v| v.as_ref().trim().trim_matches('"').to_owned())
                .filter(|v| !v.is_empty());
        } else if lower.starts_with("magnet:") {
            out.items.push(CliItem::Magnet(arg.to_owned()));
        } else if arg.len() == 40 && arg.bytes().all(|b| b.is_ascii_hexdigit()) {
            out.items
                .push(CliItem::Magnet(format!("magnet:?xt=urn:btih:{}", arg.to_ascii_lowercase())));
        } else if lower.starts_with("--") || (lower.starts_with('-') && lower.len() <= 3) {
            // unknown flag
        } else {
            out.items.push(CliItem::File(PathBuf::from(arg)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        assert_eq!(parse(Vec::<String>::new()), CliArgs::default());
    }

    #[test]
    fn minimized_and_items() {
        let a = parse([
            "--minimized",
            "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
            r"C:\Users\me\Downloads\debian.torrent",
            "--unknown-flag",
        ]);
        assert!(a.minimized);
        assert_eq!(a.items.len(), 2);
        assert!(matches!(&a.items[0], CliItem::Magnet(m) if m.starts_with("magnet:?xt=")));
        assert_eq!(
            a.items[1],
            CliItem::File(PathBuf::from(r"C:\Users\me\Downloads\debian.torrent"))
        );
    }

    #[test]
    fn bare_hash_and_quotes() {
        let a = parse(["\"DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C\"", "  "]);
        assert!(!a.minimized);
        assert_eq!(
            a.items,
            vec![CliItem::Magnet(
                "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c".into()
            )]
        );
    }

    #[test]
    fn updated_from_takes_the_version() {
        let a = parse(["--updated-from", "0.2.0", "--minimized"]);
        assert_eq!(a.updated_from.as_deref(), Some("0.2.0"));
        assert!(a.minimized);
        assert!(a.items.is_empty(), "the version is not a .torrent path");
        let a = parse(["--updated-from"]);
        assert_eq!(a.updated_from, None);
        assert_eq!(parse(["x.torrent"]).updated_from, None);
    }

    #[test]
    fn magnet_scheme_case_insensitive() {
        let a = parse(["MAGNET:?xt=urn:btih:abc", "-m"]);
        assert!(a.minimized);
        assert!(matches!(&a.items[0], CliItem::Magnet(_)));
    }
}
