//! Local integration API on `127.0.0.1:6813` (browser extensions, the
//! installer and single-instance argument forwarding). See the Zenless spec:
//!
//! * `GET /ping`, `GET /status`, `POST /add`, `POST /focus`, `POST /quit`
//! * Only extension origins may call it from a browser; every `POST` needs an
//!   `X-Zenless-Client` header; `/quit` and local `path`s need a missing Origin.

use crate::engine::SharedSnapshot;
use crate::model::{AddSource, Filter, Status};
use base64::Engine as _;
use eframe::egui;
use serde::Deserialize;
use serde_json::json;
use std::{
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc::Sender},
};

pub const PORT: u16 = 6813;
pub const APP_ID: &str = "zenless-torrent";
pub const APP_NAME: &str = "Zenless Torrent";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const CLIENT_NAME: &str = concat!("zenless-torrent/", env!("CARGO_PKG_VERSION"));
const MAX_BODY: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddMode {
    /// Show the "Add torrent" dialog and bring the window to the front.
    Ask,
    /// Add and start right away.
    Start,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddRequest {
    pub source: AddSource,
    pub mode: AddMode,
    /// Who asked (X-Zenless-Client or the `source` field).
    pub client: String,
}

/// Requests forwarded to the UI thread.
#[derive(Clone, Debug, PartialEq)]
pub enum ApiEvent {
    Add(AddRequest),
    Focus,
    Quit,
}

/// Shown on the "Browser integration" settings page.
#[derive(Clone, Debug, Default)]
pub struct ApiStatus {
    pub listening: bool,
    pub error: Option<String>,
    /// `(client, unix time)` of the last request from a browser extension.
    pub last_extension: Option<(String, u64)>,
    pub requests: u64,
}

pub type SharedApiStatus = Arc<Mutex<ApiStatus>>;

// ---------------------------------------------------------------------------
// Security policy (pure functions, unit tested)
// ---------------------------------------------------------------------------

/// Browser-extension origins that may call the API.
pub fn origin_allowed(origin: &str) -> bool {
    const PREFIXES: [&str; 4] = [
        "chrome-extension://",
        "moz-extension://",
        "extension://",
        "safari-web-extension://",
    ];
    let o = origin.trim().to_ascii_lowercase();
    PREFIXES
        .iter()
        .any(|p| o.starts_with(p) && o.len() > p.len())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// CORS preflight: answer 204 with the CORS headers.
    Preflight,
    /// Go on and route the request.
    Proceed,
    Reject(u16, &'static str),
}

/// Checks Origin / X-Zenless-Client before routing.
pub fn precheck(method: &str, origin: Option<&str>, client: Option<&str>) -> Verdict {
    if let Some(o) = origin
        && !origin_allowed(o)
    {
        return Verdict::Reject(403, "origin not allowed");
    }
    if method.eq_ignore_ascii_case("OPTIONS") {
        return Verdict::Preflight;
    }
    if method.eq_ignore_ascii_case("POST") && client.is_none_or(|c| c.trim().is_empty()) {
        return Verdict::Reject(400, "missing X-Zenless-Client header");
    }
    Verdict::Proceed
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AddBody {
    magnet: Option<String>,
    torrent_url: Option<String>,
    torrent_base64: Option<String>,
    name: Option<String>,
    path: Option<String>,
    cookies: Option<String>,
    referrer: Option<String>,
    user_agent: Option<String>,
    source: Option<String>,
    mode: Option<String>,
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

/// Validates a `POST /add` body. Errors are `(http status, message)`.
pub fn parse_add(body: &[u8], has_origin: bool, client: &str) -> Result<AddRequest, (u16, String)> {
    let b: AddBody =
        serde_json::from_slice(body).map_err(|e| (400, format!("invalid JSON body: {e}")))?;
    let magnet = non_empty(b.magnet);
    let url = non_empty(b.torrent_url);
    let b64 = non_empty(b.torrent_base64);
    let path = non_empty(b.path);
    let given = [magnet.is_some(), url.is_some(), b64.is_some(), path.is_some()]
        .iter()
        .filter(|x| **x)
        .count();
    if given != 1 {
        return Err((
            400,
            "exactly one of magnet, torrent_url, torrent_base64 or path is required".into(),
        ));
    }
    let mode = match b.mode.as_deref().map(str::trim) {
        None | Some("") | Some("ask") => AddMode::Ask,
        Some("start") => AddMode::Start,
        Some(other) => return Err((400, format!("unknown mode {other:?} (use ask or start)"))),
    };
    let source = if let Some(m) = magnet {
        crate::magnet::parse(&m).map_err(|e| (400, format!("invalid magnet link: {e}")))?;
        AddSource::Magnet(m)
    } else if let Some(url) = url {
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("magnet:") {
            crate::magnet::parse(&url).map_err(|e| (400, format!("invalid magnet link: {e}")))?;
            AddSource::Magnet(url)
        } else if lower.starts_with("http://") || lower.starts_with("https://") {
            AddSource::TorrentUrl {
                url,
                cookies: non_empty(b.cookies),
                referrer: non_empty(b.referrer),
                user_agent: non_empty(b.user_agent),
            }
        } else {
            return Err((400, "torrent_url must be an http(s) or magnet link".into()));
        }
    } else if let Some(b64) = b64 {
        let bytes = decode_base64(&b64).ok_or((400, "torrent_base64 is not valid base64".into()))?;
        if bytes.first() != Some(&b'd') {
            return Err((400, "torrent_base64 does not contain a .torrent file".into()));
        }
        AddSource::TorrentBytes {
            bytes: Arc::new(bytes),
            name: non_empty(b.name),
        }
    } else if let Some(path) = path {
        if has_origin {
            return Err((403, "local paths are only accepted from local programs".into()));
        }
        AddSource::Path(PathBuf::from(path))
    } else {
        unreachable!("exactly one source was checked above")
    };
    let client = non_empty(b.source).map_or_else(|| client.to_owned(), |s| format!("{client} ({s})"));
    Ok(AddRequest {
        source,
        mode,
        client,
    })
}

/// Standard or URL-safe base64, with or without padding / a `data:` prefix.
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let s = s.rsplit_once("base64,").map_or(s, |(_, rest)| rest);
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|e| e.decode(&clean).ok())
}

/// Clients that count as "browser extension contact".
fn is_extension_client(origin: Option<&str>, client: Option<&str>) -> bool {
    origin.is_some_and(origin_allowed)
        || client.is_some_and(|c| {
            let c = c.to_ascii_lowercase();
            c.contains("extension") || c.starts_with("chrome") || c.starts_with("firefox")
        })
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

type HttpResponse = tiny_http::Response<std::io::Cursor<Vec<u8>>>;

fn header(name: &str, value: &str) -> Option<tiny_http::Header> {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

fn json_response(code: u16, value: serde_json::Value) -> HttpResponse {
    let mut r = tiny_http::Response::from_data(value.to_string().into_bytes()).with_status_code(code);
    if let Some(h) = header("Content-Type", "application/json; charset=utf-8") {
        r.add_header(h);
    }
    if let Some(h) = header("Cache-Control", "no-store") {
        r.add_header(h);
    }
    r
}

fn error_response(code: u16, msg: &str) -> HttpResponse {
    json_response(code, json!({ "ok": false, "error": msg }))
}

/// Everything the request handler needs.
struct Ctx {
    snapshot: SharedSnapshot,
    tx: Sender<ApiEvent>,
    status: SharedApiStatus,
    egui: egui::Context,
}

/// Starts the API server thread. Returns its status for the settings page.
pub fn start(snapshot: SharedSnapshot, tx: Sender<ApiEvent>, egui: egui::Context) -> SharedApiStatus {
    let status: SharedApiStatus = Arc::new(Mutex::new(ApiStatus::default()));
    let ctx = Ctx {
        snapshot,
        tx,
        status: status.clone(),
        egui,
    };
    let spawned = std::thread::Builder::new()
        .name("zenless-api".into())
        .spawn(move || {
            let server = match tiny_http::Server::http(("127.0.0.1", PORT)) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("local API unavailable: {e}");
                    lock(&ctx.status).error = Some(format!("Port {PORT} is unavailable: {e}"));
                    ctx.egui.request_repaint();
                    return;
                }
            };
            lock(&ctx.status).listening = true;
            tracing::info!("local API listening on 127.0.0.1:{PORT}");
            for request in server.incoming_requests() {
                handle(request, &ctx);
            }
        });
    if let Err(e) = spawned {
        lock(&status).error = Some(format!("could not start the API thread: {e}"));
    }
    status
}

fn lock(s: &SharedApiStatus) -> std::sync::MutexGuard<'_, ApiStatus> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

fn request_header(req: &tiny_http::Request, name: &str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str().to_owned())
}

fn handle(mut req: tiny_http::Request, ctx: &Ctx) {
    let method = req.method().as_str().to_ascii_uppercase();
    let path = req.url().split(['?', '#']).next().unwrap_or("/");
    let path = match path.trim_end_matches('/') {
        "" => "/".to_owned(),
        p => p.to_owned(),
    };
    let origin = request_header(&req, "Origin");
    let client = request_header(&req, "X-Zenless-Client");
    let allowed_origin = origin.as_deref().filter(|o| origin_allowed(o)).map(str::to_owned);

    {
        let mut st = lock(&ctx.status);
        st.requests += 1;
        if is_extension_client(origin.as_deref(), client.as_deref()) {
            let who = client
                .clone()
                .or_else(|| origin.clone())
                .unwrap_or_else(|| "extension".into());
            st.last_extension = Some((who, crate::shared::kit::unix_now()));
        }
    }

    let mut response = match precheck(&method, origin.as_deref(), client.as_deref()) {
        Verdict::Reject(code, msg) => error_response(code, msg),
        Verdict::Preflight => {
            let mut r = tiny_http::Response::from_data(Vec::new()).with_status_code(204);
            for (k, v) in [
                ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
                ("Access-Control-Allow-Headers", "content-type, x-zenless-client"),
                ("Access-Control-Max-Age", "600"),
            ] {
                if let Some(h) = header(k, v) {
                    r.add_header(h);
                }
            }
            r
        }
        Verdict::Proceed => route(&mut req, &method, &path, origin.is_some(), client.as_deref(), ctx),
    };
    if let Some(o) = &allowed_origin {
        for (k, v) in [
            ("Access-Control-Allow-Origin", o.as_str()),
            ("Access-Control-Allow-Private-Network", "true"),
            ("Vary", "Origin"),
        ] {
            if let Some(h) = header(k, v) {
                response.add_header(h);
            }
        }
    }
    let _ = req.respond(response);
}

fn read_body(req: &mut tiny_http::Request) -> Result<Vec<u8>, HttpResponse> {
    if req.body_length().is_some_and(|l| l > MAX_BODY) {
        return Err(error_response(413, "body too large (max 8 MiB)"));
    }
    let mut buf = Vec::new();
    match req
        .as_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut buf)
    {
        Ok(_) if buf.len() > MAX_BODY => Err(error_response(413, "body too large (max 8 MiB)")),
        Ok(_) => Ok(buf),
        Err(e) => Err(error_response(400, &format!("could not read body: {e}"))),
    }
}

