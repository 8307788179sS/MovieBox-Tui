//! `moviebox-stremio` — runs MovieBox-TUI's MovieBox provider as a Stremio addon.
//!
//! It serves the Stremio addon protocol over plain HTTP:
//!   /manifest.json, /catalog/..., /meta/..., /stream/...
//! and a small stream proxy at /p/... that adds the headers MovieBox's CDN
//! expects (User-Agent, Referer, signed cookies), so streams play in any
//! Stremio player on the phone, the TV or the desktop.
//!
//! Run it in Termux (or any Linux box) and install
//! `http://127.0.0.1:7000/manifest.json` in Stremio.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures::StreamExt;
use moviebox_tui::providers::models::{
    AudioTrackOption, CatalogItem, MediaDetails, MediaType, ProviderKind, SubtitleOption,
};
use moviebox_tui::service::MovieBoxService;
use serde_json::{Value, json};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::net::{TcpListener, TcpStream};

const ADDON_ID: &str = "community.moviebox.tui";
const ID_PREFIX: &str = "mbx:";
const HOME_TAB: &str = "2";
const DEFAULT_PORT: u16 = 7000;

const PAGE_OUT: usize = 25;
const MAX_FETCHES_PER_REQUEST: usize = 6;
const MAX_LIST_KEYS: usize = 300;
const MAX_DETAIL_KEYS: usize = 500;
const LIST_TTL: Duration = Duration::from_secs(30 * 60);
const DETAILS_TTL: Duration = Duration::from_secs(30 * 60);
const STREAM_LOOKUP_TIMEOUT: Duration = Duration::from_secs(25);
const CAPTION_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_DUB_SUBJECTS: usize = 6;

const DASH_RANGE_CHUNK_BYTES: usize = 95 * 1024;
const MAX_SEGMENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 10 * 1024 * 1024;
const CHUNK_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_LINE_BYTES: u64 = 8 * 1024;
const MAX_HEADERS: usize = 64;

const CORS: &str = "Access-Control-Allow-Origin: *\r\n\
Access-Control-Allow-Headers: *\r\n\
Access-Control-Allow-Methods: GET, HEAD, OPTIONS\r\n";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct Config {
    proxy_streams: bool,
    public_url: Option<String>,
}

struct PagedList {
    items: Vec<Value>,
    seen_raw: HashSet<String>,
    next_page: usize,
    done: bool,
    created: Instant,
}

impl PagedList {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            seen_raw: HashSet::new(),
            next_page: 1,
            done: false,
            created: Instant::now(),
        }
    }
}

enum Source {
    Home,
    Search(String),
}

impl Source {
    fn key(&self) -> String {
        match self {
            Source::Home => "home".to_string(),
            Source::Search(q) => format!("search:{}", q.to_lowercase()),
        }
    }
}

