#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;
use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

fn list(port: u16, viewer: i64, kind: i64, page: i64) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        port,
        "/api/index.php/comic/get_list",
        &encode_request(&json!({"viewer_id":viewer,"kind":kind,"page_index":page})),
    ))
    .data
}
fn put(port: u16, path: &str, body: &[u8]) -> String {
    support::request_with_body(port, "PUT", path, "application/octet-stream", body)
}
#[test]
fn offline_comics_paginate_validate_import_and_survive_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let port = service.port();
    let viewer = decode_response::<SignupData>(&cn_support::send_request(
        port,
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 872 }),
    ))
    .data_headers
    .viewer_id;
    for kind in [0, 1] {
        let first = list(port, viewer, kind, 0);
        assert_eq!(first["total_count"], 1);
        let main = first["comic_list"][0]["media_image"]["main"]
            .as_str()
            .unwrap();
        assert!(main.starts_with(&format!("http://127.0.0.1:{port}/")));
        let response =
            support::request_bytes(port, "GET", main.split_once(&port.to_string()).unwrap().1);
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        assert!(response.len() > 100_000);
    }
    let rows: Vec<_> = (1..=12)
        .map(|n| json!({"kind":0,"episode":n,"title":format!("Episode {n}")}))
        .chain(std::iter::once(
            json!({"kind":1,"episode":1,"title":"講座"}),
        ))
        .collect();
    let body = serde_json::to_vec(&rows).unwrap();
    assert!(put(port, "/v1/local-comics/catalog", &body).contains("incomplete_comic_catalog"));
    assert!(put(port, "/v1/local-comics/0/2/base", b"not a PNG").contains("invalid_comic_image"));
    assert!(put(port, "/v1/local-comics/0/../base", b"bad").contains("invalid_comic_path"));
    let png = include_bytes!("../assets/cn-comic-fallback/0-small.png");
    let mut corrupt = png.to_vec();
    corrupt[50] ^= 1;
    assert!(put(port, "/v1/local-comics/0/2/base", &corrupt).contains("invalid_comic_image"));
    assert!(support::request_with_headers(
        port,
        "PUT",
        "/v1/local-comics/0/2/base",
        "image/png",
        &[("Authorization", "Bearer invalid")],
        png
    )
    .contains("401 Unauthorized"));
    for n in 2..=12 {
        for v in ["base", "large", "small"] {
            assert!(put(port, &format!("/v1/local-comics/0/{n}/{v}"), png)
                .starts_with("HTTP/1.1 200 OK"));
        }
    }
    assert!(put(port, "/v1/local-comics/catalog", &body).starts_with("HTTP/1.1 200 OK"));
    let first = list(port, viewer, 0, 0);
    assert_eq!(first["total_count"], 12);
    assert_eq!(first["comic_list"].as_array().unwrap().len(), 9);
    assert_eq!(first["comic_list"][0]["episode"], 12);
    assert_eq!(first["comic_list"][8]["episode"], 4);
    let last = list(port, viewer, 0, 999);
    assert_eq!(last["current_page_index"], 1);
    assert_eq!(last["comic_list"][0]["episode"], 3);
    assert_eq!(last["comic_list"][2]["episode"], 1);
    assert!(
        support::request_bytes(port, "GET", "/api/index.php/comic/image?kind=0&episode=99")
            .starts_with(b"HTTP/1.1 404")
    );
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(list(service.port(), viewer, 0, 0)["total_count"], 12);
    assert_eq!(list(service.port(), viewer, 1, 0)["total_count"], 1);
    service.stop().unwrap();
}

#[test]
fn fresh_install_reads_packaged_comics_without_importing_player_state() {
    let root = TempDir::new().unwrap();
    let cdn = TempDir::new().unwrap();
    for kind in 0..=1 {
        let dir = cdn.path().join(format!("local-comics/{kind}/2"));
        std::fs::create_dir_all(&dir).unwrap();
        for variant in ["base", "large", "small"] {
            std::fs::write(
                dir.join(format!("{variant}.png")),
                include_bytes!("../assets/cn-comic-fallback/0-small.png"),
            )
            .unwrap();
        }
    }
    let rows: Vec<_> = (0..=1)
        .flat_map(|kind| {
            (1..=2).map(
                move |episode| json!({"kind":kind,"episode":episode,"title":"Bundled episode"}),
            )
        })
        .collect();
    std::fs::write(
        cdn.path().join("local-comics/catalog.json"),
        serde_json::to_vec(&rows).unwrap(),
    )
    .unwrap();
    let service = PersonalService::start_with_cdn_root(root.path(), 0, cdn.path()).unwrap();
    let port = service.port();
    let viewer = decode_response::<SignupData>(&cn_support::send_request(
        port,
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id: 873 }),
    ))
    .data_headers
    .viewer_id;
    assert_eq!(list(port, viewer, 0, 0)["total_count"], 2);
    assert_eq!(list(port, viewer, 1, 0)["total_count"], 2);
    assert!(support::request_bytes(
        port,
        "GET",
        "/api/index.php/comic/image?kind=1&episode=2&variant=small"
    )
    .starts_with(b"HTTP/1.1 200 OK"));
    let imported = json!([
        {"kind":0,"episode":1,"title":"Imported"},
        {"kind":1,"episode":1,"title":"Imported"}
    ]);
    assert!(put(
        port,
        "/v1/local-comics/catalog",
        &serde_json::to_vec(&imported).unwrap()
    )
    .starts_with("HTTP/1.1 200 OK"));
    assert_eq!(list(port, viewer, 0, 0)["total_count"], 1);
    assert_eq!(
        serde_json::from_slice::<Value>(
            &std::fs::read(cdn.path().join("local-comics/catalog.json")).unwrap()
        )
        .unwrap(),
        json!(rows)
    );
    service.stop().unwrap();
}
