// audience: internal
// # personal-service-cn-mission-tests
//
// 该文件验证 CN mission master 查询、响应契约、模式匹配更新和重启持久化.

#[path = "support/cn.rs"]
#[allow(dead_code)]
mod cn_support;
#[allow(dead_code)]
mod support;

use cn_support::{decode_response, encode_request, SignupData, SignupRequest};
use serde::Serialize;
use serde_json::Value;
use starpoint_personal_service::PersonalService;
use std::collections::BTreeSet;
use support::request_with_headers;
use tempfile::TempDir;

#[derive(Serialize)]
struct MissionCategory {
    category: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    character_id: Option<i64>,
}

#[derive(Serialize)]
struct GetMissionProgressRequest {
    viewer_id: i64,
    api_count: i64,
    category_list: Vec<MissionCategory>,
}

#[derive(Serialize)]
struct MissionParameter<'a> {
    progress_value: i64,
    mission_pattern: &'a str,
}

#[derive(Serialize)]
struct UpdateMissionProgressRequest<'a> {
    viewer_id: i64,
    api_count: i64,
    mission_param_list: Vec<MissionParameter<'a>>,
}

fn signup(service: &PersonalService, device_id: i64) -> i64 {
    decode_response::<SignupData>(&cn_support::send_request(
        service.port(),
        "/api/index.php/tool/signup",
        &encode_request(&SignupRequest { device_id }),
    ))
    .data_headers
    .viewer_id
}

fn create_mail(service: &PersonalService, viewer_id: i64) {
    let authorization_value = format!("Bearer {}", service.management_token());
    let authorization = [("Authorization", authorization_value.as_str())];
    let mail = format!(
        "{{\"viewer_id\":{viewer_id},\"title\":\"Mission mail\",\"body\":\"Mail state\",\"sender\":\"Admin\",\"rewards\":{{\"freeVmoney\":1}}}}"
    );
    let response = request_with_headers(
        service.port(),
        "POST",
        "/v1/mails",
        "application/json",
        &authorization,
        mail.as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 201 Created"));
}

fn get_progress(
    service: &PersonalService,
    viewer_id: i64,
    categories: Vec<MissionCategory>,
) -> Value {
    decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mission/get_mission_progress",
        &encode_request(&GetMissionProgressRequest {
            viewer_id,
            api_count: 1,
            category_list: categories,
        }),
    ))
    .data
}

fn mission<'a>(data: &'a Value, category: i64, mission_id: i64) -> &'a Value {
    data["mission_progress_list"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mission| {
            mission["mission_category"] == category && mission["mission_id"] == mission_id
        })
        .unwrap()
}

// //// 验证多类别 master 查询和角色觉醒筛选 [@x380kkm 2026-08-22] ////
#[test]
fn returns_master_categories_and_filtered_character_awake_missions() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 49);

    let categories = get_progress(
        &service,
        viewer_id,
        vec![
            MissionCategory {
                category: 1,
                character_id: None,
            },
            MissionCategory {
                category: 10,
                character_id: None,
            },
        ],
    );
    assert_eq!(
        categories
            .as_object()
            .expect("mission progress data is an object")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        [
            "character_list",
            "equipment_list",
            "item_list",
            "mail_arrived",
            "mission_progress_list",
            "user_info"
        ]
        .into_iter()
        .collect()
    );
    let progress = categories["mission_progress_list"].as_array().unwrap();
    assert_eq!(
        progress
            .iter()
            .filter(|mission| mission["mission_category"] == 1)
            .count(),
        120
    );
    assert_eq!(
        progress
            .iter()
            .filter(|mission| mission["mission_category"] == 10)
            .count(),
        2
    );
    assert!(progress
        .iter()
        .all(|mission| mission["stage"].as_i64().is_some_and(|stage| stage > 0)));

    let awake = get_progress(
        &service,
        viewer_id,
        vec![MissionCategory {
            category: 9,
            character_id: Some(1),
        }],
    );
    let awake_ids = awake["mission_progress_list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mission| mission["mission_id"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(awake_ids, vec![11, 12, 13, 14]);

    let repeated = get_progress(
        &service,
        viewer_id,
        vec![MissionCategory {
            category: 1,
            character_id: None,
        }],
    );
    let repeated_again = get_progress(
        &service,
        viewer_id,
        vec![MissionCategory {
            category: 1,
            character_id: None,
        }],
    );
    assert_eq!(repeated_again, repeated);
    service.stop().expect("service stops cleanly");
}
// //// /验证多类别 master 查询和角色觉醒筛选 ////