fn route(
    req: &mut tiny_http::Request,
    method: &str,
    path: &str,
    has_origin: bool,
    client: Option<&str>,
    ctx: &Ctx,
) -> HttpResponse {
    let send = |ev: ApiEvent| {
        let ok = ctx.tx.send(ev).is_ok();
        ctx.egui.request_repaint();
        ok
    };
    match (method, path) {
        ("GET", "/ping") => json_response(
            200,
            json!({ "ok": true, "app": APP_ID, "name": APP_NAME, "version": VERSION }),
        ),
        ("GET", "/status") => {
            let snap = ctx.snapshot.lock().unwrap_or_else(|e| e.into_inner()).clone();
            json_response(200, status_json(&snap))
        }
        ("POST", "/add") => {
            let body = match read_body(req) {
                Ok(b) => b,
                Err(r) => return r,
            };
            match parse_add(&body, has_origin, client.unwrap_or("unknown")) {
                Ok(add) => {
                    let info_hash = match &add.source {
                        AddSource::Magnet(m) => crate::magnet::parse(m).ok().map(|m| m.info_hash),
                        AddSource::TorrentBytes { bytes, .. } => librqbit::torrent_from_bytes(bytes)
                            .ok()
                            .map(|t| t.info_hash.as_string()),
                        _ => None,
                    };
                    let direct = add.mode == AddMode::Start;
                    if send(ApiEvent::Add(add)) {
                        let mut v = json!({ "ok": true });
                        if let (true, Some(h)) = (direct, info_hash) {
                            v["id"] = json!(h);
                        }
                        json_response(200, v)
                    } else {
                        error_response(503, "the app is shutting down")
                    }
                }
                Err((code, msg)) => error_response(code, &msg),
            }
        }
        ("POST", "/focus") => {
            send(ApiEvent::Focus);
            json_response(200, json!({ "ok": true }))
        }
        ("POST", "/quit") => {
            if has_origin {
                error_response(403, "quit is only accepted from local programs")
            } else {
                send(ApiEvent::Quit);
                json_response(200, json!({ "ok": true }))
            }
        }
        (_, "/ping" | "/status" | "/add" | "/focus" | "/quit") => {
            error_response(405, "method not allowed")
        }
        _ => error_response(404, "not found"),
    }
}

