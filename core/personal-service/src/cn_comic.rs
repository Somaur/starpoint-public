//! Offline comic catalogue and loopback-only asset import. The native client
//! requires a non-empty, contiguous catalogue (the latest episode = total_count).
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::player_snapshot;
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

const PREFIX: &str = "/v1/local-comics/";
const VARIANTS: [&str; 3] = ["base", "large", "small"];
const FALLBACK: [[&[u8]; 3]; 2] = [
    [
        include_bytes!("../assets/cn-comic-fallback/0-base.png"),
        include_bytes!("../assets/cn-comic-fallback/0-large.png"),
        include_bytes!("../assets/cn-comic-fallback/0-small.png"),
    ],
    [
        include_bytes!("../assets/cn-comic-fallback/1-base.png"),
        include_bytes!("../assets/cn-comic-fallback/1-large.png"),
        include_bytes!("../assets/cn-comic-fallback/1-small.png"),
    ],
];

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Episode {
    kind: usize,
    episode: usize,
    title: String,
}
#[derive(Deserialize)]
struct ListRequest {
    viewer_id: i64,
    #[serde(default)]
    kind: usize,
    #[serde(default)]
    page_index: usize,
}

fn error(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, json!({"error":code}).to_string())
}
fn path(root: &Path, kind: usize, episode: usize, variant: &str) -> PathBuf {
    root.join("local-comics")
        .join(kind.to_string())
        .join(episode.to_string())
        .join(format!("{variant}.png"))
}
fn fallback(kind: usize) -> Episode {
    Episode {
        kind,
        episode: 1,
        title: if kind == 0 {
            "Bon Voyage [EN]"
        } else {
            "史黛拉講座 第1話"
        }
        .into(),
    }
}
fn catalogue(root: &Path, kind: usize) -> Vec<Episode> {
    let mut rows = fs::read(root.join("local-comics/catalog.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Episode>>(&bytes).ok())
        .unwrap_or_default();
    rows.retain(|e| e.kind == kind);
    rows.sort_by_key(|e| e.episode);
    // Never advertise a gap/missing image: the client's episode pager assumes 1..N.
    let usable = rows
        .iter()
        .enumerate()
        .take_while(|(i, e)| e.episode == i + 1 && available(root, e))
        .count();
    rows.truncate(usable);
    if rows.is_empty() {
        rows.push(fallback(kind));
    }
    rows
}
fn available(root: &Path, e: &Episode) -> bool {
    e.episode == 1
        || VARIANTS
            .iter()
            .all(|v| path(root, e.kind, e.episode, v).is_file())
}

pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    root: &Path,
    packaged_root: &Path,
    port: u16,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    // An explicitly imported catalogue takes precedence over the bundled archive.
    let read_root = if root.join("local-comics/catalog.json").is_file() {
        root
    } else {
        packaged_root
    };
    if request.path().starts_with(PREFIX) {
        return Some(import(request, database, root, read_root));
    }
    if request.path() == "/api/index.php/comic/image" {
        return Some(Ok(image(request, read_root)));
    }
    if request.path() != "/api/index.php/comic/get_list" {
        return None;
    }
    Some(list(request, database, read_root, port))
}

fn list(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    root: &Path,
    port: u16,
) -> Result<HttpResponse, PersonalServiceError> {
    if request.method() != "POST" {
        return Ok(error("405 Method Not Allowed", "method_not_allowed"));
    }
    let body = match decode_request::<ListRequest>(request) {
        Ok(body) if body.kind <= 1 && body.viewer_id > 0 => body,
        _ => return Ok(error("400 Bad Request", "invalid_comic_request")),
    };
    if let Err(response) = player_snapshot(database, body.viewer_id)? {
        return Ok(response);
    }
    let rows = catalogue(root, body.kind);
    let total = rows.len();
    let page = body.page_index.min((total - 1) / 9);
    let comics: Vec<_> = rows.iter().rev().skip(page*9).take(9).map(|e| {
        let url = |v| format!("http://127.0.0.1:{port}/api/index.php/comic/image?kind={}&episode={}&variant={v}", e.kind,e.episode);
        json!({"episode":e.episode,"title":e.title,"media_image":{"main":url("base"),"thumbnail_l":url("large"),"thumbnail_s":url("small")}})
    }).collect();
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({"comic_list":comics,"current_page_index":page,"total_count":total}),
    )
}