// //// 验证模式更新覆盖所有匹配任务并持久化 [@x380kkm 2026-08-23] ////
#[test]
fn updates_every_matching_master_mission_and_persists_latest_value() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 50);
    let updated = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mission/update_mission_progress",
        &encode_request(&UpdateMissionProgressRequest {
            viewer_id,
            api_count: 2,
            mission_param_list: vec![
                MissionParameter {
                    progress_value: 3,
                    mission_pattern: "get_item_count",
                },
                MissionParameter {
                    progress_value: 5,
                    mission_pattern: "get_item_count",
                },
                MissionParameter {
                    progress_value: 5,
                    mission_pattern: "max_combo_achievement",
                },
            ],
        }),
    ));
    assert_eq!(
        updated
            .data
            .as_object()
            .expect("mission update data is an object")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        ["degree_list", "mail_arrived", "mission_info"]
            .into_iter()
            .collect()
    );
    assert!(updated.data["mission_info"].as_array().unwrap().is_empty());
    assert!(updated.data["degree_list"].as_array().unwrap().is_empty());

    let progress = get_progress(
        &service,
        viewer_id,
        vec![
            MissionCategory {
                category: 1,
                character_id: None,
            },
            MissionCategory {
                category: 4,
                character_id: None,
            },
        ],
    );
    assert_eq!(mission(&progress, 1, 1)["progress_value"], 5);
    assert_eq!(mission(&progress, 4, 1574)["progress_value"], 5);
    assert_eq!(mission(&progress, 4, 1652)["progress_value"], 5);
    assert_eq!(mission(&progress, 4, 1500)["progress_value"], 5);
    service.stop().expect("service stops cleanly");

    let restarted = PersonalService::start(root.path(), 0).expect("service restarts");
    let restarted_viewer_id = signup(&restarted, 50);
    let persisted = get_progress(
        &restarted,
        restarted_viewer_id,
        vec![
            MissionCategory {
                category: 1,
                character_id: None,
            },
            MissionCategory {
                category: 4,
                character_id: None,
            },
        ],
    );
    assert_eq!(mission(&persisted, 1, 1)["progress_value"], 5);
    assert_eq!(mission(&persisted, 4, 1574)["progress_value"], 5);
    assert_eq!(mission(&persisted, 4, 1652)["progress_value"], 5);
    restarted.stop().expect("restarted service stops cleanly");
}
// //// /验证模式更新覆盖所有匹配任务并持久化 ////

// //// 验证任务查询和更新返回当前邮件状态 [@x380kkm 2026-08-25] ////
#[test]
fn returns_current_mail_arrival_state() {
    let root = TempDir::new().expect("temporary service directory is created");
    let service = PersonalService::start(root.path(), 0).expect("service starts");
    let viewer_id = signup(&service, 51);
    create_mail(&service, viewer_id);

    let progress = get_progress(
        &service,
        viewer_id,
        vec![MissionCategory {
            category: 1,
            character_id: None,
        }],
    );
    assert_eq!(progress["mail_arrived"], true);

    let updated = decode_response::<Value>(&cn_support::send_request(
        service.port(),
        "/api/index.php/mission/update_mission_progress",
        &encode_request(&UpdateMissionProgressRequest {
            viewer_id,
            api_count: 2,
            mission_param_list: vec![MissionParameter {
                progress_value: 1,
                mission_pattern: "home_tap_town_character_count",
            }],
        }),
    ));
    assert_eq!(updated.data["mail_arrived"], true);
    service.stop().expect("service stops cleanly");
}
// //// /验证任务查询和更新返回当前邮件状态 ////