/// `GET /status` body. `items` lists up to 8 most recently added active
/// (not finished) torrents, like the Download Manager's status.
pub fn status_json(snap: &crate::model::Snapshot) -> serde_json::Value {
    let mut active: Vec<&crate::model::TorrentView> = snap
        .torrents
        .iter()
        .filter(|t| !matches!(t.status, Status::Seeding | Status::Completed))
        .collect();
    active.sort_by_key(|t| std::cmp::Reverse(t.added_at));
    let items: Vec<serde_json::Value> = active
        .iter()
        .take(8)
        .map(|t| {
            json!({
                "id": t.key,
                "name": t.name,
                "progress": (t.total_bytes > 0).then(|| t.progress()),
                "speed": t.down_bps,
                "peers": t.peers_live,
                "status": t.status.api_name(),
            })
        })
        .collect();
    json!({
        "ok": true,
        "app": APP_ID,
        "version": VERSION,
        "downloading": snap.count_filter(Filter::Downloading),
        "seeding": snap.count(Status::Seeding),
        "paused": snap.count(Status::Paused),
        "total": snap.torrents.len(),
        "download_speed": snap.down_bps,
        "upload_speed": snap.up_bps,
        "items": items,
    })
}

// ---------------------------------------------------------------------------
// Client side: single-instance forwarding
// ---------------------------------------------------------------------------

