#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[path = "support/local_saves.rs"]
#[allow(dead_code)]
mod local_save_support;
#[allow(dead_code)]
mod support;

use local_save_support::*;
use serde_json::{json, Value};
use starpoint_personal_service::PersonalService;
use tempfile::TempDir;

fn new_slot(service: &PersonalService, name: &str) -> i64 {
    let response = authorized_request(
        service,
        "POST",
        "/v1/local-saves/new",
        Some(&json!({"name":name})),
    );
    assert_status(&response, "201 Created");
    response_body(&response)["id"].as_i64().unwrap()
}
fn editor(service: &PersonalService, slot: i64) -> Value {
    let response = authorized_request(
        service,
        "GET",
        &format!("/v1/local-saves/{slot}/editor"),
        None,
    );
    assert_status(&response, "200 OK");
    response_body(&response)
}
fn edit(service: &PersonalService, slot: i64, etag: &Value, resources: Value) -> String {
    authorized_request(
        service,
        "PATCH",
        &format!("/v1/local-saves/{slot}/resources"),
        Some(&json!({"expected_etag":etag,"resources":resources})),
    )
}

#[test]
fn beginner_save_starts_at_prologue_and_resumes_without_preclearing_quests() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    signup(service.port(), 78103);
    let original = list_local_saves(&service)["slots"][0]["id"].as_i64().unwrap();
    let original_data = export_local_save(&service, original)["data"].clone();
    let slot = new_slot(&service, "From the beginning");
    let fresh = export_local_save(&service, slot)["data"].clone();
    assert_eq!(fresh["quest_progress"], json!({}));
    assert_eq!(fresh["user_triggered_tutorial"], json!([]));
    assert_eq!(fresh["user_tutorial"]["tutorial_step"], 0);
    assert!(fresh["user_tutorial"]["skip_flag"].is_null());
    assert!(fresh["tutorial_gacha"].is_null());
    assert_eq!(fresh["user_info"]["rank_point"], 0);
    assert_eq!(fresh["user_info"]["name"], "玩家");
    // This 150 is reserved for the tutorial single draw, before the completion reward.
    assert_eq!(fresh["user_info"]["free_vmoney"], 150);
    assert_eq!(fresh["user_info"]["vmoney"], 0);
    assert_eq!(fresh["user_character_list"].as_object().unwrap().len(), 1);
    assert_eq!(fresh["user_character_list"]["1"]["exp"], 0);
    activate_local_save(&service, slot, 78103);
    let viewer = signup(service.port(), 78103).data_headers.viewer_id;
    let loaded = load(service.port(), viewer, "1.4.54");
    assert_eq!(loaded.data["user_tutorial"]["tutorial_step"], 0);
    assert_eq!(loaded.data["quest_progress"], json!({}));
    let step = cn_support::decode_response::<Value>(&cn_support::send_request(
        service.port(), "/api/index.php/tutorial/update_step",
        &cn_support::encode_request(&json!({"viewer_id":viewer,"step":0,"skip":false})),
    ));
    assert_eq!(step.data["step"], 1);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(service.port(), 78103).data_headers.viewer_id;
    let resumed = load(service.port(), viewer, "1.4.54");
    assert_eq!(resumed.data["user_tutorial"]["tutorial_step"], 1);
    assert_eq!(resumed.data["quest_progress"], json!({}));
    assert!(resumed.data.get("preserve_main_quest_progress").is_none());
    for step in 1..14 {
        let next = cn_support::decode_response::<Value>(&cn_support::send_request(
            service.port(), "/api/index.php/tutorial/update_step",
            &cn_support::encode_request(&json!({"viewer_id":viewer,"step":step,"skip":false})),
        ));
        assert_eq!(next.data["step"], step + 1);
    }
    let draw = cn_support::decode_response::<Value>(&cn_support::send_request(
        service.port(), "/api/index.php/tutorial/update_step",
        &cn_support::encode_request(&json!({"viewer_id":viewer,"step":14,"skip":false,"gacha_id":1})),
    ));
    assert_eq!(draw.data["step"], 15);
    assert_eq!(draw.data["user_info"]["free_vmoney"], 0);
    assert!(draw.data["gacha"]["draw"][0]["character_id"].as_i64().is_some());
    for _ in 0..2 {
        let finish = cn_support::decode_response::<Value>(&cn_support::send_request(
            service.port(), "/api/index.php/tutorial/update_step",
            &cn_support::encode_request(&json!({"viewer_id":viewer,"step":15,"skip":false})),
        ));
        assert_eq!(finish.data["step"], 16);
        assert_eq!(finish.data["user_info"]["free_vmoney"], 1500);
    }
    let triggered = cn_support::decode_response::<Value>(&cn_support::send_request(
        service.port(), "/api/index.php/tutorial/finish_trigger",
        &cn_support::encode_request(&json!({"viewer_id":viewer,"tutorial_ids":[12]})),
    ));
    assert_eq!(triggered.data, json!([]));
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(service.port(), 78103).data_headers.viewer_id;
    let completed = load(service.port(), viewer, "1.4.54");
    assert!(completed.data["user_tutorial"].is_null());
    assert_eq!(completed.data["quest_progress"], json!({}));
    assert_eq!(completed.data["user_info"]["free_vmoney"], 1500);
    assert_eq!(export_local_save(&service, slot)["data"]["preserve_main_quest_progress"], true);
    assert_eq!(export_local_save(&service, original)["data"], original_data);
    service.stop().unwrap();
}