struct State {
    svc: MovieBoxService,
    stream_client: reqwest::Client,
    cfg: Config,
    lists: Mutex<HashMap<String, PagedList>>,
    details: Mutex<HashMap<String, (Instant, MediaDetails)>>,
    imdb: Mutex<HashMap<String, String>>,
    /// Same keys as `imdb`: whether Cinemeta lists India as a country, so the
    /// title's "Original Audio" is most likely Hindi.
    indian: Mutex<HashMap<String, bool>>,
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn print_help() {
    println!("moviebox-stremio {}", env!("CARGO_PKG_VERSION"));
    println!("Serves MovieBox as a Stremio addon.\n");
    println!("USAGE:");
    println!(
        "    moviebox-stremio [--port 7000] [--host 0.0.0.0] [--public-url URL] [--no-proxy]\n"
    );
    println!("OPTIONS / ENVIRONMENT:");
    println!("    --port, MBX_PORT              Port to listen on (default {DEFAULT_PORT})");
    println!(
        "    --host, MBX_HOST              Address to bind (default 0.0.0.0, i.e. LAN-visible)"
    );
    println!(
        "    --public-url, MBX_PUBLIC_URL  Base URL clients use, e.g. https://x.trycloudflare.com"
    );
    println!("    --no-proxy, MBX_PROXY=0       Hand raw CDN links to Stremio (uses proxyHeaders)");
    println!("\nThen install http://127.0.0.1:<port>/manifest.json in Stremio.");
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return;
    }
    if args
        .iter()
        .any(|a| a == "-V" || a == "-v" || a == "--version")
    {
        println!("moviebox-stremio {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let opt = |flag: &str, env: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
            .or_else(|| std::env::var(env).ok())
            .filter(|s| !s.trim().is_empty())
    };

    let port: u16 = opt("--port", "MBX_PORT")
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let host = opt("--host", "MBX_HOST").unwrap_or_else(|| "0.0.0.0".to_string());
    let env_proxy_off = std::env::var("MBX_PROXY")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            )
        })
        .unwrap_or(false);
    let proxy_streams = !env_proxy_off && !args.iter().any(|a| a == "--no-proxy");
    let public_url =
        opt("--public-url", "MBX_PUBLIC_URL").map(|u| u.trim().trim_end_matches('/').to_string());

    let listener = match TcpListener::bind((host.as_str(), port)).await {
        Ok(l) => l,
        Err(err) => {
            eprintln!("error: cannot listen on {host}:{port}: {err}");
            std::process::exit(1);
        }
    };

    let stream_client = moviebox_tui::net::http_client_builder_base()
        .http1_only()
        .pool_max_idle_per_host(16)
        .build()
        .unwrap_or_default();

    let state = Arc::new(State {
        svc: MovieBoxService::new(),
        stream_client,
        cfg: Config {
            proxy_streams,
            public_url,
        },
        lists: Mutex::new(HashMap::new()),
        details: Mutex::new(HashMap::new()),
        imdb: Mutex::new(HashMap::new()),
        indian: Mutex::new(HashMap::new()),
    });

    println!(
        "MovieBox Stremio addon {} is running.",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("  Same device:   http://127.0.0.1:{port}/manifest.json");
    if let Some(ip) = lan_ip() {
        println!("  Other devices: http://{ip}:{port}/manifest.json");
    }
    if let Some(url) = &state.cfg.public_url {
        println!("  Public URL:    {url}/manifest.json");
    }
    println!();
    println!(
        "  Stream proxy: {}",
        if proxy_streams {
            "on"
        } else {
            "off (raw CDN links)"
        }
    );
    println!("  Keep this window open while you watch. Ctrl+C to stop.");

    loop {
        match listener.accept().await {
            Ok((socket, _)) => {
                let _ = socket.set_nodelay(true);
                let st = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(err) = handle_conn(socket, &st).await {
                        log_debug(&format!("connection ended: {err}"));
                    }
                });
            }
            Err(err) => {
                eprintln!("accept error: {err}");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

fn log_debug(msg: &str) {
    if std::env::var("MBX_DEBUG").is_ok_and(|v| !v.is_empty() && v != "0") {
        eprintln!("[debug] {msg}");
    }
}

fn lan_ip() -> Option<std::net::IpAddr> {
    // No packets are sent: connecting a UDP socket only picks the outbound interface.
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

// ---------------------------------------------------------------------------
// HTTP plumbing
// ---------------------------------------------------------------------------

async fn read_request<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<Option<Request>> {
    let mut line = String::new();
    let n = (&mut *reader)
        .take(MAX_LINE_BYTES)
        .read_line(&mut line)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();

    let mut headers = Vec::new();
    for _ in 0..MAX_HEADERS {
        let mut header_line = String::new();
        let n = (&mut *reader)
            .take(MAX_LINE_BYTES)
            .read_line(&mut header_line)
            .await?;
        let trimmed = header_line.trim();
        if n == 0 || trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    Ok(Some(Request {
        method,
        target,
        headers,
    }))
}

fn reason(status: u16) -> &'static str {
    reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("OK")
}

async fn send<W: AsyncWrite + Unpin>(
    w: &mut W,
    status: u16,
    content_type: &str,
    body: &[u8],
    extra: &[(&str, String)],
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{CORS}Connection: close\r\n",
        reason(status),
        body.len()
    );
    for (k, v) in extra {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    w.write_all(head.as_bytes()).await?;
    w.write_all(body).await?;
    w.flush().await
}

async fn send_json<W: AsyncWrite + Unpin>(w: &mut W, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    send(w, 200, "application/json; charset=utf-8", &body, &[]).await
}

async fn handle_conn(socket: TcpStream, st: &State) -> std::io::Result<()> {
    let (read_half, write_half) = socket.into_split();
    let mut reader = BufReader::new(read_half);
    let mut w = tokio::io::BufWriter::with_capacity(128 * 1024, write_half);

    let Some(req) = read_request(&mut reader).await? else {
        return Ok(());
    };
    log_debug(&format!("{} {}", req.method, req.target));

    if req.method.eq_ignore_ascii_case("OPTIONS") {
        return send(&mut w, 204, "text/plain", b"", &[]).await;
    }

    let base = st.base_url(&req);

    if req.target.starts_with("/p/") {
        return st.proxy(&req, &base, &mut w).await;
    }

    let path = req.target.split(['?', '#']).next().unwrap_or("/");
    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();

    match segs.as_slice() {
        [""] | ["configure"] => {
            let page = landing_page(&base);
            send(
                &mut w,
                200,
                "text/html; charset=utf-8",
                page.as_bytes(),
                &[],
            )
            .await
        }
        ["manifest.json"] => send_json(&mut w, &manifest()).await,
        ["catalog", ty, id] => {
            let v = st.catalog(ty, strip_json(id), None).await;
            send_json(&mut w, &v).await
        }
        ["catalog", ty, id, extra] => {
            let v = st.catalog(ty, id, Some(strip_json(extra))).await;
            send_json(&mut w, &v).await
        }
        ["meta", ty, id] => {
            let id = decode_segment(strip_json(id));
            let v = st.meta(ty, &id).await;
            send_json(&mut w, &v).await
        }
        ["stream", ty, id] => {
            let id = decode_segment(strip_json(id));
            let v = st.streams(ty, &id, &base).await;
            send_json(&mut w, &v).await
        }
        // Not part of Stremio: which audio dubs MovieBox has for a title, so a
        // launcher can list only the titles that really have a Hindi dub.
        ["dubs", ty, id] => {
            let id = decode_segment(strip_json(id));
            let v = st.dubs(ty, &id).await;
            send_json(&mut w, &v).await
        }
        _ => send(&mut w, 404, "text/plain", b"not found", &[]).await,
    }
}

fn strip_json(s: &str) -> &str {
    s.strip_suffix(".json").unwrap_or(s)
}

fn decode_segment(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

// ---------------------------------------------------------------------------
// Stremio protocol
// ---------------------------------------------------------------------------

fn manifest() -> Value {
    json!({
        "id": ADDON_ID,
        "version": env!("CARGO_PKG_VERSION"),
        "name": "MovieBox (TUI)",
        "description": "MovieBox movies and series via MovieBox-TUI, with dubs and subtitles. Runs on your own device.",
        "resources": [
            "catalog",
            { "name": "meta", "types": ["movie", "series"], "idPrefixes": [ID_PREFIX] },
            { "name": "stream", "types": ["movie", "series"], "idPrefixes": [ID_PREFIX, "tt"] }
        ],
        "types": ["movie", "series"],
        "idPrefixes": [ID_PREFIX, "tt"],
        "catalogs": [
            { "type": "movie", "id": "mbx_home", "name": "MovieBox Trending", "extra": [{ "name": "skip" }] },
            { "type": "series", "id": "mbx_home", "name": "MovieBox Trending", "extra": [{ "name": "skip" }] },
            { "type": "movie", "id": "mbx_search", "name": "MovieBox",
              "extra": [{ "name": "search", "isRequired": true }, { "name": "skip" }] },
            { "type": "series", "id": "mbx_search", "name": "MovieBox",
              "extra": [{ "name": "search", "isRequired": true }, { "name": "skip" }] }
        ],
        "behaviorHints": { "configurable": false, "configurationRequired": false }
    })
}

fn landing_page(base: &str) -> String {
    let manifest_url = format!("{base}/manifest.json");
    let stremio_url = manifest_url
        .strip_prefix("https://")
        .or_else(|| manifest_url.strip_prefix("http://"))
        .map(|rest| format!("stremio://{rest}"))
        .unwrap_or_else(|| manifest_url.clone());
    format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>MovieBox Stremio addon</title>
<style>body{{font-family:system-ui,sans-serif;background:#111;color:#eee;max-width:560px;margin:40px auto;padding:0 16px}}
a.btn{{display:inline-block;background:#7b5bf5;color:#fff;padding:14px 22px;border-radius:10px;text-decoration:none;font-weight:600}}
code{{background:#222;padding:8px;border-radius:6px;display:block;word-break:break-all;margin:12px 0}}</style></head>
<body><h1>MovieBox addon is running</h1>
<p><a class="btn" href="{stremio_url}">Install in Stremio</a></p>
<p>Or paste this URL into Stremio &rarr; Addons &rarr; search box:</p><code>{manifest_url}</code>
</body></html>"#
    )
}

fn parse_type(ty: &str) -> Option<MediaType> {
    match ty {
        "movie" => Some(MediaType::Movie),
        "series" => Some(MediaType::Series),
        _ => None,
    }
}

fn type_str(t: MediaType) -> &'static str {
    match t {
        MediaType::Movie => "movie",
        MediaType::Series => "series",
    }
}

fn meta_preview(item: &CatalogItem) -> Value {
    json!({
        "id": format!("{ID_PREFIX}{}", item.id.value),
        "type": type_str(item.media_type),
        "name": item.title.trim(),
        "poster": item.poster_url,
        "posterShape": "poster",
        "releaseInfo": item.year,
    })
}

fn split_list(s: &str) -> Vec<String> {
    s.split([',', '/', '|'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

fn details_to_meta(ty: &str, id: &str, d: &MediaDetails) -> Value {
    let mut meta = json!({
        "id": id,
        "type": ty,
        "name": d.title,
        "poster": d.poster_url,
        "background": d.poster_url,
        "posterShape": "poster",
        "description": d.description,
        "releaseInfo": d.year,
        "genres": d.genres,
        "imdbRating": d.imdb_rating,
        "runtime": d.duration,
    });
    if let Some(dir) = d.director.as_deref() {
        meta["director"] = json!(split_list(dir));
    }
    if let Some(stars) = d.stars.as_deref() {
        meta["cast"] = json!(split_list(stars));
    }

    if d.is_series() {
        let year = d.year.as_deref().and_then(first_year).unwrap_or(2000);
        let released = format!("{year:04}-01-01T00:00:00.000Z");
        let mut videos = Vec::new();
        for season in &d.seasons {
            for ep in &season.episodes {
                let title = ep
                    .title
                    .clone()
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| format!("Episode {}", ep.number));
                videos.push(json!({
                    "id": format!("{id}:{}:{}", season.number, ep.number),
                    "title": title,
                    "season": season.number,
                    "episode": ep.number,
                    "overview": ep.overview,
                    "released": released,
                }));
            }
        }
        meta["videos"] = json!(videos);
    } else {
        meta["behaviorHints"] = json!({ "defaultVideoId": id });
    }
    meta
}

impl State {
    fn base_url(&self, req: &Request) -> String {
        if let Some(url) = &self.cfg.public_url {
            return url.clone();
        }
        let first = |v: &str| v.split(',').next().unwrap_or("").trim().to_string();
        let host = req
            .header("x-forwarded-host")
            .or_else(|| req.header("host"))
            .map(first)
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| format!("127.0.0.1:{DEFAULT_PORT}"));
        let proto = req
            .header("x-forwarded-proto")
            .map(first)
            .filter(|p| p == "https" || p == "http")
            .unwrap_or_else(|| "http".to_string());
        format!("{proto}://{host}")
    }

    // ----- catalogs -------------------------------------------------------

    async fn catalog(&self, ty: &str, id: &str, extra: Option<&str>) -> Value {
        let empty = json!({ "metas": [] });
        let Some(media_type) = parse_type(ty) else {
            return empty;
        };
        let mut search = None;
        let mut skip = 0usize;
        if let Some(extra) = extra {
            for (k, v) in url::form_urlencoded::parse(extra.as_bytes()) {
                match &*k {
                    "search" => search = Some(v.trim().to_string()),
                    "skip" => skip = v.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        let source = match (id, search) {
            ("mbx_search", Some(q)) if !q.is_empty() => Source::Search(q),
            ("mbx_home", _) => Source::Home,
            _ => return empty,
        };
        let metas = self.paged(media_type, &source, skip).await;
        json!({ "metas": metas, "cacheMaxAge": 1800 })
    }

    async fn fetch_source_page(
        &self,
        source: &Source,
        page: usize,
    ) -> Result<Vec<CatalogItem>, String> {
        match source {
            Source::Home => self
                .svc
                .homepage(HOME_TAB, page)
                .await
                .map(|(items, _)| items),
            Source::Search(q) => self
                .svc
                .search_typed(ProviderKind::MovieBox, q, page)
                .await
                .map_err(|e| e.to_string()),
        }
    }

    async fn paged(&self, media_type: MediaType, source: &Source, skip: usize) -> Vec<Value> {
        let key = format!("{}|{}", type_str(media_type), source.key());
        for _ in 0..MAX_FETCHES_PER_REQUEST {
            let page = {
                let mut lists = lock(&self.lists);
                if lists.len() > MAX_LIST_KEYS {
                    lists.retain(|_, l| l.created.elapsed() < LIST_TTL);
                    if lists.len() > MAX_LIST_KEYS {
                        lists.clear();
                    }
                }
                let entry = lists.entry(key.clone()).or_insert_with(PagedList::new);
                if entry.created.elapsed() > LIST_TTL {
                    *entry = PagedList::new();
                }
                if entry.done || entry.items.len() >= skip + PAGE_OUT {
                    return page_slice(&entry.items, skip);
                }
                entry.next_page
            };

            let fetched = self.fetch_source_page(source, page).await;

            let mut lists = lock(&self.lists);
            let entry = lists.entry(key.clone()).or_insert_with(PagedList::new);
            match fetched {
                Ok(items) => {
                    let mut new_raw = 0usize;
                    for item in &items {
                        if !entry.seen_raw.insert(item.id.value.clone()) {
                            continue;
                        }
                        new_raw += 1;
                        if item.media_type == media_type {
                            entry.items.push(meta_preview(item));
                        }
                    }
                    if new_raw == 0 {
                        entry.done = true;
                    }
                    entry.next_page = entry.next_page.max(page + 1);
                }
                Err(err) => {
                    eprintln!("catalog page {page} failed: {err}");
                    return page_slice(&entry.items, skip);
                }
            }
        }
        let lists = lock(&self.lists);
        lists
            .get(&key)
            .map(|l| page_slice(&l.items, skip))
            .unwrap_or_default()
    }

    // ----- meta -------------------------------------------------------------

    async fn details(&self, subject_id: &str) -> Result<MediaDetails, String> {
        let cached = {
            let map = lock(&self.details);
            map.get(subject_id)
                .filter(|(at, _)| at.elapsed() < DETAILS_TTL)
                .map(|(_, d)| d.clone())
        };
        if let Some(d) = cached {
            return Ok(d);
        }
        let d = self
            .svc
            .details_typed(ProviderKind::MovieBox, subject_id)
            .await
            .map_err(|e| e.to_string())?;
        let mut map = lock(&self.details);
        if map.len() > MAX_DETAIL_KEYS {
            map.clear();
        }
        map.insert(subject_id.to_string(), (Instant::now(), d.clone()));
        Ok(d)
    }

    async fn meta(&self, ty: &str, id: &str) -> Value {
        let Some(subject) = id.strip_prefix(ID_PREFIX) else {
            return json!({ "meta": null });
        };
        let subject = subject.split(':').next().unwrap_or(subject);
        match self.details(subject).await {
            Ok(d) => json!({
                "meta": details_to_meta(ty, &format!("{ID_PREFIX}{subject}"), &d),
                "cacheMaxAge": 3600
            }),
            Err(err) => {
                eprintln!("meta {id} failed: {err}");
                json!({ "meta": null })
            }
        }
    }

    // ----- streams ----------------------------------------------------------

    async fn resolve_id(&self, media_type: MediaType, id: &str) -> Option<(String, usize, usize)> {
        let parse_se = |parts: &[&str]| -> (usize, usize) {
            match parts {
                [s, e, ..] => (s.parse().unwrap_or(0), e.parse().unwrap_or(0)),
                _ => (0, 0),
            }
        };
        if let Some(rest) = id.strip_prefix(ID_PREFIX) {
            let parts: Vec<&str> = rest.split(':').collect();
            let subject = parts.first().filter(|s| !s.is_empty())?.to_string();
            let (s, e) = parse_se(&parts[1..]);
            return Some((subject, s, e));
        }
        if id.starts_with("tt") {
            let parts: Vec<&str> = id.split(':').collect();
            let imdb = parts[0];
            let (s, e) = parse_se(&parts[1..]);
            let subject = self.imdb_to_subject(media_type, imdb).await?;
            return Some((subject, s, e));
        }
        None
    }

    async fn imdb_to_subject(&self, media_type: MediaType, imdb: &str) -> Option<String> {
        let cache_key = format!("{}:{imdb}", type_str(media_type));
        let cached = lock(&self.imdb).get(&cache_key).cloned();
        if let Some(hit) = cached {
            return Some(hit);
        }

        let url = format!(
            "https://v3-cinemeta.strem.io/meta/{}/{imdb}.json",
            type_str(media_type)
        );
        let payload: Value = match self.svc.http_client.get(&url).send().await {
            Ok(resp) => resp.json().await.ok()?,
            Err(err) => {
                eprintln!("cinemeta lookup for {imdb} failed: {err}");
                return None;
            }
        };
        let meta = payload.get("meta")?;
        let name = meta.get("name")?.as_str()?.to_string();
        let year = meta
            .get("year")
            .or_else(|| meta.get("releaseInfo"))
            .and_then(Value::as_str)
            .and_then(first_year);
        let indian = meta
            .get("country")
            .and_then(Value::as_str)
            .is_some_and(|c| c.to_lowercase().contains("india"));
        {
            let mut map = lock(&self.indian);
            if map.len() > MAX_DETAIL_KEYS {
                map.clear();
            }
            map.insert(cache_key.clone(), indian);
        }

        let mut best: Option<(i32, String)> = None;
        for page in 1..=2 {
            let items = match self
                .svc
                .search_typed(ProviderKind::MovieBox, &name, page)
                .await
            {
                Ok(items) if !items.is_empty() => items,
                _ => break,
            };
            for item in &items {
                if let Some(score) = match_score(&name, year, media_type, item) {
                    if best.as_ref().is_none_or(|(s, _)| score > *s) {
                        best = Some((score, item.id.value.clone()));
                    }
                }
            }
            if best.as_ref().is_some_and(|(s, _)| *s >= 100) {
                break;
            }
        }

        let Some((_, subject)) = best else {
            eprintln!("no MovieBox match for {imdb} ({name})");
            return None;
        };
        let mut map = lock(&self.imdb);
        if map.len() > MAX_DETAIL_KEYS {
            map.clear();
        }
        map.insert(cache_key, subject.clone());
        Some(subject)
    }

    /// The audio versions MovieBox has for a title (`tt…` or `mbx…` id).
    /// `found` is false when the title is not on MovieBox or the lookup failed,
    /// so the caller may ask again later.
    async fn dubs(&self, ty: &str, id: &str) -> Value {
        let media_type = parse_type(ty).unwrap_or(MediaType::Movie);
        let Some((subject, _, _)) = self.resolve_id(media_type, id).await else {
            return json!({ "found": false, "hindi": false, "languages": [] });
        };
        match self.details(&subject).await {
            Ok(d) => {
                let languages: Vec<String> = d
                    .dubs
                    .iter()
                    .map(dub_label)
                    .filter(|l| !l.is_empty())
                    .collect();
                let hindi = languages.iter().any(|l| audio_rank(l, false) == 0);
                json!({ "found": true, "hindi": hindi, "languages": languages })
            }
            Err(err) => {
                eprintln!("dubs for {id}: {err}");
                json!({ "found": false, "hindi": false, "languages": [] })
            }
        }
    }

    async fn streams(&self, ty: &str, id: &str, base: &str) -> Value {
        let media_type = parse_type(ty).unwrap_or(MediaType::Movie);
        let Some((subject, season, episode)) = self.resolve_id(media_type, id).await else {
            return json!({ "streams": [] });
        };

        let details = match self.details(&subject).await {
            Ok(d) => Some(d),
            Err(err) => {
                eprintln!("details for {subject} failed: {err}");
                None
            }
        };

        // The title itself, then its dubbed versions (each dub is its own MovieBox subject).
        let mut subjects: Vec<(String, String)> = Vec::new();
        let main_label = details
            .as_ref()
            .and_then(|d| d.dubs.iter().find(|dub| dub.subject_id == subject))
            .map(dub_label)
            .unwrap_or_default();
        subjects.push((subject.clone(), main_label));
        if let Some(d) = &details {
            for dub in &d.dubs {
                if !dub.subject_id.is_empty() && !subjects.iter().any(|(s, _)| s == &dub.subject_id)
                {
                    subjects.push((dub.subject_id.clone(), dub_label(dub)));
                }
            }
        }
        subjects.truncate(MAX_DUB_SUBJECTS);
        let sibling_ids = details
            .as_ref()
            .map(MediaDetails::sibling_ids)
            .unwrap_or_else(|| vec![subject.clone()]);

        let lookups = subjects.iter().map(|(sid, label)| async move {
            let fut = moviebox_tui::providers::ReleaseProvider::episode_streams(
                &self.svc.client,
                sid,
                season,
                episode,
            );
            let releases = match tokio::time::timeout(STREAM_LOOKUP_TIMEOUT, fut).await {
                Ok(Ok(list)) => list,
                Ok(Err(err)) => {
                    eprintln!("streams for {sid} s{season}e{episode}: {err}");
                    Vec::new()
                }
                Err(_) => {
                    eprintln!("streams for {sid} s{season}e{episode}: timed out");
                    Vec::new()
                }
            };
            (sid.clone(), label.clone(), releases)
        });
        let results = futures::future::join_all(lookups).await;

        // Subtitles: one lookup, shared by every stream of this episode.
        let caption_rid = results
            .iter()
            .flat_map(|(_, _, rels)| rels.iter())
            .find_map(|r| r.resource_id.clone())
            .unwrap_or_default();
        let captions: Vec<SubtitleOption> = match tokio::time::timeout(
            CAPTION_LOOKUP_TIMEOUT,
            self.svc
                .get_ext_captions(&subject, &caption_rid, &sibling_ids, season, episode),
        )
        .await
        {
            Ok(Ok(list)) => list,
            Ok(Err(err)) => {
                log_debug(&format!("captions failed: {err}"));
                Vec::new()
            }
            Err(_) => Vec::new(),
        };

        // Was this title made in India? Then its "Original Audio" is most likely Hindi.
        let indian_original = id.starts_with("tt")
            && lock(&self.indian)
                .get(&format!(
                    "{}:{}",
                    type_str(media_type),
                    id.split(':').next().unwrap_or(id)
                ))
                .copied()
                .unwrap_or(false);

        let ua = self.svc.client.user_agent().to_string();
        let default_headers = vec![("User-Agent".to_string(), ua)];
        // English subtitles first, so the player's first pick is English.
        let mut captions = captions;
        captions.sort_by_key(|s| u8::from(stremio_lang(&s.name) != "eng"));
        let subtitles: Vec<Value> = captions
            .iter()
            .enumerate()
            .filter(|(_, s)| s.url.starts_with("http"))
            .map(|(i, s)| {
                let url = if self.cfg.proxy_streams {
                    proxied_url(base, &s.url, &default_headers)
                } else {
                    s.url.clone()
                };
                json!({ "id": format!("mbx-sub-{i}"), "url": url, "lang": stremio_lang(&s.name) })
            })
            .collect();

        let mut streams = Vec::new();
        let mut seen = HashSet::new();
        for (_sid, label, releases) in &results {
            for rel in releases {
                let Some(mirror) = rel.mirrors.first() else {
                    continue;
                };
                let url = mirror.resolver_url.as_str();
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    continue;
                }
                if !seen.insert(url.split('?').next().unwrap_or(url).to_string()) {
                    continue;
                }
                let headers = if mirror.headers.is_empty() {
                    default_headers.clone()
                } else {
                    mirror.headers.clone()
                };
                let is_dash = moviebox_tui::player::is_dash_url(url);
                let quality = if rel.is_multi_resolution() || is_dash {
                    "Auto".to_string()
                } else {
                    rel.quality
                        .clone()
                        .filter(|q| !q.trim().is_empty())
                        .unwrap_or_else(|| "SD".to_string())
                };
                let language = rel
                    .language
                    .clone()
                    .filter(|l| !l.trim().is_empty())
                    .unwrap_or_else(|| label.clone());

                let mut info = Vec::new();
                if !language.is_empty() {
                    info.push(format!("🔊 {language}"));
                }
                if let Some(size) = rel.size_bytes.filter(|&s| s > 0) {
                    info.push(format!("💾 {}", human_size(size)));
                }
                if let Some(codec) = rel.codec.as_deref().filter(|c| !c.trim().is_empty()) {
                    info.push(codec.to_string());
                }
                if is_dash {
                    info.push("DASH".to_string());
                }
                if !subtitles.is_empty() {
                    info.push(format!("💬 {} subs", subtitles.len()));
                }
                let filename = rel.filename.trim();
                let description = if filename.is_empty() {
                    info.join(" · ")
                } else {
                    format!("{filename}\n{}", info.join(" · "))
                };

                let mut hints = json!({
                    "bingeGroup": format!("moviebox-{quality}-{language}"),
                });
                if let Some(size) = rel.size_bytes.filter(|&s| s > 0) {
                    hints["videoSize"] = json!(size);
                }
                if !filename.is_empty() {
                    hints["filename"] = json!(filename);
                }

                let stream_url = if self.cfg.proxy_streams {
                    proxied_url(base, url, &headers)
                } else {
                    let request: serde_json::Map<String, Value> = headers
                        .iter()
                        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                        .collect();
                    hints["notWebReady"] = json!(true);
                    hints["proxyHeaders"] = json!({ "request": request });
                    url.to_string()
                };

                // Only `description`: Stremio's core treats `title` as an alias of
                // `description`, so sending both is a duplicate-field error that
                // makes it discard this addon's whole stream list.
                streams.push((
                    audio_rank(&language, indian_original),
                    json!({
                        "name": format!("MovieBox\n{quality}"),
                        "description": description,
                        "url": stream_url,
                        "subtitles": subtitles,
                        "behaviorHints": hints,
                    }),
                ));
            }
        }
        // Hindi audio first, then English; the order within a language is kept.
        streams.sort_by_key(|(rank, _)| *rank);
        let streams: Vec<Value> = streams.into_iter().map(|(_, s)| s).collect();

        if streams.is_empty() {
            eprintln!("no playable streams for {id}");
        }
        json!({ "streams": streams, "cacheMaxAge": 300 })
    }

    // ----- stream proxy -----------------------------------------------------

    async fn proxy<W: AsyncWrite + Unpin>(
        &self,
        req: &Request,
        base: &str,
        w: &mut W,
    ) -> std::io::Result<()> {
        let rest = req.target.strip_prefix("/p/").unwrap_or("");
        let Some((token, after)) = rest.split_once('/') else {
            return send(w, 400, "text/plain", b"bad proxy path", &[]).await;
        };
        let Some((scheme, tail)) = after.split_once('/') else {
            return send(w, 400, "text/plain", b"bad proxy path", &[]).await;
        };
        if scheme != "https" && scheme != "http" {
            return send(w, 400, "text/plain", b"bad scheme", &[]).await;
        }
        let Some((token_headers, origin)) = decode_token(token) else {
            return send(w, 400, "text/plain", b"bad token", &[]).await;
        };
        let target = format!("{scheme}://{tail}");
        let host = authority(&target);
        let same_host = host.is_some() && host == origin;
        // Cookies and other auth only go to the host they were issued for.
        let headers: Vec<(String, String)> = token_headers
            .into_iter()
            .filter(|(k, _)| {
                same_host
                    || k.eq_ignore_ascii_case("user-agent")
                    || k.eq_ignore_ascii_case("referer")
            })
            .collect();

        let path_only = target
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_manifest_path = path_only.ends_with(".mpd") || path_only.ends_with(".m3u8");
        let is_head = req.method.eq_ignore_ascii_case("HEAD");
        let range = req.header("range").map(str::to_string);

        // MovieBox's DASH CDN throttles large segment requests; fetch them in small ranges.
        if path_only.ends_with(".m4s") && range.is_none() && !is_head {
            return match fetch_chunked(&self.stream_client, &target, &headers).await {
                Ok((status, ct, body)) => send(w, status, &ct, &body, &[]).await,
                Err(err) => send(w, 502, "text/plain", err.to_string().as_bytes(), &[]).await,
            };
        }

        let mut rb = if is_head {
            self.stream_client.head(&target)
        } else {
            self.stream_client.get(&target)
        };
        for (k, v) in &headers {
            rb = rb.header(k.as_str(), v.as_str());
        }
        if let Some(r) = &range {
            if !is_manifest_path {
                rb = rb.header("Range", r.as_str());
            }
        }
        let resp = match rb.send().await {
            Ok(r) => r,
            Err(err) => {
                eprintln!("proxy upstream error: {err}");
                return send(w, 502, "text/plain", err.to_string().as_bytes(), &[]).await;
            }
        };

        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let ct_lower = content_type.to_ascii_lowercase();
        let is_manifest =
            is_manifest_path || ct_lower.contains("dash+xml") || ct_lower.contains("mpegurl");

        if is_manifest && !is_head {
            let body = match resp.bytes().await {
                Ok(b) if b.len() <= MAX_MANIFEST_BYTES => b,
                Ok(_) => return send(w, 502, "text/plain", b"manifest too large", &[]).await,
                Err(err) => {
                    return send(w, 502, "text/plain", err.to_string().as_bytes(), &[]).await;
                }
            };
            let text = String::from_utf8_lossy(&body);
            let rewritten = rewrite_manifest(&text, base, token, host.as_deref());
            let ct = if !content_type.is_empty() {
                content_type.as_str()
            } else if path_only.ends_with(".mpd") {
                "application/dash+xml"
            } else {
                "application/vnd.apple.mpegurl"
            };
            return send(w, status, ct, rewritten.as_bytes(), &[]).await;
        }

        let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
        for name in [
            reqwest::header::CONTENT_TYPE,
            reqwest::header::CONTENT_LENGTH,
            reqwest::header::CONTENT_RANGE,
            reqwest::header::ACCEPT_RANGES,
            reqwest::header::LAST_MODIFIED,
            reqwest::header::ETAG,
        ] {
            if let Some(value) = resp.headers().get(&name).and_then(|v| v.to_str().ok()) {
                head.push_str(name.as_str());
                head.push_str(": ");
                head.push_str(value);
                head.push_str("\r\n");
            }
        }
        head.push_str(CORS);
        head.push_str("Connection: close\r\n\r\n");
        w.write_all(head.as_bytes()).await?;
        if is_head {
            return w.flush().await;
        }

        let mut body = resp.bytes_stream();
        loop {
            match tokio::time::timeout(CHUNK_IDLE_TIMEOUT, body.next()).await {
                Ok(Some(Ok(chunk))) => w.write_all(&chunk).await?,
                Ok(Some(Err(err))) => {
                    log_debug(&format!("upstream body error: {err}"));
                    break;
                }
                Ok(None) | Err(_) => break,
            }
        }
        w.flush().await
    }
}

fn page_slice(items: &[Value], skip: usize) -> Vec<Value> {
    items.iter().skip(skip).take(PAGE_OUT).cloned().collect()
}

// ---------------------------------------------------------------------------
// Proxy helpers
// ---------------------------------------------------------------------------

fn authority(url: &str) -> Option<String> {
    let after = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let auth = after.split(['/', '?', '#']).next()?;
    (!auth.is_empty()).then(|| auth.to_string())
}

fn encode_token(headers: &[(String, String)], origin: Option<&str>) -> String {
    let payload = json!({ "h": headers, "o": origin });
    let bytes = serde_json::to_vec(&payload).unwrap_or_default();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn decode_token(token: &str) -> Option<(Vec<(String, String)>, Option<String>)> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token)
        .ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    let headers = v
        .get("h")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|pair| {
                    let pair = pair.as_array()?;
                    Some((
                        pair.first()?.as_str()?.to_string(),
                        pair.get(1)?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let origin = v.get("o").and_then(Value::as_str).map(str::to_string);
    Some((headers, origin))
}

/// `https://cdn/a/b.mpd?x` -> `{base}/p/{token}/https/cdn/a/b.mpd?x`.
/// Keeping the original path means relative DASH/HLS segment URLs resolve back through the proxy.
fn proxied_url(base: &str, target: &str, headers: &[(String, String)]) -> String {
    let token = encode_token(headers, authority(target).as_deref());
    let (scheme, rest) = if let Some(r) = target.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = target.strip_prefix("http://") {
        ("http", r)
    } else {
        ("https", target)
    };
    format!("{base}/p/{token}/{scheme}/{rest}")
}

fn rewrite_manifest(text: &str, base: &str, token: &str, host: Option<&str>) -> String {
    let Some(host) = host else {
        return text.to_string();
    };
    text.replace(
        &format!("https://{host}/"),
        &format!("{base}/p/{token}/https/{host}/"),
    )
    .replace(
        &format!("http://{host}/"),
        &format!("{base}/p/{token}/http/{host}/"),
    )
}

async fn fetch_chunked(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<(u16, String, Vec<u8>), reqwest::Error> {
    let mut rb = client
        .get(url)
        .header("Range", format!("bytes=0-{}", DASH_RANGE_CHUNK_BYTES - 1));
    for (k, v) in headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    let res = rb.send().await?;
    let status = res.status();
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("video/iso.segment")
        .to_string();

    if status != reqwest::StatusCode::PARTIAL_CONTENT {
        let code = if status.is_success() {
            200
        } else {
            status.as_u16()
        };
        let body = res.bytes().await?;
        return Ok((code, content_type, body.to_vec()));
    }

    let total = res
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|t| t.trim().parse::<usize>().ok());
    let first = res.bytes().await?;
    let Some(total) = total.filter(|&t| t > first.len() && t <= MAX_SEGMENT_BYTES) else {
        return Ok((200, content_type, first.to_vec()));
    };

    let mut ranges = Vec::new();
    let mut pos = first.len();
    while pos < total {
        let end = (pos + DASH_RANGE_CHUNK_BYTES - 1).min(total - 1);
        ranges.push((pos, end));
        pos = end + 1;
    }

    let parts: Vec<_> = futures::stream::iter(ranges)
        .map(|(start, end)| {
            let client = client.clone();
            let url = url.to_string();
            let headers = headers.to_vec();
            async move {
                let mut rb = client
                    .get(&url)
                    .header("Range", format!("bytes={start}-{end}"));
                for (k, v) in &headers {
                    rb = rb.header(k.as_str(), v.as_str());
                }
                rb.send().await?.error_for_status()?.bytes().await
            }
        })
        .buffered(6)
        .collect()
        .await;

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&first);
    for part in parts {
        out.extend_from_slice(&part?);
    }
    Ok((200, content_type, out))
}

// ---------------------------------------------------------------------------
// Small text helpers
// ---------------------------------------------------------------------------

/// Stream order by audio: Hindi, then an Indian title's original audio (most
/// likely Hindi), then English, then any other original audio, then the rest.
fn audio_rank(language: &str, indian_original: bool) -> u8 {
    let l = language.trim().to_lowercase();
    if l.contains("hindi") || l == "hin" || l == "hi" {
        0
    } else if l.contains("english") || l == "eng" || l == "en" {
        2
    } else if l.is_empty() || l.contains("original") || l == "unknown" {
        if indian_original { 1 } else { 3 }
    } else {
        4
    }
}

fn dub_label(dub: &AudioTrackOption) -> String {
    let lang = dub.language.trim();
    if !lang.is_empty() && !lang.eq_ignore_ascii_case("unknown") {
        lang.to_string()
    } else {
        dub.label.trim().to_string()
    }
}

fn first_year(s: &str) -> Option<i32> {
    let bytes = s.as_bytes();
    (0..bytes.len().saturating_sub(3)).find_map(|i| {
        let window = &bytes[i..i + 4];
        let boundary_before = i == 0 || !bytes[i - 1].is_ascii_digit();
        let boundary_after = bytes.get(i + 4).is_none_or(|b| !b.is_ascii_digit());
        (window.iter().all(u8::is_ascii_digit) && boundary_before && boundary_after)
            .then(|| s[i..i + 4].parse().ok())
            .flatten()
    })
}

fn normalize_title(s: &str) -> String {
    s.to_lowercase()
        .replace('&', " and ")
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn match_score(
    name: &str,
    year: Option<i32>,
    media_type: MediaType,
    item: &CatalogItem,
) -> Option<i32> {
    if item.media_type != media_type {
        return None;
    }
    let wanted = normalize_title(name);
    let raw = item.title.trim();
    let cleaned = normalize_title(moviebox_tui::providers::moviebox::clean_moviebox_title(raw));
    if wanted.is_empty() || cleaned.is_empty() {
        return None;
    }
    let mut score = if cleaned == wanted {
        80
    } else if cleaned.starts_with(&wanted) || wanted.starts_with(&cleaned) {
        40
    } else {
        return None;
    };
    if let (Some(a), Some(b)) = (year, item.year.as_deref().and_then(first_year)) {
        match (a - b).abs() {
            0 => score += 20,
            1 => score += 10,
            _ if media_type == MediaType::Movie => return None,
            _ => score -= 10,
        }
    }
    if !raw.contains('[') {
        // Prefer the original-language subject; dubs are reachable from it.
        score += 5;
    }
    (score >= 60).then_some(score)
}

fn human_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.2} GB", b / (1024.0 * 1024.0 * 1024.0))
    } else if b >= 1024.0 * 1024.0 {
        format!("{:.0} MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB", b / 1024.0)
    }
}

/// Stremio auto-selects subtitles by ISO 639-2 code; anything else is shown as text.
fn stremio_lang(name: &str) -> String {
    let label = moviebox_tui::tui::text::sanitize_language_label(name);
    let code = moviebox_tui::providers::moviebox::title::language_to_code(&label)
        .or_else(|| moviebox_tui::providers::moviebox::title::language_to_code(name));
    let three = match code {
        Some("en") => "eng",
        Some("es") => "spa",
        Some("hi") => "hin",
        Some("fr") => "fre",
        Some("de") => "ger",
        Some("it") => "ita",
        Some("ja") => "jpn",
        Some("ko") => "kor",
        Some("zh") => "chi",
        Some("pt") => "por",
        Some("ru") => "rus",
        Some("ar") => "ara",
        Some("tr") => "tur",
        Some("bn") => "ben",
        Some("ta") => "tam",
        Some("te") => "tel",
        Some("ml") => "mal",
        Some("kn") => "kan",
        Some("mr") => "mar",
        Some("pa") => "pan",
        Some("gu") => "guj",
        Some("ur") => "urd",
        Some("id") => "ind",
        Some("th") => "tha",
        Some("vi") => "vie",
        Some("nl") => "dut",
        Some("pl") => "pol",
        Some("sv") => "swe",
        Some("da") => "dan",
        Some("no") => "nor",
        Some("fi") => "fin",
        Some("el") => "gre",
        Some("he") => "heb",
        Some("cs") => "cze",
        Some("hu") => "hun",
        Some("ro") => "rum",
        Some("uk") => "ukr",
        Some("fa") => "per",
        Some("tl") => "fil",
        Some("ms") => "may",
        _ => "",
    };
    if three.is_empty() {
        if label.trim().is_empty() {
            name.trim().to_string()
        } else {
            label
        }
    } else {
        three.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hindi_audio_comes_first_then_english() {
        // A Hollywood film: Hindi dub, then English, then the rest.
        assert!(audio_rank("Hindi", false) < audio_rank("English", false));
        assert!(audio_rank("English", false) < audio_rank("Original Audio", false));
        assert!(audio_rank("Original Audio", false) < audio_rank("Tamil", false));
        // An Indian film: its original audio (Hindi) beats an English dub.
        assert!(audio_rank("Original Audio", true) < audio_rank("English", true));
        assert!(audio_rank("Hindi", true) < audio_rank("Original Audio", true));
        assert_eq!(audio_rank("", true), audio_rank("Original Audio", true));
    }

    #[test]
    fn year_parsing() {
        assert_eq!(first_year("2019"), Some(2019));
        assert_eq!(first_year("2008–2013"), Some(2008));
        assert_eq!(first_year("abc"), None);
        assert_eq!(first_year("12345"), None);
    }

    #[test]
    fn token_round_trip_and_proxy_url() {
        let headers = vec![("User-Agent".to_string(), "ua".to_string())];
        let url = proxied_url(
            "http://127.0.0.1:7000",
            "https://cdn.example.com/a/b.mpd?x=1",
            &headers,
        );
        let rest = url.strip_prefix("http://127.0.0.1:7000/p/").unwrap();
        let (token, tail) = rest.split_once('/').unwrap();
        assert_eq!(tail, "https/cdn.example.com/a/b.mpd?x=1");
        let (h, origin) = decode_token(token).unwrap();
        assert_eq!(h, headers);
        assert_eq!(origin.as_deref(), Some("cdn.example.com"));
    }

    #[test]
    fn manifest_rewrite_is_host_scoped() {
        let mpd = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011"><BaseURL>https://cdn.x/v/1080.mp4</BaseURL><BaseURL>https://other.y/z.mp4</BaseURL></MPD>"#;
        let out = rewrite_manifest(mpd, "http://h:7000", "TOK", Some("cdn.x"));
        assert!(out.contains("http://h:7000/p/TOK/https/cdn.x/v/1080.mp4"));
        assert!(out.contains("https://other.y/z.mp4"));
    }

    #[test]
    fn title_matching() {
        let item = |title: &str, year: &str| CatalogItem {
            id: moviebox_tui::providers::models::ProviderMediaId {
                provider: ProviderKind::MovieBox,
                value: "1".to_string(),
            },
            title: title.to_string(),
            media_type: MediaType::Movie,
            year: Some(year.to_string()),
            poster_url: None,
            season_count: None,
        };
        assert!(
            match_score(
                "Avatar",
                Some(2009),
                MediaType::Movie,
                &item("Avatar", "2009")
            )
            .is_some()
        );
        assert!(
            match_score(
                "Avatar",
                Some(2009),
                MediaType::Movie,
                &item("Avatar", "2022")
            )
            .is_none()
        );
        let orig = match_score(
            "Avatar",
            Some(2009),
            MediaType::Movie,
            &item("Avatar", "2009"),
        )
        .unwrap();
        let dub = match_score(
            "Avatar",
            Some(2009),
            MediaType::Movie,
            &item("Avatar [Hindi]", "2009"),
        )
        .unwrap();
        assert!(orig > dub);
    }
}