pub mod client {
    use super::{APP_ID, CLIENT_NAME, PORT};
    use crate::cli::CliItem;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::{Ipv4Addr, SocketAddr, TcpStream},
        time::Duration,
    };

    /// Minimal blocking HTTP/1.1 request to our own port. Returns `(status, body)`.
    pub fn call(method: &str, path: &str, body: Option<&str>, timeout: Duration) -> std::io::Result<(u16, String)> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, PORT));
        let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_read_timeout(Some(timeout.max(Duration::from_secs(2))))?;
        stream.set_write_timeout(Some(timeout.max(Duration::from_secs(2))))?;
        let body = body.unwrap_or("");
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nX-Zenless-Client: {CLIENT_NAME}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes())?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        let text = String::from_utf8_lossy(&raw);
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| std::io::Error::other("malformed HTTP response"))?;
        let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_owned()).unwrap_or_default();
        Ok((status, body))
    }

    /// Is another Zenless Torrent already running?
    pub fn ping() -> bool {
        match call("GET", "/ping", None, Duration::from_millis(300)) {
            Ok((200, body)) => serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .is_some_and(|v| v["app"] == APP_ID),
            _ => false,
        }
    }

    /// Sends CLI items to the running instance and focuses it.
    pub fn forward(items: &[CliItem]) -> bool {
        let mut ok = true;
        for item in items {
            let body = match item {
                CliItem::Magnet(m) => json!({ "magnet": m, "source": "cli", "mode": "ask" }),
                CliItem::File(p) => {
                    let abs = std::path::absolute(p).unwrap_or_else(|_| p.clone());
                    json!({ "path": abs.to_string_lossy(), "source": "cli", "mode": "ask" })
                }
            };
            ok &= matches!(
                call("POST", "/add", Some(&body.to_string()), Duration::from_secs(2)),
                Ok((200, _))
            );
        }
        let _ = call("POST", "/focus", Some("{}"), Duration::from_secs(1));
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        assert!(origin_allowed("chrome-extension://abcdefghijklmnop"));
        assert!(origin_allowed("moz-extension://1234-5678"));
        assert!(origin_allowed("extension://edge-id"));
        assert!(origin_allowed("safari-web-extension://xyz"));
        assert!(origin_allowed("Chrome-Extension://ABC"));
        assert!(!origin_allowed("https://evil.example"));
        assert!(!origin_allowed("http://127.0.0.1:6813"));
        assert!(!origin_allowed("null"));
        assert!(!origin_allowed("chrome-extension://"));
        assert!(!origin_allowed("https://chrome-extension.evil.example"));
    }

    #[test]
    fn prechecks() {
        assert_eq!(
            precheck("GET", Some("https://evil.example"), None),
            Verdict::Reject(403, "origin not allowed")
        );
        assert_eq!(
            precheck("OPTIONS", Some("https://evil.example"), None),
            Verdict::Reject(403, "origin not allowed")
        );
        assert_eq!(precheck("OPTIONS", Some("chrome-extension://abc"), None), Verdict::Preflight);
        assert_eq!(precheck("GET", None, None), Verdict::Proceed);
        assert_eq!(
            precheck("POST", None, None),
            Verdict::Reject(400, "missing X-Zenless-Client header")
        );
        assert_eq!(
            precheck("POST", Some("moz-extension://x"), Some("  ")),
            Verdict::Reject(400, "missing X-Zenless-Client header")
        );
        assert_eq!(
            precheck("POST", Some("moz-extension://x"), Some("firefox-extension/0.1.0")),
            Verdict::Proceed
        );
    }

    const MAGNET: &str = "magnet:?xt=urn:btih:dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c&dn=Big+Buck+Bunny";

    #[test]
    fn add_magnet() {
        let body = format!(r#"{{"magnet":"{MAGNET}","mode":"start","source":"chrome"}}"#);
        let r = parse_add(body.as_bytes(), true, "chrome-extension/0.1.0").unwrap();
        assert_eq!(r.mode, AddMode::Start);
        assert_eq!(r.source, AddSource::Magnet(MAGNET.into()));
        assert_eq!(r.client, "chrome-extension/0.1.0 (chrome)");
        let ask = parse_add(format!(r#"{{"magnet":"{MAGNET}"}}"#).as_bytes(), false, "cli").unwrap();
        assert_eq!(ask.mode, AddMode::Ask);
    }

    #[test]
    fn add_rejections() {
        let err = |body: &str, origin: bool| parse_add(body.as_bytes(), origin, "t").unwrap_err().0;
        assert_eq!(err("not json", false), 400);
        assert_eq!(err("{}", false), 400);
        assert_eq!(
            err(&format!(r#"{{"magnet":"{MAGNET}","torrent_url":"https://x/y.torrent"}}"#), false),
            400
        );
        assert_eq!(err(r#"{"magnet":"magnet:?dn=nohash"}"#, false), 400);
        assert_eq!(err(r#"{"torrent_url":"ftp://x/y.torrent"}"#, false), 400);
        assert_eq!(err(&format!(r#"{{"magnet":"{MAGNET}","mode":"later"}}"#), false), 400);
        assert_eq!(err(r#"{"path":"C:\\x.torrent"}"#, true), 403);
        assert_eq!(err(r#"{"torrent_base64":"!!!"}"#, false), 400);
        // valid base64 but not bencode
        assert_eq!(err(r#"{"torrent_base64":"aGVsbG8="}"#, false), 400);
    }

    #[test]
    fn add_other_sources() {
        let r = parse_add(
            br#"{"torrent_url":"https://example.org/debian.torrent","cookies":"a=b","referrer":"https://example.org/","user_agent":"UA"}"#,
            true,
            "firefox-extension/0.1.0",
        )
        .unwrap();
        assert_eq!(
            r.source,
            AddSource::TorrentUrl {
                url: "https://example.org/debian.torrent".into(),
                cookies: Some("a=b".into()),
                referrer: Some("https://example.org/".into()),
                user_agent: Some("UA".into()),
            }
        );
        // "d4:infoe" = minimal bencoded dict
        let r = parse_add(br#"{"torrent_base64":"ZDQ6aW5mb2U=","name":"x.torrent"}"#, true, "c").unwrap();
        assert!(matches!(r.source, AddSource::TorrentBytes { ref bytes, ref name }
            if bytes.as_slice() == b"d4:infoe" && name.as_deref() == Some("x.torrent")));
        let r = parse_add(br#"{"path":"C:\\x.torrent"}"#, false, "cli").unwrap();
        assert_eq!(r.source, AddSource::Path(PathBuf::from(r"C:\x.torrent")));
        // magnet given as torrent_url is accepted
        let r = parse_add(format!(r#"{{"torrent_url":"{MAGNET}"}}"#).as_bytes(), true, "c").unwrap();
        assert!(r.source.is_magnet());
    }

    #[test]
    fn base64_variants() {
        assert_eq!(decode_base64("ZDQ6aW5mb2U").unwrap(), b"d4:infoe");
        assert_eq!(
            decode_base64("data:application/x-bittorrent;base64,ZDQ6aW5mb2U=").unwrap(),
            b"d4:infoe"
        );
        assert_eq!(decode_base64("ZDQ6\naW5mb2U=").unwrap(), b"d4:infoe");
    }

    #[test]
    fn status_body() {
        use crate::model::{Snapshot, TorrentView};
        let mut s = Snapshot::default();
        let mut t = TorrentView::placeholder("h1", "one", Status::Downloading);
        t.total_bytes = 100;
        t.have_bytes = 25;
        t.added_at = 2;
        t.down_bps = 1000.0;
        s.torrents.push(t);
        s.torrents.push(TorrentView::placeholder("h2", "two", Status::Seeding));
        s.torrents.push(TorrentView::placeholder("pending-1", "three", Status::Metadata));
        s.down_bps = 1000.0;
        let v = status_json(&s);
        assert_eq!(v["ok"], true);
        assert_eq!(v["app"], "zenless-torrent");
        assert_eq!(v["downloading"], 2);
        assert_eq!(v["seeding"], 1);
        assert_eq!(v["download_speed"], 1000.0);
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["id"], "h1");
        assert_eq!(items[0]["progress"], 0.25);
        assert_eq!(items[0]["status"], "downloading");
        assert!(items[1]["progress"].is_null());
        assert_eq!(items[1]["status"], "metadata");
    }

    #[test]
    fn extension_contact_detection() {
        assert!(is_extension_client(Some("chrome-extension://abc"), None));
        assert!(is_extension_client(None, Some("firefox-extension/0.1.0")));
        assert!(!is_extension_client(None, Some("zenless-dm/0.1.0")));
        assert!(!is_extension_client(None, None));
    }
}
