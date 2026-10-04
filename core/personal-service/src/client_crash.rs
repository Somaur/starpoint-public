// Keep only bounded diagnostic fields, never the device identifiers in a crash upload.
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn parse(body: &[u8]) -> Option<Value> {
    if body.len() > 1024 * 1024 {
        return None;
    }
    let envelope = serde_json::from_slice::<Value>(body).ok().or_else(|| {
        url::form_urlencoded::parse(body).find_map(|(key, value)| {
            (key == "info")
                .then(|| serde_json::from_str::<Value>(&value).ok())
                .flatten()
        })
    })?;
    let info = if let Some(value) = envelope.get("info") {
        nested(value)?
    } else {
        envelope
    };
    let error = info.get("error").and_then(nested).unwrap_or(Value::Null);
    let code = info
        .get("code")
        .and_then(Value::as_str)
        .or_else(|| error.get("code").and_then(Value::as_str))?;
    if code.len() > 32 || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(json!({
        "code": code,
        "message": bounded(error.get("internalMessage"), 4096),
        "stack": bounded(info.get("stackTrace"), 16384),
        "resource_version": bounded(info.get("resourceVersion"), 64),
        "received_at_unix": SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs(),
    }))
}

fn nested(value: &Value) -> Option<Value> {
    if let Some(text) = value.as_str() {
        serde_json::from_str(text).ok()
    } else {
        Some(value.clone())
    }
}

fn bounded(value: Option<&Value>, limit: usize) -> Option<String> {
    Some(value?.as_str()?.chars().take(limit).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_client_form_and_json_without_device_fields() {
        let info = json!({"error": "{\"code\":\"C8013\",\"internalMessage\":\"missing master\"}",
            "stackTrace":"gacha/result", "resourceVersion":"1.4.57", "imei":"private-id", "userId":77});
        let form = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("info", &info.to_string())
            .finish();
        for body in [
            form.into_bytes(),
            json!({"info":info.to_string()}).to_string().into_bytes(),
            info.to_string().into_bytes(),
        ] {
            let result = parse(&body).unwrap();
            assert_eq!(result["code"], "C8013");
            assert_eq!(result["message"], "missing master");
            assert_eq!(result["stack"], "gacha/result");
            assert!(!result.to_string().contains("private-id"));
            assert!(result.get("userId").is_none());
        }
    }
    #[test]
    fn bounds_untrusted_text_and_rejects_non_reports() {
        assert_eq!(parse(json!({"code":"C8013","error":"{\"code\":8013,\"internalMessage\":\"missing master\"}"}).to_string().as_bytes()).unwrap()["message"], "missing master");
        assert!(parse(b"not-json").is_none());
        assert!(parse(&vec![0; 1024 * 1024 + 1]).is_none());
        let result = parse(
            json!({"error":{"code":"C8013","internalMessage":"错".repeat(5000)}})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(result["message"].as_str().unwrap().chars().count(), 4096);
    }
}
