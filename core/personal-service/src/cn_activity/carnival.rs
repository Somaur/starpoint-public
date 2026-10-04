// audience: internal
// # personal-service-cn-carnival-event
//
// 该模块返回 CN 嘉年华活动记录和持久化活动队伍.

use super::{closed_activity_response, error_response, party, state};
use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::database::ServiceDatabase;
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

const FAMILY: &str = "carnival";

#[derive(Deserialize)]
struct IndexRequest {
    event_id: i64,
    viewer_id: i64,
}

#[derive(Deserialize)]
struct PartyRequest {
    event_id: Option<i64>,
    viewer_id: i64,
}

pub(super) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    let response = match request.path() {
        "/api/index.php/carnival_event/index" => index(request, database, asset_root),
        "/api/index.php/carnival_event/get_party" => get_party(request, database, asset_root),
        _ => return None,
    };
    Some(response)
}

// //// 返回嘉年华活动首页 [@x380kkm 2026-08-22] ////
fn index(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<IndexRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.event_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    if let Some(response) =
        closed_activity_response(database, asset_root, &format!("{FAMILY}:{}", body.event_id))?
    {
        return Ok(response);
    }
    let root = player.root_mut()?;
    state::set_current_event(root, FAMILY, body.event_id)?;
    repair_legacy_scores(root, body.event_id)?;
    let records = state::event_state(root, FAMILY, body.event_id)
        .and_then(|event| event.get("carnival_records"))
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let party_groups = party::carnival_party_groups(root, FAMILY, body.event_id)?;
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({
            "records": records,
            "user_party_group_list": party_groups,
        }),
    )
}
// //// /返回嘉年华活动首页 ////