fn image(request: &HttpRequest, root: &Path) -> HttpResponse {
    if request.method() != "GET" {
        return error("405 Method Not Allowed", "method_not_allowed");
    }
    let query: std::collections::HashMap<_, _> = url::form_urlencoded::parse(
        request
            .target()
            .split_once('?')
            .map(|(_, q)| q)
            .unwrap_or("")
            .as_bytes(),
    )
    .into_owned()
    .collect();
    let kind = query
        .get("kind")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let episode = query
        .get("episode")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let variant = query.get("variant").map(String::as_str).unwrap_or("base");
    let Some(index) = VARIANTS.iter().position(|v| *v == variant) else {
        return error("400 Bad Request", "invalid_comic_variant");
    };
    if kind > 1 || episode == 0 || episode > 1000 {
        return error("400 Bad Request", "invalid_comic_episode");
    }
    let bytes = fs::read(path(root, kind, episode, variant))
        .ok()
        .or_else(|| (episode == 1).then(|| FALLBACK[kind][index].to_vec()));
    match bytes {
        Some(bytes) => HttpResponse::bytes("200 OK", "image/png", bytes)
            .with_header("Cache-Control", "no-cache"),
        None => error("404 Not Found", "comic_image_missing"),
    }
}

fn import(
    request: &HttpRequest,
    database: &ServiceDatabase,
    root: &Path,
    read_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    if !crate::management::is_authorized(request, database) {
        return Ok(error("401 Unauthorized", "unauthorized"));
    }
    let suffix = request.path().strip_prefix(PREFIX).unwrap_or("");
    if suffix == "catalog" && request.method() == "GET" {
        return Ok(HttpResponse::json(
            "200 OK",
            json!({"series":[catalogue(read_root,0),catalogue(read_root,1)]}).to_string(),
        ));
    }
    if request.method() != "PUT" {
        return Ok(error("405 Method Not Allowed", "method_not_allowed"));
    }
    let target = if suffix == "catalog" {
        let mut rows = match serde_json::from_slice::<Vec<Episode>>(request.body()) {
            Ok(rows) if !rows.is_empty() && rows.len() <= 2000 => rows,
            _ => return Ok(error("400 Bad Request", "invalid_comic_catalog")),
        };
        rows.sort_by_key(|e| (e.kind, e.episode));
        let mut counts = [0; 2];
        for e in &rows {
            if e.kind > 1
                || e.episode > 1000
                || e.episode != counts[e.kind] + 1
                || e.title.is_empty()
                || e.title.len() > 512
                || e.title.chars().any(char::is_control)
                || !available(root, e)
            {
                return Ok(error("400 Bad Request", "incomplete_comic_catalog"));
            }
            counts[e.kind] += 1;
        }
        if counts.contains(&0) {
            return Ok(error("400 Bad Request", "missing_comic_series"));
        }
        root.join("local-comics/catalog.json")
    } else {
        let parts: Vec<_> = suffix.split('/').collect();
        let parsed = if parts.len() == 3 {
            parts[0]
                .parse::<usize>()
                .ok()
                .zip(parts[1].parse::<usize>().ok())
        } else {
            None
        };
        let Some((kind, episode)) = parsed.filter(|(k, e)| *k <= 1 && *e > 0 && *e <= 1000) else {
            return Ok(error("400 Bad Request", "invalid_comic_path"));
        };
        if !VARIANTS.contains(&parts[2]) || !valid_png(request.body()) {
            return Ok(error("400 Bad Request", "invalid_comic_image"));
        }
        path(root, kind, episode, parts[2])
    };
    fs::create_dir_all(target.parent().unwrap())
        .map_err(|e| PersonalServiceError::new(format!("comic directory: {e}")))?;
    let temp = target.with_extension("tmp");
    fs::write(&temp, request.body())
        .and_then(|_| fs::rename(temp, target))
        .map_err(|e| PersonalServiceError::new(format!("comic import: {e}")))?;
    Ok(HttpResponse::json("200 OK", "{\"status\":\"ok\"}".into()))
}

fn valid_png(bytes: &[u8]) -> bool {
    if bytes.len() < 57 || bytes.len() > 8 * 1024 * 1024 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return false;
    }
    let mut p = 8;
    let mut data = false;
    while p + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[p..p + 4].try_into().unwrap()) as usize;
        let Some(end) = p
            .checked_add(12)
            .and_then(|n| n.checked_add(len))
            .filter(|n| *n <= bytes.len())
        else {
            return false;
        };
        let tag = &bytes[p + 4..p + 8];
        if crc32fast::hash(&bytes[p + 4..end - 4])
            != u32::from_be_bytes(bytes[end - 4..end].try_into().unwrap())
        {
            return false;
        }
        if p == 8 {
            if tag != b"IHDR" || len != 13 {
                return false;
            }
            for offset in [8, 12] {
                let n = u32::from_be_bytes(bytes[p + offset..p + offset + 4].try_into().unwrap());
                if n == 0 || n > 8192 {
                    return false;
                }
            }
        }
        if tag == b"IDAT" {
            data = true;
        }
        if tag == b"IEND" {
            return len == 0 && data && end == bytes.len();
        }
        p = end;
    }
    false
}