#[test]
fn short_prologue_and_copied_beginner_save_do_not_unlock_main_story() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let slot = new_slot(&service, "Short prologue");
    signup(service.port(), 78104);
    activate_local_save(&service, slot, 78104);
    let viewer = signup(service.port(), 78104).data_headers.viewer_id;
    for step in 0..6 {
        let mut body = json!({"viewer_id":viewer,"step":step,"skip":true});
        if step == 3 { body["gacha_id"] = json!(1704); }
        let response = cn_support::decode_response::<Value>(&cn_support::send_request(
            service.port(), "/api/index.php/tutorial/update_step",
            &cn_support::encode_request(&body),
        ));
        assert_eq!(response.data["step"], step + 12);
    }
    cn_support::decode_response::<Value>(&cn_support::send_request(
        service.port(), "/api/index.php/tutorial/finish_trigger",
        &cn_support::encode_request(&json!({"viewer_id":viewer,"tutorial_ids":[12]})),
    ));
    assert_eq!(load(service.port(), viewer, "1.4.54").data["quest_progress"], json!({}));
    let copied = response_body(&authorized_request(
        &service, "POST", &format!("/v1/local-saves/{slot}/copy"),
        Some(&json!({"name":"Copy of beginner"})),
    ))["id"].as_i64().unwrap();
    activate_local_save(&service, copied, 78104);
    let viewer = signup(service.port(), 78104).data_headers.viewer_id;
    let copied_load = load(service.port(), viewer, "1.4.54");
    assert!(copied_load.data["user_tutorial"].is_null());
    assert_eq!(copied_load.data["quest_progress"], json!({}));
    service.stop().unwrap();
}

#[test]
fn resource_edit_is_validated_backed_up_and_persistent() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    signup(service.port(), 78101);
    let original = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let original_data = export_local_save(&service, original)["data"].clone();
    let slot = new_slot(&service, "Resource test");
    let before = editor(&service, slot);
    for invalid in [
        json!({"free_vmoney":-1}),
        json!({"vmoney":100000000}),
        json!({"free_mana":1.5}),
        json!({"free_mana":"20"}),
        json!({"rank":100}),
        json!({}),
    ] {
        assert_status(
            &edit(&service, slot, &before["etag"], invalid),
            "400 Bad Request",
        );
    }
    assert_eq!(editor(&service, slot), before);
    let resources = json!({"free_vmoney":123456,"vmoney":900,"free_mana":987654,"exp_pool":456789});
    assert_status(
        &edit(&service, slot, &before["etag"], resources.clone()),
        "200 OK",
    );
    assert_eq!(editor(&service, slot)["resources"], resources);
    assert_status(
        &edit(&service, slot, &before["etag"], json!({"free_vmoney":0})),
        "409 Conflict",
    );
    assert_eq!(export_local_save(&service, original)["data"], original_data);
    let snapshots = response_body(&authorized_request(
        &service,
        "GET",
        &format!("/v1/local-saves/{slot}/snapshots"),
        None,
    ));
    let snapshot_id = snapshots[0]["id"].as_i64().unwrap();
    assert_eq!(snapshots[0]["label"], "Before resource edit");
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert_eq!(editor(&service, slot)["resources"], resources);
    activate_local_save(&service, slot, 78101);
    let user = signup(service.port(), 78101).data_headers.viewer_id;
    let loaded = load(service.port(), user, "1.4.54");
    for (key, value) in resources.as_object().unwrap() {
        assert_eq!(&loaded.data["user_info"][key], value);
    }
    let restored = authorized_request(
        &service,
        "POST",
        &format!("/v1/local-saves/{slot}/snapshots/{snapshot_id}/restore"),
        None,
    );
    assert_status(&restored, "200 OK");
    assert_eq!(editor(&service, slot)["resources"], before["resources"]);
    service.stop().unwrap();
}