// Older service builds used the mana column as difficulty and frames as ms.
// Recover each recorded best from its real, persisted clear time.
pub(crate) fn repair_legacy_scores(
    root: &mut serde_json::Map<String, Value>,
    event_id: i64,
) -> Result<(), PersonalServiceError> {
    let fixture = crate::cn_battle_assets::load_battle_fixture()?;
    let clears = root
        .get("quest_progress")
        .and_then(|v| v.get("22"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let event = state::event_state_mut(root, FAMILY, event_id)?;
    if event.get("record_schema_version").and_then(Value::as_i64) != Some(2) {
        if let Some(old) = event.get("carnival_records").and_then(Value::as_array).cloned() {
            let mut migrated = std::collections::BTreeMap::<i64, Value>::new();
            for mut record in old.iter().cloned() {
                let old_folder = record.get("folder_id").and_then(Value::as_i64);
                let Some(folder) = fixture.quests.values().find(|q| {
                    q.carnival_event_id == Some(event_id)
                        && q.carnival_legacy_folder_id == old_folder
                }).and_then(|q| q.carnival_folder_id) else { continue };
                record["folder_id"] = json!(folder);
                let previous = migrated.get(&folder).and_then(|r| r["best_score"].as_i64()).unwrap_or(-1);
                if record["best_score"].as_i64().unwrap_or(0) > previous {
                    migrated.insert(folder, record);
                }
            }
            event.insert("legacy_carnival_records".to_owned(), json!(old));
            event.insert("carnival_records".to_owned(), json!(migrated.into_values().collect::<Vec<_>>()));
        }
        event.insert("record_schema_version".to_owned(), json!(2));
    }
    let Some(records) = event
        .get_mut("carnival_records")
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    for clear in clears
        .iter()
        .filter(|v| v.get("finished").and_then(Value::as_bool) == Some(true))
    {
        let Some(id) = clear.get("quest_id").and_then(Value::as_i64) else {
            continue;
        };
        let Some(quest) = fixture.quests.get(&format!("22:{id}")) else {
            continue;
        };
        if quest.carnival_event_id != Some(event_id) {
            continue;
        }
        let (Some(time), Some(limit), Some(difficulty), Some(folder)) = (
            clear.get("best_elapsed_time_ms").and_then(Value::as_i64),
            quest.carnival_time_limit_ms,
            quest.carnival_difficulty_score,
            quest.carnival_folder_id,
        ) else {
            continue;
        };
        let score = difficulty.saturating_add(limit.saturating_sub(time).max(0));
        if let Some(record) = records
            .iter_mut()
            .find(|r| r.get("folder_id").and_then(Value::as_i64) == Some(folder))
        {
            let previous = record
                .get("best_score")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if score > previous {
                record["best_score"] = Value::from(score);
            }
            // With exactly one clear in this folder, the saved best time is
            // also the previous run. Older snapshots do not retain the last
            // time after retries, so do not invent a previous score there.
            let folder_clears: i64 = clears.iter().filter(|c| {
                c.get("finished").and_then(Value::as_bool) == Some(true)
                    && c.get("quest_id").and_then(Value::as_i64)
                        .and_then(|id| fixture.quests.get(&format!("22:{id}")))
                        .is_some_and(|q| q.carnival_event_id == Some(event_id)
                            && q.carnival_folder_id == Some(folder))
            }).map(|c| c.get("single_clear_count").and_then(Value::as_i64).unwrap_or(1)).sum();
            if folder_clears == 1 && record.get("previous_score").and_then(Value::as_i64)
                .is_some_and(|old| old < difficulty)
            {
                record["previous_score"] = Value::from(score);
            }
        }
    }
    Ok(())
}

// //// 返回嘉年华活动队伍 [@x380kkm 2026-08-22] ////
fn get_party(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
    asset_root: &Path,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<PartyRequest>(request) {
        Ok(body) if body.viewer_id > 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let mut player = match state::load_player(database, body.viewer_id)? {
        Ok(player) => player,
        Err(response) => return Ok(response),
    };
    let current_event = state::current_event_id(player.root()?, FAMILY);
    let managed_event = state::managed_event_id(database, FAMILY)?;
    let event_id = body
        .event_id
        .filter(|event_id| *event_id > 0)
        .or(current_event)
        .or(managed_event);
    let has_activity_context = event_id.is_some();
    let event_id = event_id.unwrap_or(1);
    if has_activity_context {
        if let Some(response) =
            closed_activity_response(database, asset_root, &format!("{FAMILY}:{event_id}"))?
        {
            return Ok(response);
        }
    }
    let root = player.root_mut()?;
    if has_activity_context {
        state::set_current_event(root, FAMILY, event_id)?;
    }
    let party_groups = party::carnival_party_groups(root, FAMILY, event_id)?;
    player.save(database)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        server_time(database)?,
        json!({ "user_party_group_list": party_groups }),
    )
}
// //// /返回嘉年华活动队伍 ////

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_folder_contract_and_legacy_stage_two_record_are_preserved() {
        let fixture = crate::cn_battle_assets::load_battle_fixture().unwrap();
        let canonical: Value = serde_json::from_str(include_str!("../../assets/cn-carnival-quest-scores.json")).unwrap();
        for (id, value) in canonical.as_object().unwrap() {
            let quest = &fixture.quests[&format!("22:{id}")];
            assert_eq!(quest.carnival_folder_id, value["folder_id"].as_i64(), "quest {id}");
            assert_eq!(quest.carnival_legacy_folder_id, value["legacy_folder_id"].as_i64());
            assert_eq!(quest.carnival_difficulty_score, value["difficulty_score"].as_i64());
            assert_eq!(quest.carnival_time_limit_ms, value["time_limit_ms"].as_i64());
        }
        let mut root = json!({"quest_progress":{"22":[
            {"quest_id":100000004,"finished":true,"single_clear_count":1,"best_elapsed_time_ms":225377}
        ]}}).as_object().unwrap().clone();
        state::event_state_mut(&mut root, FAMILY, 100000).unwrap().insert("carnival_records".into(),
            json!([{"folder_id":4,"best_score":2574623,"previous_score":2574623,"previous_character_ids":[141001,141002,141003]}]));
        repair_legacy_scores(&mut root, 100000).unwrap();
        let event = state::event_state(&root, FAMILY, 100000).unwrap();
        assert_eq!(event["carnival_records"][0]["folder_id"], 2);
        assert_eq!(event["carnival_records"][0]["previous_score"], 2574623);
        assert_eq!(event["legacy_carnival_records"][0]["folder_id"], 4);
        let saved = root.clone();
        repair_legacy_scores(&mut root, 100000).unwrap();
        assert_eq!(root, saved);
    }

    #[test]
    fn legacy_single_clear_repairs_both_displayed_and_best_scores_once() {
        let mut root = json!({
            "quest_progress": {"22": [{"quest_id": 100000001,
                "finished": true, "single_clear_count": 1,
                "best_elapsed_time_ms": 289841}]}
        }).as_object().unwrap().clone();
        state::event_state_mut(&mut root, FAMILY, 100000).unwrap().insert(
            "carnival_records".to_owned(),
            json!([{"folder_id": 1, "best_score": 2000, "previous_score": 2000}]),
        );
        repair_legacy_scores(&mut root, 100000).unwrap();
        let records = &state::event_state(&root, FAMILY, 100000).unwrap()["carnival_records"];
        assert_eq!(records[0]["best_score"], 2510159);
        assert_eq!(records[0]["previous_score"], 2510159);
        let saved = root.clone();
        repair_legacy_scores(&mut root, 100000).unwrap();
        assert_eq!(root, saved);
        root["quest_progress"]["22"][0]["single_clear_count"] = json!(2);
        state::event_state_mut(&mut root, FAMILY, 100000).unwrap()["carnival_records"][0]["previous_score"] = json!(1900);
        repair_legacy_scores(&mut root, 100000).unwrap();
        assert_eq!(state::event_state(&root, FAMILY, 100000).unwrap()["carnival_records"][0]["previous_score"], 1900);
    }
}