fn snapshot(root: &TempDir) -> Value {
    let connection =
        rusqlite::Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    let value: String = connection
        .query_row("SELECT data_json FROM player_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&value).unwrap()
}

fn edit_snapshot(root: &TempDir, edit: impl FnOnce(&mut Value)) {
    let mut value = snapshot(root);
    edit(&mut value);
    let connection =
        rusqlite::Connection::open(root.path().join("personal-service.sqlite3")).unwrap();
    connection
        .execute(
            "UPDATE player_snapshots SET data_json = ?1",
            [serde_json::to_string(&value).unwrap()],
        )
        .unwrap();
}

fn set_time(service: &PersonalService, iso: &str) {
    let token = format!("Bearer {}", service.management_token());
    let response = request_with_headers(
        service.port(),
        "PUT",
        "/v1/time",
        "application/json",
        &[("Authorization", token.as_str())],
        serde_json::json!({"enabled":true,"iso":iso,"rate":1.0})
            .to_string()
            .as_bytes(),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
}

#[test]
fn action_subtypes_grant_real_rewards_once_and_daily_progress_resets() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 985);
    set_time(&service, "2019-12-02T10:00:00.000Z");
    let query = || {
        get_progress(
            &service,
            viewer,
            vec![MissionCategory {
                category: 2,
                character_id: None,
            }],
        )
    };
    let initial = query();
    let before = snapshot(&root);
    edit_snapshot(&root, |data| {
        data["user_info"]["total_dashes"] = Value::from(10)
    });
    let progress = query();
    assert_eq!(mission(&progress, 2, 4)["progress_value"], 10);
    assert_eq!(
        mission(&progress, 2, 5)["progress_value"].as_i64().unwrap(),
        mission(&initial, 2, 5)["progress_value"].as_i64().unwrap() + 1,
        "only one additional daily task was completed"
    );
    let rewarded = snapshot(&root);
    assert_eq!(
        rewarded["user_info"]["free_mana"].as_i64().unwrap(),
        before["user_info"]["free_mana"].as_i64().unwrap() + 1500
    );
    query();
    assert_eq!(
        snapshot(&root),
        rewarded,
        "reopening the page cannot pay twice"
    );
    set_time(&service, "2019-12-02T20:59:00.000Z");
    assert_eq!(mission(&query(), 2, 4)["progress_value"], 10);
    set_time(&service, "2019-12-02T21:00:00.000Z");
    let reset = query();
    assert_eq!(mission(&reset, 2, 4)["progress_value"], 0);
    assert_eq!(mission(&reset, 2, 5)["progress_value"], 0);
    edit_snapshot(&root, |data| {
        data["user_info"]["total_dashes"] = Value::from(20)
    });
    assert_eq!(mission(&query(), 2, 4)["progress_value"], 10);
    assert_eq!(
        snapshot(&root)["user_info"]["free_mana"].as_i64().unwrap(),
        before["user_info"]["free_mana"].as_i64().unwrap() + 3000
    );
    edit_snapshot(&root, |data| {
        data["user_info"]["total_dashes"] = Value::from(100);
        data["user_info"]["total_skills"] = Value::from(10);
        data["user_info"]["total_powerflips"] = Value::from(0);
    });
    let regular = get_progress(
        &service,
        viewer,
        vec![MissionCategory {
            category: 1,
            character_id: None,
        }],
    );
    assert_eq!(mission(&regular, 1, 3)["progress_value"], 100);
    assert_eq!(mission(&regular, 1, 8)["progress_value"], 10);
    assert_eq!(mission(&regular, 1, 7)["progress_value"], 0);
    let after = snapshot(&root);
    assert!(
        after["user_info"]["free_vmoney"].as_i64().unwrap()
            >= before["user_info"]["free_vmoney"].as_i64().unwrap() + 5
    );
    get_progress(
        &service,
        viewer,
        vec![MissionCategory {
            category: 1,
            character_id: None,
        }],
    );
    assert_eq!(snapshot(&root), after);
    service.stop().unwrap();
}

#[test]
fn chapter_completion_and_battle_mode_use_the_actual_contract() {
    let root = TempDir::new().unwrap();
    let service = PersonalService::start(root.path(), 0).unwrap();
    let viewer = signup(&service, 986);
    set_time(&service, "2019-12-02T10:00:00.000Z");
    edit_snapshot(&root, |data| {
        data["quest_progress"]["1"] = serde_json::json!([{"quest_id":1002002,"finished":true,"clear_rank":5,"ss_clear_count":3}]);
        data["user_info"]["total_fevers"] = Value::from(17);
        data["user_info"]["total_fevers_single"] = Value::from(5);
        data["user_info"]["total_fevers_multi"] = Value::from(12);
    });
    let query = || {
        get_progress(
            &service,
            viewer,
            vec![
                MissionCategory {
                    category: 1,
                    character_id: None,
                },
                MissionCategory {
                    category: 5,
                    character_id: None,
                },
            ],
        )
    };
    let data = query();
    assert_eq!(
        mission(&data, 1, 10)["progress_value"],
        0,
        "an early quest is not chapter completion"
    );
    assert_eq!(mission(&data, 1, 31)["progress_value"], 17);
    assert_eq!(
        mission(&data, 1, 2)["progress_value"],
        3,
        "SS uses client rank 5 and counts repeat clears"
    );
    assert_eq!(
        mission(&data, 5, 16000)["progress_value"],
        5,
        "solo fever title must not count multi battles"
    );
    edit_snapshot(&root, |data| {
        data["quest_progress"]["1"] = serde_json::json!([{"quest_id":1008004,"finished":true}])
    });
    let before = snapshot(&root);
    assert_eq!(mission(&query(), 1, 10)["progress_value"], 1);
    let paid = snapshot(&root);
    assert!(
        paid["user_info"]["free_vmoney"].as_i64().unwrap()
            >= before["user_info"]["free_vmoney"].as_i64().unwrap() + 150
    );
    query();
    assert_eq!(snapshot(&root), paid);
    service.stop().unwrap();
}