#[test]
fn deletes_only_inactive_save_and_restores_after_restart() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    signup(service.port(), 78102);
    let active = list_local_saves(&service)["slots"][0]["id"]
        .as_i64()
        .unwrap();
    let before_active = export_local_save(&service, active)["data"].clone();
    let active_editor = editor(&service, active);
    assert_status(
        &authorized_request(
            &service,
            "DELETE",
            &format!("/v1/local-saves/{active}"),
            Some(&json!({"expected_etag":active_editor["etag"]})),
        ),
        "409 Conflict",
    );
    let slot = new_slot(&service, "Delete test");
    let first = editor(&service, slot);
    assert_status(
        &edit(&service, slot, &first["etag"], json!({"free_vmoney":55555})),
        "200 OK",
    );
    let state = editor(&service, slot);
    assert_status(
        &authorized_request(
            &service,
            "GET",
            &format!("/v1/local-saves/{slot}/ai-teams"),
            None,
        ),
        "200 OK",
    );
    // Multiple revisions and AI references must be removed without violating foreign keys.
    let expected_data = export_local_save(&service, slot)["data"].clone();
    let current = editor(&service, slot);
    let response = authorized_request(
        &service,
        "DELETE",
        &format!("/v1/local-saves/{slot}"),
        Some(&json!({"expected_etag":first["etag"]})),
    );
    assert_status(&response, "409 Conflict");
    let response = authorized_request(
        &service,
        "DELETE",
        &format!("/v1/local-saves/{slot}"),
        Some(&json!({"expected_etag":current["etag"]})),
    );
    assert_status(&response, "200 OK");
    let backup = response_body(&response)["backup_id"].as_i64().unwrap();
    assert_status(
        &authorized_request(
            &service,
            "GET",
            &format!("/v1/local-saves/{slot}/export"),
            None,
        ),
        "404 Not Found",
    );
    assert_eq!(export_local_save(&service, active)["data"], before_active);
    service.stop().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    assert!(!list_local_saves(&service)["slots"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == slot));
    let deleted = response_body(&authorized_request(
        &service,
        "GET",
        "/v1/local-saves/deleted",
        None,
    ));
    assert_eq!(deleted["backups"].as_array().unwrap().len(), 1);
    let path = format!("/v1/local-saves/deleted/{backup}/restore");
    let restored = authorized_request(&service, "POST", &path, None);
    assert_status(&restored, "201 Created");
    let restored_id = response_body(&restored)["id"].as_i64().unwrap();
    assert_ne!(restored_id, slot);
    assert_eq!(
        export_local_save(&service, restored_id)["data"],
        expected_data
    );
    assert_eq!(
        editor(&service, restored_id)["resources"],
        state["resources"]
    );
    assert_eq!(
        response_body(&authorized_request(&service, "POST", &path, None))["id"],
        restored_id
    );
    let db = rusqlite::Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    service.stop().unwrap();
}

#[test]
fn rejects_edit_during_battle_and_invalid_credentials() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let slot = new_slot(&service, "Busy test");
    let state = editor(&service, slot);
    let path = format!("/v1/local-saves/{slot}/resources");
    let request =
        json!({"expected_etag":state["etag"],"resources":{"free_vmoney":123}}).to_string();
    let denied = support::request_with_headers(
        service.port(),
        "PATCH",
        &path,
        "application/json",
        &[("Authorization", "Bearer incorrect")],
        request.as_bytes(),
    );
    assert_status(&denied, "401 Unauthorized");
    let db = rusqlite::Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    db.execute("INSERT INTO active_single_quests (account_id, quest_id, category, use_boss_boost_point, use_boost_point, is_auto_start_mode)
        SELECT account_id,1001002,1,0,0,0 FROM local_save_slots WHERE id=?1",[slot]).unwrap();
    assert_status(
        &edit(&service, slot, &state["etag"], json!({"free_vmoney":123})),
        "409 Conflict",
    );
    assert_eq!(editor(&service, slot), state);
    service.stop().unwrap();
}
