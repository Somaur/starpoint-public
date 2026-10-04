use super::*;
use serde_json::{json, Map};

const RESOURCE_FIELDS: [&str; 4] = ["free_vmoney", "vmoney", "free_mana", "exp_pool"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditRequest {
    expected_etag: String,
    resources: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteRequest {
    expected_etag: String,
}

fn valid_etag(etag: &str) -> bool {
    etag.len() == 64
        && etag
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    prefix: &str,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let segments = request
        .path()
        .strip_prefix(prefix)?
        .split('/')
        .collect::<Vec<_>>();
    let result = match segments.as_slice() {
        ["new"] if request.method() == "POST" => create_new(request, database),
        ["deleted"] if request.method() == "GET" => match database.deleted_local_save_backups() {
            Ok(backups) => serialize_json("200 OK", json!({"backups": backups})),
            Err(error) => map_store_error(error),
        },
        ["deleted", id, "restore"] if request.method() == "POST" => match parse_id(id) {
            Some(id) => map_slot_result(database.restore_deleted_local_save(id)),
            None => Ok(json_error("400 Bad Request", "invalid_local_save_id")),
        },
        [id, "editor"] if request.method() == "GET" => editor(database, id),
        [id, "resources"] if request.method() == "PATCH" => edit(request, database, id),
        ["new"] | ["deleted"] | ["deleted", _, "restore"] | [_, "editor"] | [_, "resources"] => {
            Ok(method_not_allowed())
        }
        [id] if request.method() == "DELETE" => delete(request, database, id),
        _ => return None,
    };
    Some(result)
}

fn create_new(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let Some(body) = parse_json::<NameRequest>(request) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_name"));
    };
    let Some(name) = normalize_text(&body.name) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_name"));
    };
    let data = crate::cn_player::create_beginner_player_data(
        database.current_server_time_seconds()?,
        &database.get_current_client_time()?,
    )?;
    map_slot_result(database.import_local_save(&name, &data))
}

fn editor(database: &mut ServiceDatabase, id: &str) -> Result<HttpResponse, PersonalServiceError> {
    let Some(id) = parse_id(id) else {
        return Ok(json_error("400 Bad Request", "invalid_local_save_id"));
    };
    let Some(export) = database.export_local_save(id)? else {
        return Ok(json_error("404 Not Found", "local_save_not_found"));
    };
    let data: Value = serde_json::from_str(&export.data_json)
        .map_err(|e| PersonalServiceError::new(e.to_string()))?;
    let resources: Map<String, Value> = RESOURCE_FIELDS
        .into_iter()
        .map(|key| {
            (
                key.to_owned(),
                data["user_info"][key].as_i64().unwrap_or_default().into(),
            )
        })
        .collect();
    serialize_json(
        "200 OK",
        json!({"etag":export.etag, "name":export.slot.name, "resources":resources}),
    )
}

fn edit(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(id), Some(body)) = (parse_id(id), parse_json::<EditRequest>(request)) else {
        return Ok(json_error("400 Bad Request", "invalid_save_edit"));
    };
    if !valid_etag(&body.expected_etag)
        || body.resources.is_empty()
        || body.resources.iter().any(|(key, value)| {
            !RESOURCE_FIELDS.contains(&key.as_str())
                || !value
                    .as_i64()
                    .is_some_and(|n| (0..=99_999_999).contains(&n))
        })
    {
        return Ok(json_error("400 Bad Request", "invalid_save_resources"));
    }
    match database.edit_local_save_resources(id, &body.expected_etag, &body.resources) {
        Ok(revision) => serialize_json("200 OK", json!({"saved":true, "etag":revision.etag})),
        Err(error) => map_store_error(error),
    }
}

fn delete(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    id: &str,
) -> Result<HttpResponse, PersonalServiceError> {
    let (Some(id), Some(body)) = (parse_id(id), parse_json::<DeleteRequest>(request)) else {
        return Ok(json_error("400 Bad Request", "invalid_save_delete"));
    };
    if !valid_etag(&body.expected_etag) {
        return Ok(json_error("400 Bad Request", "invalid_save_revision_etag"));
    }
    match database.delete_local_save_with_backup(id, &body.expected_etag) {
        Ok(backup_id) => serialize_json("200 OK", json!({"deleted":true, "backup_id":backup_id})),
        Err(error) => map_store_error(error),
    }
}
