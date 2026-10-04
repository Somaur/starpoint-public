mod support;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

#[test]
fn collects_bounded_redacted_reports_and_obeys_management_authorization() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    for n in 0..10 {
        let info = json!({"code":"C8013","error":json!({"code":8013,"internalMessage":format!("master {n}")}).to_string(),
            "resourceVersion":"1.4.57","stackTrace":"equipment/result", "imei":"private-device"});
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("info", &info.to_string())
            .finish();
        let result = support::request_with_body(
            service.port(),
            "POST",
            "/crash",
            "application/x-www-form-urlencoded",
            body.as_bytes(),
        );
        assert!(result.starts_with("HTTP/1.1 200"));
        assert!(result.ends_with("OK"));
    }
    let result = support::request(service.port(), "GET", "/v1/client-crashes");
    assert!(!result.contains("private-device"));
    let body: Value = serde_json::from_str(result.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["crashes"].as_array().unwrap().len(), 8);
    assert_eq!(body["crashes"][0]["message"], "master 2");
    let invalid = support::request_with_headers(
        service.port(),
        "GET",
        "/v1/client-crashes",
        "text/plain",
        &[("Authorization", "Bearer invalid")],
        b"",
    );
    assert!(invalid.starts_with("HTTP/1.1 401"));
    assert!(
        support::request(service.port(), "POST", "/v1/client-crashes").starts_with("HTTP/1.1 405")
    );
    service.stop().unwrap();
}
