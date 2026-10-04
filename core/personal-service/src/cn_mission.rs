// audience: internal
// # personal-service-cn-mission
//
// 该模块按 CN mission master 计算任务进度和阶段. 阶段奖励与领取记录使用同一事务提交.

mod action;
mod derived;
mod response;
pub(crate) use response::sync_reward_response;

pub(crate) use derived::{prepare_periods, prepare_snapshot_periods};

use crate::cn::{decode_request, msgpack_response_at, server_time};
use crate::cn_tutorial::{decode_player_data, encode_player_data, player_snapshot};
use crate::database::{ReceiveHistoryEntry, ServiceDatabase};
use crate::http::{HttpRequest, HttpResponse};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Number, Value};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub(crate) use action::{
    record_battle_action, record_bond_token_action, record_character_level_action,
    record_character_over_limit_action, record_equipment_upgrade_action,
    record_mana_board_open_action, record_mana_node_action, record_shop_purchase_active_missions,
    record_story_action,
};

const MAX_MISSION_PARAMETERS: usize = 100;
const MISSION_MASTER: &str = include_str!("../assets/cn-mission-master.json");
static MISSION_CATALOG: OnceLock<Result<MissionCatalog, String>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct MissionKey {
    category: i64,
    mission_id: i64,
}

struct MissionCatalog {
    chapter_final_quests: BTreeMap<(i64, i64), i64>,
    quest_ranks: BTreeMap<String, i64>,
    categories: BTreeMap<i64, Vec<MissionDefinition>>,
    pattern_index: BTreeMap<String, Vec<MissionKey>>,
    character_stories: BTreeMap<String, Vec<i64>>,
    rank_thresholds: Vec<RankThreshold>,
}

struct MissionDefinition {
    id: i64,
    pattern: Option<String>,
    degree_target: Option<i64>,
    quest_categories: Vec<i64>,
    quest_scope: Vec<Option<Vec<i64>>>,
    quest_rank_id: Option<i64>,
    enable_start_time: Option<i64>,
    enable_end_time: Option<i64>,
    battle_kind: Option<i64>,
    statistics_kind: Option<i64>,
    target_mission_ids: Vec<i64>,
    leader_character_id: Option<i64>,
    required_character_ids: Vec<i64>,
    required_races: Vec<String>,
    stages: Vec<MissionStage>,
}

#[derive(Deserialize)]
struct MissionMasterDocument {
    categories: BTreeMap<String, Vec<MissionDocument>>,
    character_stories: BTreeMap<String, Vec<i64>>,
    rank_thresholds: Vec<RankThreshold>,
}

#[derive(Deserialize)]
struct MissionDocument {
    id: i64,
    pattern: Option<String>,
    degree_target: Option<i64>,
    #[serde(default)]
    quest_categories: Vec<i64>,
    #[serde(default)]
    quest_scope: Vec<Option<Vec<i64>>>,
    #[serde(default)]
    quest_rank_id: Option<i64>,
    #[serde(default)]
    enable_start_time: Option<i64>,
    #[serde(default)]
    enable_end_time: Option<i64>,
    #[serde(default)]
    battle_kind: Option<i64>,
    #[serde(default)]
    statistics_kind: Option<i64>,
    #[serde(default)]
    target_mission_ids: Vec<i64>,
    #[serde(default)]
    leader_character_id: Option<i64>,
    #[serde(default)]
    required_character_ids: Vec<i64>,
    #[serde(default)]
    required_races: Vec<String>,
    stages: Vec<MissionStage>,
}

#[derive(Deserialize)]
struct MissionStage {
    stage: i64,
    reward_id: i64,
    target: Option<i64>,
    #[serde(default)]
    rewards: Vec<MissionReward>,
}

#[derive(Deserialize)]
pub(crate) struct MissionReward {
    pub(crate) kind: i64,
    pub(crate) amount: i64,
    pub(crate) item_id: Option<i64>,
    pub(crate) character_id: Option<i64>,
    pub(crate) equipment_id: Option<i64>,
    #[serde(default)]
    pub(crate) degree_id: Option<i64>,
}

#[derive(Deserialize)]
struct RankThreshold {
    degree: i64,
    threshold: i64,
}

struct ComputeContext {
    finished_quests: BTreeSet<i64>,
    quest_progress: BTreeMap<i64, Vec<QuestProgressEntry>>,
    character_clears: BTreeMap<String, i64>,
    leader_clears: BTreeMap<String, i64>,
    leader_multi_clears: BTreeMap<String, i64>,
    leader_powerflips: BTreeMap<String, i64>,
    co_clears: BTreeMap<String, i64>,
    race_clears: BTreeMap<String, i64>,
    rank_counts: BTreeMap<&'static str, i64>,
    rank_degree: i64,
    total_powerflips: i64,
    total_stamina_used: i64,
    total_stories: i64,
    statistics: BTreeMap<i64, i64>,
    mode_statistics: BTreeMap<(i64, i64), i64>,
    drop_mana: i64,
    period_baselines: BTreeMap<String, i64>,
}

struct QuestProgressEntry {
    ss_clear_count: i64,
    quest_id: i64,
    best_elapsed_time_ms: Option<i64>,
    leader_character_id: Option<i64>,
    multi_clear_count: Option<i64>,
    single_clear_count: Option<i64>,
}

#[derive(Deserialize)]
struct MissionCategory {
    category: i64,
    #[serde(default)]
    character_id: Option<Value>,
}

#[derive(Deserialize)]
struct GetMissionProgressRequest {
    viewer_id: i64,
    api_count: i64,
    category_list: Option<Vec<MissionCategory>>,
}

#[derive(Deserialize)]
struct MissionParameter {
    progress_value: i64,
    mission_pattern: String,
}

#[derive(Deserialize)]
struct UpdateMissionProgressRequest {
    viewer_id: i64,
    api_count: i64,
    mission_param_list: Vec<MissionParameter>,
}

// //// 分派 CN 任务进度请求 [@x380kkm 2026-07-24] ////
pub(crate) fn route(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Option<Result<HttpResponse, PersonalServiceError>> {
    if request.method() != "POST" {
        return None;
    }
    let response = match request.path() {
        "/api/index.php/mission/get_mission_progress" => get_progress(request, database),
        "/api/index.php/mission/update_mission_progress" => update_progress(request, database),
        _ => return None,
    };
    Some(response)
}
// //// /分派 CN 任务进度请求 ////

// //// 注入 CN 载入响应的角色觉醒任务摘要 [@x380kkm 2026-08-24] ////
pub(crate) fn inject_load_awake_summary(
    player_data: &mut Value,
    serialized_player_data: &str,
    database: &ServiceDatabase,
    account_id: i64,
) -> Result<(), PersonalServiceError> {
    let catalog = mission_catalog()?;
    let stored_progress = database.mission_progress(account_id)?;
    let counters = database.mission_counters(account_id)?;
    let internal_player_data =
        serde_json::from_str::<Value>(serialized_player_data).map_err(|error| {
            PersonalServiceError::new(format!(
                "failed to decode stored CN mission player data: {error}"
            ))
        })?;
    let internal_root = internal_player_data
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN mission player data is invalid"))?;
    let context = build_context(internal_root, catalog, &counters)?;
    let root = player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let owned_character_ids = root
        .get("user_character_list")
        .and_then(Value::as_object)
        .map(|characters| characters.keys().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();
    let mut missions_by_character = BTreeMap::<String, Vec<&MissionDefinition>>::new();
    for mission in catalog.categories.get(&9).into_iter().flatten() {
        let character_id = mission_character_id(mission.id);
        if owned_character_ids.contains(&character_id) {
            missions_by_character
                .entry(character_id)
                .or_default()
                .push(mission);
        }
    }

    let mut active_mission_list = Vec::new();
    let mut awakened_character_ids = Vec::new();
    for (character_id, missions) in missions_by_character {
        let mut all_complete = true;
        for mission in missions {
            let database_progress = stored_progress
                .get(&(9, mission.id))
                .copied()
                .unwrap_or_default();
            let progress =
                compute_progress(9, mission, &context, &counters, database_progress, catalog);
            let stages = mission
                .stages
                .iter()
                .map(|stage| {
                    let received = stage.target.is_some_and(|target| progress >= target);
                    if !received {
                        all_complete = false;
                    }
                    json!({"stage": stage.stage, "received": received})
                })
                .collect::<Vec<_>>();
            active_mission_list.push(json!({
                "mission_id": mission.id,
                "progress_value": progress,
                "stages": stages,
            }));
        }
        if all_complete {
            awakened_character_ids.push(character_id);
        }
    }

    root.insert(
        "active_mission_list".to_owned(),
        Value::Array(active_mission_list),
    );
    if let Some(characters) = root
        .get_mut("user_character_list")
        .and_then(Value::as_object_mut)
    {
        for character in characters.values_mut().filter_map(Value::as_object_mut) {
            character.remove("mana_board_awake");
        }
        for character_id in awakened_character_ids {
            if let Some(character) = characters
                .get_mut(&character_id)
                .and_then(Value::as_object_mut)
            {
                character.insert("mana_board_awake".to_owned(), json!({"1": 1}));
            }
        }
    }
    Ok(())
}
// //// /注入 CN 载入响应的角色觉醒任务摘要 ////

// //// 计算任务进度并结算新完成阶段 [@x380kkm 2026-08-22] ////
fn get_progress(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<GetMissionProgressRequest>(request) {
        Ok(body) if body.viewer_id > 0 && body.api_count >= 0 => body,
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let categories = body.category_list.unwrap_or_else(|| {
        vec![MissionCategory {
            category: 1,
            character_id: None,
        }]
    });
    if categories.iter().any(|entry| entry.category <= 0) {
        return Ok(error_response("400 Bad Request", "invalid_request_body"));
    }
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let catalog = mission_catalog()?;
    let stored_progress = database.mission_progress(snapshot.account_id)?;
    let counters = database.mission_counters(snapshot.account_id)?;
    let response_time = server_time(database)?;
    let mut player_data = decode_player_data(&snapshot.data)?;
    let root = player_data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    let period_changed = prepare_periods(root, database, snapshot.account_id, response_time)?;
    let context = build_context(root, catalog, &counters)?;
    let character_filters = category_character_filters(&categories);
    let mut mission_progress_list = Vec::new();
    let mut rewarded_progress = BTreeMap::new();
    let mut rewarded_receipts = Vec::new();
    let mut history_entries = Vec::new();
    let mut snapshot_changed = period_changed;

    for requested in &categories {
        let Some(missions) = catalog.categories.get(&requested.category) else {
            continue;
        };
        for mission in missions {
            if requested.category == 9
                && character_filters
                    .get(&requested.category)
                    .is_some_and(|character_id| mission_character_id(mission.id) != *character_id)
            {
                continue;
            }
            let key = MissionKey {
                category: requested.category,
                mission_id: mission.id,
            };
            let database_progress = stored_progress
                .get(&(key.category, key.mission_id))
                .copied()
                .unwrap_or_default();
            let progress = compute_progress(
                requested.category,
                mission,
                &context,
                &counters,
                database_progress,
                catalog,
            );
            mission_progress_list.push(json!({
                "mission_category": requested.category,
                "mission_id": mission.id,
                "progress_value": progress,
                "stage": current_stage(mission, progress),
            }));

            for stage in mission.stages.iter().filter(|stage| {
                mission_is_enabled(mission, response_time)
                    && stage.target.is_some_and(|target| progress >= target)
            }) {
                let receipt = derived::stage_receipt(key, stage.stage, response_time);
                if has_stage_receipt(root, &receipt)? {
                    continue;
                }
                history_entries.extend(apply_rewards(
                    root,
                    &stage.rewards,
                    body.viewer_id,
                    response_time,
                )?);
                mark_stage_received(root, receipt.clone())?;
                rewarded_receipts.push(receipt);
                rewarded_progress.insert((key.category, key.mission_id), progress);
                snapshot_changed = true;
            }
        }
    }

    if snapshot_changed {
        database.save_mission_rewards_with_receive_history(
            snapshot.account_id,
            &encode_player_data(&player_data)?,
            &rewarded_progress,
            &mission_reward_event_key(&rewarded_receipts),
            response_time,
            &history_entries,
        )?;
    }
    let mail_arrived = database.has_unreceived_mail(snapshot.account_id, response_time)?;
    let mut response = json!({
        "mission_progress_list": mission_progress_list,
        "mail_arrived": mail_arrived,
    });
    sync_reward_response(&mut response, &player_data, response_time);
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        response,
    )
}
// //// /计算任务进度并结算新完成阶段 ////

// //// 按 mission master 模式保存匹配任务进度 [@x380kkm 2026-08-22] ////
fn update_progress(
    request: &HttpRequest,
    database: &mut ServiceDatabase,
) -> Result<HttpResponse, PersonalServiceError> {
    let body = match decode_request::<UpdateMissionProgressRequest>(request) {
        Ok(body)
            if body.viewer_id > 0
                && body.api_count >= 0
                && body.mission_param_list.len() <= MAX_MISSION_PARAMETERS =>
        {
            body
        }
        Ok(_) | Err(_) => return Ok(error_response("400 Bad Request", "invalid_request_body")),
    };
    let snapshot = match player_snapshot(database, body.viewer_id)? {
        Ok(snapshot) => snapshot,
        Err(response) => return Ok(response),
    };
    let counters = match validated_pattern_values(body.mission_param_list) {
        Ok(counters) => counters,
        Err(response) => return Ok(response),
    };
    let catalog = mission_catalog()?;
    let mut mission_updates = BTreeMap::new();
    for (pattern, value) in &counters {
        if let Some(matches) = catalog.pattern_index.get(pattern) {
            for key in matches {
                let scoped = catalog
                    .categories
                    .get(&key.category)
                    .and_then(|missions| {
                        missions.iter().find(|mission| mission.id == key.mission_id)
                    })
                    .is_some_and(is_scoped_battle_mission);
                if scoped {
                    continue;
                }
                mission_updates.insert((key.category, key.mission_id), *value);
            }
        }
    }
    database.set_mission_progress(snapshot.account_id, &counters, &mission_updates)?;
    let response_time = server_time(database)?;
    let mail_arrived = database.has_unreceived_mail(snapshot.account_id, response_time)?;
    msgpack_response_at(
        body.viewer_id,
        false,
        response_time,
        json!({
            "mission_info": [],
            "degree_list": [],
            "mail_arrived": mail_arrived,
        }),
    )
}
// //// /按 mission master 模式保存匹配任务进度 ////

fn validated_pattern_values(
    parameters: Vec<MissionParameter>,
) -> Result<BTreeMap<String, i64>, HttpResponse> {
    let mut values = BTreeMap::new();
    for parameter in parameters {
        let pattern = parameter.mission_pattern.trim();
        if pattern.is_empty()
            || pattern.chars().count() > 128
            || pattern.chars().any(char::is_control)
            || parameter.progress_value < 0
        {
            return Err(error_response(
                "400 Bad Request",
                "invalid_mission_parameter",
            ));
        }
        values.insert(pattern.to_owned(), parameter.progress_value);
    }
    Ok(values)
}

fn mission_catalog() -> Result<&'static MissionCatalog, PersonalServiceError> {
    MISSION_CATALOG
        .get_or_init(build_mission_catalog)
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))
}

// //// 返回角色觉醒任务的阶段奖励 [@x380kkm 2026-08-24] ////
pub(crate) fn awake_mission_stage_rewards(
    mission_id: i64,
    stage: i64,
) -> Result<&'static [MissionReward], PersonalServiceError> {
    let rewards = mission_catalog()?
        .categories
        .get(&9)
        .and_then(|missions| missions.iter().find(|mission| mission.id == mission_id))
        .and_then(|mission| mission.stages.iter().find(|entry| entry.stage == stage))
        .map(|entry| entry.rewards.as_slice())
        .unwrap_or_default();
    Ok(rewards)
}
// //// /返回角色觉醒任务的阶段奖励 ////

fn build_mission_catalog() -> Result<MissionCatalog, String> {
    let document = serde_json::from_str::<MissionMasterDocument>(MISSION_MASTER)
        .map_err(|error| format!("failed to decode CN mission master: {error}"))?;
    let mut categories = BTreeMap::new();
    let mut pattern_index = BTreeMap::<String, Vec<MissionKey>>::new();
    for (category_text, missions) in document.categories {
        let category = category_text
            .parse::<i64>()
            .map_err(|error| format!("invalid CN mission category {category_text}: {error}"))?;
        let mut definitions = Vec::with_capacity(missions.len());
        for mission in missions {
            if let Some(pattern) = &mission.pattern {
                pattern_index
                    .entry(pattern.clone())
                    .or_default()
                    .push(MissionKey {
                        category,
                        mission_id: mission.id,
                    });
            }
            let mut stages = mission.stages;
            stages.sort_by(|left, right| match (left.target, right.target) {
                (Some(left), Some(right)) => left.cmp(&right),
                _ => Ordering::Equal,
            });
            definitions.push(MissionDefinition {
                id: mission.id,
                pattern: mission.pattern,
                degree_target: mission.degree_target,
                quest_categories: mission.quest_categories,
                quest_scope: mission.quest_scope,
                quest_rank_id: mission.quest_rank_id,
                enable_start_time: mission.enable_start_time,
                enable_end_time: mission.enable_end_time,
                battle_kind: mission.battle_kind,
                statistics_kind: mission.statistics_kind,
                target_mission_ids: mission.target_mission_ids,
                leader_character_id: mission.leader_character_id,
                required_character_ids: mission.required_character_ids,
                required_races: mission.required_races,
                stages,
            });
        }
        categories.insert(category, definitions);
    }
    Ok(MissionCatalog {
        chapter_final_quests: derived::chapter_final_quests()?,
        quest_ranks: serde_json::from_str(include_str!("../assets/cn-boss-quest-ranks.json"))
            .map_err(|error| format!("failed to decode CN quest ranks: {error}"))?,
        categories,
        pattern_index,
        character_stories: document.character_stories,
        rank_thresholds: document.rank_thresholds,
    })
}

fn category_character_filters(categories: &[MissionCategory]) -> BTreeMap<i64, String> {
    categories
        .iter()
        .filter_map(|entry| {
            entry
                .character_id
                .as_ref()
                .map(|character_id| (entry.category, value_text(character_id)))
        })
        .collect()
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_owned(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn build_context(
    root: &Map<String, Value>,
    catalog: &MissionCatalog,
    counters: &BTreeMap<String, i64>,
) -> Result<ComputeContext, PersonalServiceError> {
    let mut finished_quests = BTreeSet::new();
    let mut quest_progress = BTreeMap::<i64, Vec<QuestProgressEntry>>::new();
    let mut total_stories = 0;
    let mut rank_ss = 0;
    if let Some(categories) = root.get("quest_progress").and_then(Value::as_object) {
        for (category, entries) in categories {
            let Some(entries) = entries.as_array() else {
                continue;
            };
            let category_id = category.parse::<i64>().ok();
            for entry in entries {
                if entry.get("finished").and_then(Value::as_bool) != Some(true) {
                    continue;
                }
                if category == "3" {
                    total_stories += 1;
                }
                if let Some(quest_id) = entry.get("quest_id").and_then(Value::as_i64) {
                    finished_quests.insert(quest_id);
                    if let Some(category_id) = category_id {
                        quest_progress
                            .entry(category_id)
                            .or_default()
                            .push(QuestProgressEntry {
                                ss_clear_count: entry
                                    .get("ss_clear_count")
                                    .and_then(Value::as_i64)
                                    .unwrap_or_else(|| {
                                        i64::from(
                                            entry.get("clear_rank").and_then(Value::as_i64)
                                                == Some(5),
                                        )
                                    })
                                    .max(0),
                                quest_id,
                                best_elapsed_time_ms: entry
                                    .get("best_elapsed_time_ms")
                                    .and_then(Value::as_i64),
                                leader_character_id: entry
                                    .get("leader_character_id")
                                    .and_then(Value::as_i64),
                                multi_clear_count: entry
                                    .get("multi_clear_count")
                                    .and_then(Value::as_i64),
                                single_clear_count: entry
                                    .get("single_clear_count")
                                    .and_then(Value::as_i64),
                            });
                    }
                }
                match entry.get("clear_rank").and_then(Value::as_i64) {
                    Some(5) => {
                        rank_ss += entry
                            .get("ss_clear_count")
                            .and_then(Value::as_i64)
                            .unwrap_or(1)
                            .max(0)
                    }
                    _ => {}
                }
            }
        }
    }
    let user_info = root
        .get("user_info")
        .and_then(Value::as_object)
        .ok_or_else(|| PersonalServiceError::new("stored CN user_info data is missing"))?;
    let rank_point = user_info
        .get("rank_point")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let total_stamina_used = user_info
        .get("total_stamina_used")
        .or_else(|| user_info.get("stamina_used"))
        .and_then(Value::as_i64)
        .unwrap_or_else(|| {
            counters
                .iter()
                .filter(|(pattern, _)| pattern.as_str() == "used_stamina_count")
                .map(|(_, value)| *value)
                .max()
                .unwrap_or_default()
        });
    let total_powerflips = user_info
        .get("total_powerflips")
        .or_else(|| user_info.get("total_power_flips"))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    Ok(ComputeContext {
        finished_quests,
        quest_progress,
        character_clears: character_clear_counts(root),
        leader_clears: named_counts(root, "character_leader_clear_counts"),
        leader_multi_clears: named_counts(root, "character_leader_multi_clear_counts"),
        leader_powerflips: named_counts(root, "character_leader_power_flip_counts"),
        co_clears: named_counts(root, "party_member_co_clear_counts"),
        race_clears: named_counts(root, "party_race_clear_counts"),
        rank_counts: BTreeMap::from([("ss_rank_count", rank_ss)]),
        rank_degree: rank_degree(rank_point, &catalog.rank_thresholds),
        total_powerflips,
        total_stamina_used,
        total_stories,
        statistics: derived::statistics(root),
        mode_statistics: derived::mode_statistics(root),
        drop_mana: user_info
            .get("total_drop_mana")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        period_baselines: derived::period_baselines(root),
    })
}

fn character_clear_counts(root: &Map<String, Value>) -> BTreeMap<String, i64> {
    let mut counts = BTreeMap::new();
    for key in ["character_clear_counts", "player_character_clear_list"] {
        if let Some(entries) = root.get(key).and_then(Value::as_object) {
            for (character_id, value) in entries {
                if let Some(clear_count) = value
                    .as_i64()
                    .or_else(|| value.get("clear_count").and_then(Value::as_i64))
                {
                    counts.insert(character_id.clone(), clear_count);
                }
            }
        }
    }
    if let Some(characters) = root.get("user_character_list").and_then(Value::as_object) {
        for (character_id, character) in characters {
            if let Some(clear_count) = character.get("clear_count").and_then(Value::as_i64) {
                counts.insert(character_id.clone(), clear_count);
            }
        }
    }
    counts
}

fn named_counts(root: &Map<String, Value>, field: &str) -> BTreeMap<String, i64> {
    root.get(field)
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|(key, value)| value.as_i64().map(|count| (key.clone(), count)))
                .collect()
        })
        .unwrap_or_default()
}

fn rank_degree(rank_point: i64, thresholds: &[RankThreshold]) -> i64 {
    let mut degree = 1;
    for threshold in thresholds {
        if rank_point < threshold.threshold {
            break;
        }
        degree = threshold.degree;
    }
    degree
}

fn compute_progress(
    category: i64,
    mission: &MissionDefinition,
    context: &ComputeContext,
    counters: &BTreeMap<String, i64>,
    database_progress: i64,
    catalog: &MissionCatalog,
) -> i64 {
    derived::compute_progress(
        category,
        mission,
        context,
        counters,
        database_progress,
        catalog,
        0,
    )
}

fn compute_lifetime_progress(
    category: i64,
    mission: &MissionDefinition,
    context: &ComputeContext,
    counters: &BTreeMap<String, i64>,
    database_progress: i64,
    catalog: &MissionCatalog,
) -> i64 {
    if mission.pattern.as_deref() == Some("player_rank_achievement") {
        return context.rank_degree;
    }
    if category == 9 {
        return awake_progress(mission.id, context, catalog);
    }
    if mission.pattern.as_deref() == Some("chapter_clear") {
        return derived::chapter_progress(mission, context, catalog);
    }
    if mission.pattern.as_deref() == Some("ss_rank_count") {
        return context
            .quest_progress
            .iter()
            .filter(|(category, _)| {
                mission.quest_categories.is_empty() || mission.quest_categories.contains(category)
            })
            .flat_map(|(_, entries)| entries)
            .filter(|entry| quest_matches_scope(mission, entry.quest_id))
            .map(|entry| entry.ss_clear_count)
            .fold(0, i64::saturating_add);
    }
    if is_scoped_battle_mission(mission) {
        return scoped_battle_progress(mission, context, counters, catalog);
    }
    if mission.pattern.as_deref() == Some("battle_zone_statistics_count") {
        return if mission.quest_categories.is_empty() {
            mission
                .statistics_kind
                .and_then(|kind| match mission.battle_kind {
                    Some(mode @ (1 | 2)) => context.mode_statistics.get(&(mode, kind)),
                    _ => context.statistics.get(&kind),
                })
                .copied()
                .unwrap_or(0)
        } else {
            database_progress
        };
    }
    if mission.pattern.as_deref() == Some("total_attained_drop_mana_count") {
        return context.drop_mana;
    }
    if category == 1 || category == 2 {
        if let Some(pattern) = mission.pattern.as_deref() {
            if pattern == "single_battle_clear_count" {
                return context
                    .quest_progress
                    .iter()
                    .filter(|(category, _)| **category != 3)
                    .map(|(category, _)| battle_progress_for_category(context, pattern, *category))
                    .fold(0, i64::saturating_add)
                    .max(counters.get(pattern).copied().unwrap_or(0));
            }
            if pattern == "used_stamina_count" {
                return context.total_stamina_used;
            }
            if let Some(progress) = context.rank_counts.get(pattern) {
                return *progress;
            }
        }
    }
    mission
        .pattern
        .as_ref()
        .and_then(|pattern| counters.get(pattern))
        .copied()
        .unwrap_or(database_progress)
        .max(database_progress)
}

fn is_scoped_battle_mission(mission: &MissionDefinition) -> bool {
    !mission.quest_categories.is_empty()
        && matches!(
            mission.pattern.as_deref(),
            Some("single_battle_clear_count") | Some("multi_battle_clear_count")
        )
}

// //// 计算任务关卡范围内的战斗次数 [@x380kkm 2026-08-29] ////
fn scoped_battle_progress(
    mission: &MissionDefinition,
    context: &ComputeContext,
    counters: &BTreeMap<String, i64>,
    catalog: &MissionCatalog,
) -> i64 {
    let pattern = mission.pattern.as_deref().unwrap_or_default();
    if mission.quest_scope.iter().any(Option::is_some) || mission.quest_rank_id.is_some() {
        return mission
            .quest_categories
            .iter()
            .flat_map(|category| {
                context
                    .quest_progress
                    .get(category)
                    .into_iter()
                    .flatten()
                    .filter_map(move |entry| {
                        (quest_matches_scope(mission, entry.quest_id)
                            && mission.quest_rank_id.map_or(true, |rank| {
                                catalog
                                    .quest_ranks
                                    .get(&format!("{category}:{}", entry.quest_id))
                                    == Some(&rank)
                            }))
                        .then(|| {
                            counters
                                .get(&quest_battle_counter_key(
                                    pattern,
                                    *category,
                                    entry.quest_id,
                                ))
                                .copied()
                                .unwrap_or(0)
                                .max(quest_clear_count(
                                    entry,
                                    pattern,
                                    counters
                                        .get(&quest_battle_counter_key(
                                            "multi_battle_clear_count",
                                            *category,
                                            entry.quest_id,
                                        ))
                                        .copied()
                                        .unwrap_or(0),
                                ))
                        })
                    })
            })
            .fold(0, i64::saturating_add);
    }
    mission
        .quest_categories
        .iter()
        .map(|quest_category| {
            counters
                .get(&scoped_battle_counter_key(pattern, *quest_category))
                .copied()
                .unwrap_or(0)
                .max(battle_progress_for_category(
                    context,
                    pattern,
                    *quest_category,
                ))
        })
        .fold(0, i64::saturating_add)
}

pub(super) fn scoped_battle_counter_key(pattern: &str, quest_category: i64) -> String {
    format!("{pattern}:quest_category:{quest_category}")
}

// Match the client's QuestRangeReferenceIdKindTools key decomposition. None is
// a wildcard; an explicitly empty master list matches no quest.
fn quest_matches_scope(mission: &MissionDefinition, quest_id: i64) -> bool {
    let mut rest = quest_id;
    let mut keys = vec![0; mission.quest_scope.len()];
    for index in (0..keys.len()).rev() {
        keys[index] = if index == 0 { rest } else { rest % 1000 };
        rest /= 1000;
    }
    mission.quest_scope.iter().zip(keys).all(|(scope, key)| {
        scope
            .as_ref()
            .map_or(true, |allowed| allowed.contains(&key))
    })
}

pub(super) fn quest_battle_counter_key(pattern: &str, category: i64, quest_id: i64) -> String {
    format!("{pattern}:quest:{category}:{quest_id}")
}

fn mission_is_enabled(mission: &MissionDefinition, now: i64) -> bool {
    mission.enable_start_time.map_or(true, |start| now >= start)
        && mission.enable_end_time.map_or(true, |end| now <= end)
}

fn battle_progress_for_category(
    context: &ComputeContext,
    pattern: &str,
    quest_category: i64,
) -> i64 {
    context
        .quest_progress
        .get(&quest_category)
        .map_or(0, |entries| {
            entries.iter().fold(0, |total, entry| {
                total.saturating_add(quest_clear_count(entry, pattern, 0))
            })
        })
}

fn quest_clear_count(entry: &QuestProgressEntry, pattern: &str, known_multi: i64) -> i64 {
    if pattern == "multi_battle_clear_count" {
        entry.multi_clear_count.unwrap_or(0).max(0)
    } else {
        entry
            .single_clear_count
            .unwrap_or_else(|| i64::from(entry.multi_clear_count.unwrap_or(known_multi) == 0))
            .max(0)
    }
}
// //// /计算任务关卡范围内的战斗次数 ////

fn awake_progress(mission_id: i64, context: &ComputeContext, catalog: &MissionCatalog) -> i64 {
    let Some(mission) = catalog
        .categories
        .get(&9)
        .and_then(|missions| missions.iter().find(|mission| mission.id == mission_id))
    else {
        return 0;
    };
    if let Some(progress) = quest_clear_progress(mission_id, context) {
        return progress;
    }
    let character_id = mission_character_id(mission_id);
    let clear_count = context
        .character_clears
        .get(&character_id)
        .copied()
        .unwrap_or_default();
    if mission.pattern.as_deref() == Some("battle_clear_with_specific_races")
        && !mission.required_races.is_empty()
    {
        let race_key = mission
            .required_races
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join("+");
        return context
            .race_clears
            .get(&race_key)
            .copied()
            .unwrap_or_default();
    }
    if mission.pattern.as_deref() == Some("battle_clear_with_specific_characters") {
        let mut required_character_ids = mission.required_character_ids.clone();
        if let Some(leader_character_id) = mission.leader_character_id {
            if !required_character_ids.contains(&leader_character_id) {
                required_character_ids.push(leader_character_id);
            }
        }
        if required_character_ids.len() >= 2 {
            let co_clears = minimum_co_clears(&required_character_ids, &context.co_clears);
            return mission
                .leader_character_id
                .map_or(co_clears, |leader_character_id| {
                    co_clears.min(
                        context
                            .leader_clears
                            .get(&leader_character_id.to_string())
                            .copied()
                            .unwrap_or_default(),
                    )
                });
        }
        if let Some(leader_character_id) = mission.leader_character_id {
            let counts = if mission.battle_kind == Some(2) {
                &context.leader_multi_clears
            } else {
                &context.leader_clears
            };
            return counts
                .get(&leader_character_id.to_string())
                .copied()
                .unwrap_or_default();
        }
        if let Some(character_id) = required_character_ids.first() {
            return context
                .character_clears
                .get(&character_id.to_string())
                .copied()
                .unwrap_or_default();
        }
    }
    if mission.pattern.as_deref() == Some("battle_zone_statistics_count")
        && mission.statistics_kind == Some(1)
    {
        if let Some(leader_character_id) = mission.leader_character_id {
            return context
                .leader_powerflips
                .get(&leader_character_id.to_string())
                .copied()
                .unwrap_or_default();
        }
    }
    if let Some(leader_character_id) = mission.leader_character_id {
        let counts = if mission.battle_kind == Some(2) {
            &context.leader_multi_clears
        } else {
            &context.leader_clears
        };
        return counts
            .get(&leader_character_id.to_string())
            .copied()
            .unwrap_or_default();
    }
    match mission_id.rem_euclid(10) {
        1 => match catalog.character_stories.get(&character_id) {
            Some(stories) if !stories.is_empty() => stories
                .iter()
                .filter(|quest_id| context.finished_quests.contains(quest_id))
                .count() as i64,
            _ => clear_count,
        },
        2 if character_id == "1" => context.total_stories,
        2 => clear_count,
        3 if character_id == "1" => context.total_powerflips,
        3 => clear_count,
        4 => [mission_id - 3, mission_id - 2, mission_id - 1]
            .into_iter()
            .map(|mission_id| i64::from(awake_progress(mission_id, context, catalog) >= 1))
            .sum(),
        _ => 0,
    }
}

fn minimum_co_clears(character_ids: &[i64], co_clears: &BTreeMap<String, i64>) -> i64 {
    let mut minimum = None;
    for (index, character_id) in character_ids.iter().enumerate() {
        for other_character_id in &character_ids[index + 1..] {
            let key = if character_id < other_character_id {
                format!("{character_id}_{other_character_id}")
            } else {
                format!("{other_character_id}_{character_id}")
            };
            let count = co_clears.get(&key).copied().unwrap_or_default();
            minimum = Some(minimum.map_or(count, |current: i64| current.min(count)));
        }
    }
    minimum.unwrap_or_default()
}

fn quest_clear_progress(mission_id: i64, context: &ComputeContext) -> Option<i64> {
    let (category, quest_ids, time_limit_ms, leader_character_id) = match mission_id {
        1_110_013 => (2, &[1_028_004][..], None, Some(111_001)),
        1_310_052 => (15, &[96][..], None, Some(131_005)),
        1_410_032 => (2, &[1_020_003][..], None, None),
        2_110_013 => (2, &[1_028_004][..], None, Some(211_001)),
        2_310_013 => (2, &[1_010_004][..], Some(90_000), Some(231_001)),
        2_510_032 => (
            13,
            &[1_020, 1_023, 1_026, 1_029, 1_032, 1_035, 1_038][..],
            None,
            Some(251_003),
        ),
        2_510_033 => (
            13,
            &[1_020, 1_023, 1_026, 1_029, 1_032, 1_035, 1_038][..],
            Some(180_000),
            Some(251_003),
        ),
        2_630_023 => (19, &[100_100_004, 100_401_004][..], None, Some(151_006)),
        _ => return None,
    };
    let accomplished = context
        .quest_progress
        .get(&category)
        .into_iter()
        .flatten()
        .any(|progress| {
            quest_ids.contains(&progress.quest_id)
                && time_limit_ms.map_or(true, |limit| {
                    progress
                        .best_elapsed_time_ms
                        .is_some_and(|elapsed_time| elapsed_time <= limit)
                })
                && leader_character_id.map_or(true, |leader_character_id| {
                    progress.leader_character_id == Some(leader_character_id)
                })
        });
    Some(i64::from(accomplished))
}

fn mission_character_id(mission_id: i64) -> String {
    let text = mission_id.to_string();
    text.get(..text.len().saturating_sub(1))
        .unwrap_or(&text)
        .to_owned()
}

fn current_stage(mission: &MissionDefinition, progress: i64) -> i64 {
    let mut current = mission.stages.last().map_or(1, |stage| stage.stage);
    for stage in &mission.stages {
        if stage.target.is_some_and(|target| progress < target) {
            current = stage.stage;
            break;
        }
    }
    current
}

fn has_stage_receipt(
    root: &Map<String, Value>,
    receipt: &str,
) -> Result<bool, PersonalServiceError> {
    let Some(receipts) = root.get("mission_stage_receipts") else {
        return Ok(false);
    };
    let receipts = receipts
        .as_object()
        .ok_or_else(|| PersonalServiceError::new("stored CN mission stage receipts are invalid"))?;
    Ok(receipts
        .get(receipt)
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

fn mark_stage_received(
    root: &mut Map<String, Value>,
    receipt: String,
) -> Result<(), PersonalServiceError> {
    root.entry("mission_stage_receipts".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN mission stage receipts are invalid"))?
        .insert(receipt, Value::Bool(true));
    Ok(())
}

fn mission_reward_event_key(receipts: &[String]) -> String {
    let digest = Sha256::digest(receipts.join(",").as_bytes());
    format!("mission:{digest:x}")
}

fn apply_rewards(
    root: &mut Map<String, Value>,
    rewards: &[MissionReward],
    viewer_id: i64,
    response_time: i64,
) -> Result<Vec<ReceiveHistoryEntry>, PersonalServiceError> {
    let mut history_entries = Vec::new();
    for reward in rewards {
        match reward.kind {
            0 => {
                add_object_amount(root, "user_info", "free_vmoney", reward.amount)?;
                history_entries.push(ReceiveHistoryEntry::reward(4, None, reward.amount));
            }
            1 => {
                if let Some(item_id) = reward.item_id.or(reward.equipment_id) {
                    add_object_amount(root, "item_list", &item_id.to_string(), reward.amount)?;
                    let kind = if reward.equipment_id.is_some() { 6 } else { 1 };
                    history_entries.push(ReceiveHistoryEntry::reward(
                        kind,
                        Some(item_id),
                        reward.amount,
                    ));
                }
            }
            2 => {
                if let Some(id) = reward.equipment_id {
                    let equipment = required_object(root, "user_equipment_list")?;
                    let owned = equipment.contains_key(&id.to_string());
                    let entry = equipment.entry(id.to_string()).or_insert_with(
                        || json!({"enhancement_level":0,"level":1,"protection":false,"stack":0}),
                    );
                    let count = entry
                        .get("stack")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                        .checked_add(reward.amount - i64::from(!owned))
                        .ok_or_else(|| {
                            PersonalServiceError::new("CN mission equipment stack exceeds range")
                        })?;
                    entry["stack"] = Value::from(count.max(0));
                    history_entries.push(ReceiveHistoryEntry::reward(6, Some(id), reward.amount));
                }
            }
            3 => {
                add_object_amount(root, "user_info", "free_mana", reward.amount)?;
                history_entries.push(ReceiveHistoryEntry::reward(8, None, reward.amount));
            }
            4 => {
                if let Some(character_id) = reward.character_id {
                    for _ in 0..reward.amount {
                        crate::cn_character_reward::grant_character(
                            root,
                            viewer_id,
                            character_id,
                            response_time,
                        )?;
                    }
                    history_entries.push(ReceiveHistoryEntry::reward(
                        5,
                        Some(character_id),
                        reward.amount,
                    ));
                }
            }
            5 => {
                add_object_amount(root, "user_info", "exp_pool", reward.amount)?;
                history_entries.push(ReceiveHistoryEntry::reward(9, None, reward.amount));
            }
            6 => {
                if let Some(id) = reward.degree_id {
                    let degrees = root
                        .entry("earned_degree_ids".to_owned())
                        .or_insert_with(|| json!([]))
                        .as_array_mut()
                        .ok_or_else(|| {
                            PersonalServiceError::new("stored CN degree list is invalid")
                        })?;
                    if !degrees.contains(&Value::from(id)) {
                        degrees.push(Value::from(id));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(history_entries)
}

fn add_object_amount(
    root: &mut Map<String, Value>,
    object_key: &str,
    value_key: &str,
    amount: i64,
) -> Result<(), PersonalServiceError> {
    let object = required_object(root, object_key)?;
    let current = object
        .get(value_key)
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let updated = current.checked_add(amount).ok_or_else(|| {
        PersonalServiceError::new(format!(
            "CN mission reward {value_key} exceeds the supported range"
        ))
    })?;
    object.insert(value_key.to_owned(), Value::Number(Number::from(updated)));
    Ok(())
}

fn required_object<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, PersonalServiceError> {
    root.get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| PersonalServiceError::new(format!("stored CN {key} data is missing")))
}

fn error_response(status: &'static str, code: &str) -> HttpResponse {
    HttpResponse::json(status, format!("{{\"error\":\"{code}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_rank_mission_uses_persisted_rank_without_client_counter() {
        let catalog = mission_catalog().unwrap();
        let root = Map::from_iter([("user_info".to_owned(), json!({"rank_point": 2_000_044}))]);
        let counters = BTreeMap::new();
        let context = build_context(&root, catalog, &counters).unwrap();
        let mission = catalog.categories[&1].iter().find(|m| m.id == 22).unwrap();
        assert_eq!(compute_progress(1, mission, &context, &counters, 0, catalog), 122);
        assert_eq!(current_stage(mission, 122), 13);
    }

    #[test]
    fn boss_missions_require_the_actual_difficulty() {
        let catalog = mission_catalog().unwrap();
        let mission = catalog.categories[&5]
            .iter()
            .find(|m| m.id == 11050)
            .unwrap();
        assert_eq!(mission.quest_rank_id, Some(4));
        let mut root = Map::from_iter([
            ("user_info".to_owned(), json!({"rank_point": 0})),
            (
                "quest_progress".to_owned(),
                json!({"2": [
                    {"quest_id": 1014001, "finished": true},
                    {"quest_id": 1014003, "finished": true}
                ]}),
            ),
        ]);
        let counters = BTreeMap::new();
        let context = build_context(&root, catalog, &counters).unwrap();
        assert_eq!(
            compute_progress(5, mission, &context, &counters, 99, catalog),
            0
        );
        root["quest_progress"]["2"]
            .as_array_mut()
            .unwrap()
            .push(json!({"quest_id": 1014004, "finished": true}));
        let context = build_context(&root, catalog, &counters).unwrap();
        assert_eq!(
            compute_progress(5, mission, &context, &counters, 99, catalog),
            1
        );
    }

    #[test]
    fn mission_windows_follow_cn_android_utc_plus_eight_boundaries() {
        let catalog = mission_catalog().unwrap();
        let mission = catalog.categories[&2].iter().find(|m| m.id == 1).unwrap();
        // CN boot uses 28800000 ms despite the legacy "JAPAN_STANDARD" name.
        assert_eq!(mission.enable_start_time, Some(1574913600));
        assert_eq!(mission.enable_end_time, Some(1582318799));
        assert!(!mission_is_enabled(mission, 1574913600 - 1));
        assert!(mission_is_enabled(mission, 1574913600));
        assert!(mission_is_enabled(mission, 1582318799));
        assert!(!mission_is_enabled(mission, 1582318799 + 1));
    }

    #[test]
    fn specific_quest_missions_do_not_inherit_global_or_other_quest_progress() {
        let catalog = mission_catalog().unwrap();
        let mission = catalog.categories[&1].iter().find(|m| m.id == 42).unwrap();
        assert!(!quest_matches_scope(mission, 1_002_002));
        assert!(!quest_matches_scope(mission, 2_004_002));
        assert!(quest_matches_scope(mission, 1_004_002));
        let mut root = Map::from_iter([
            ("user_info".to_owned(), json!({"rank_point": 0})),
            (
                "quest_progress".to_owned(),
                json!({"1": [{"quest_id": 1_002_002, "finished": true}]}),
            ),
        ]);
        let mut counters = BTreeMap::from([
            ("single_battle_clear_count".to_owned(), 99),
            (
                scoped_battle_counter_key("single_battle_clear_count", 1),
                99,
            ),
        ]);
        let context = build_context(&root, catalog, &counters).unwrap();
        assert_eq!(
            compute_progress(1, mission, &context, &counters, 99, catalog),
            0
        );
        root["quest_progress"]["1"]
            .as_array_mut()
            .unwrap()
            .push(json!({"quest_id": 1_004_002, "finished": true}));
        let context = build_context(&root, catalog, &counters).unwrap();
        assert_eq!(
            compute_progress(1, mission, &context, &counters, 99, catalog),
            1
        );
        counters.insert(
            quest_battle_counter_key("single_battle_clear_count", 1, 1_004_002),
            3,
        );
        assert_eq!(
            compute_progress(1, mission, &context, &counters, 99, catalog),
            3
        );
        let boss = catalog.categories[&3]
            .iter()
            .find(|m| m.id == 1416)
            .unwrap();
        assert!(!quest_matches_scope(boss, 1_001_001));
        assert!(quest_matches_scope(boss, 1_014_001));
    }

    #[test]
    fn portable_quest_counts_preserve_battle_mode_and_imported_progress() {
        let catalog = mission_catalog().unwrap();
        let mission = catalog.categories[&1].iter().find(|m| m.id == 42).unwrap();
        let mut root = Map::from_iter([
            ("user_info".to_owned(), json!({"rank_point": 0})),
            (
                "quest_progress".to_owned(),
                json!({"1": [{
                    "quest_id": 1_004_002, "finished": true,
                    "single_clear_count": 0, "multi_clear_count": 3
                }]}),
            ),
        ]);
        let mut counters = BTreeMap::new();
        let context = build_context(&root, catalog, &counters).unwrap();
        assert_eq!(
            compute_progress(1, mission, &context, &counters, 99, catalog),
            0
        );
        assert_eq!(
            battle_progress_for_category(&context, "single_battle_clear_count", 1),
            0
        );
        assert_eq!(
            battle_progress_for_category(&context, "multi_battle_clear_count", 1),
            3
        );
        root["quest_progress"]["1"][0]["single_clear_count"] = json!(4);
        counters.insert(
            quest_battle_counter_key("single_battle_clear_count", 1, 1_004_002),
            1,
        );
        let context = build_context(&root, catalog, &counters).unwrap();
        assert_eq!(
            compute_progress(1, mission, &context, &counters, 99, catalog),
            4
        );
        // Legacy snapshots with known multiplayer history must not imply a solo clear.
        root["quest_progress"]["1"][0]
            .as_object_mut()
            .unwrap()
            .remove("single_clear_count");
        root["quest_progress"]["1"][0]
            .as_object_mut()
            .unwrap()
            .remove("multi_clear_count");
        counters.clear();
        counters.insert(
            quest_battle_counter_key("multi_battle_clear_count", 1, 1_004_002),
            3,
        );
        let context = build_context(&root, catalog, &counters).unwrap();
        assert_eq!(
            compute_progress(1, mission, &context, &counters, 99, catalog),
            0
        );
    }

    // //// 应用任务等级目标并保证阶段奖励幂等 [@x380kkm 2026-08-23] ////
    #[test]
    fn applies_mission_rewards_once_per_receipt() {
        let mut root = Map::from_iter([
            ("item_list".to_owned(), json!({})),
            (
                "user_info".to_owned(),
                json!({"free_mana": 10, "exp_pool": 20}),
            ),
            ("user_character_list".to_owned(), json!({})),
        ]);
        let catalog = mission_catalog().unwrap();
        let rank_mission = catalog.categories[&5]
            .iter()
            .find(|mission| mission.id == 1_000)
            .unwrap();
        assert_eq!(rank_mission.degree_target, Some(50));
        let mission = catalog.categories[&5]
            .iter()
            .find(|mission| mission.id == 11_010)
            .unwrap();
        let stage = mission
            .stages
            .iter()
            .find(|stage| stage.stage == 1)
            .unwrap();
        assert_eq!(stage.target, Some(1));
        let receipt = "5:11010:1";
        if !has_stage_receipt(&root, receipt).unwrap() {
            apply_rewards(&mut root, &stage.rewards, 1, 1).unwrap();
            mark_stage_received(&mut root, receipt.to_owned()).unwrap();
        }
        if !has_stage_receipt(&root, receipt).unwrap() {
            apply_rewards(&mut root, &stage.rewards, 1, 1).unwrap();
            mark_stage_received(&mut root, receipt.to_owned()).unwrap();
        }
        assert_eq!(root["earned_degree_ids"], json!([11010]));
        assert_eq!(root["item_list"], json!({}));
        assert_eq!(root["user_info"]["free_mana"], 10);
    }
    // //// /应用任务等级目标并保证阶段奖励幂等 ////

    // //// 按战斗队伍和动作统计计算觉醒任务 [@x380kkm 2026-08-25] ////
    #[test]
    fn computes_awake_progress_from_battle_contract() {
        let root = Map::from_iter([
            (
                "quest_progress".to_owned(),
                json!({
                    "2": [{
                        "quest_id": 1_028_004,
                        "finished": true,
                        "best_elapsed_time_ms": 80_000,
                        "leader_character_id": 111_001,
                    }],
                }),
            ),
            ("user_info".to_owned(), json!({"rank_point": 0})),
            (
                "character_clear_counts".to_owned(),
                json!({"151006": 3, "161002": 4}),
            ),
            (
                "character_leader_clear_counts".to_owned(),
                json!({"151006": 3, "161002": 4}),
            ),
            (
                "character_leader_multi_clear_counts".to_owned(),
                json!({"131005": 2}),
            ),
            (
                "character_leader_power_flip_counts".to_owned(),
                json!({"121001": 7}),
            ),
            (
                "party_member_co_clear_counts".to_owned(),
                json!({"151006_263002": 2, "211001_231001": 4}),
            ),
            (
                "party_race_clear_counts".to_owned(),
                json!({"Devil+Dragon+Human": 5}),
            ),
        ]);
        let catalog = mission_catalog().unwrap();
        let context = build_context(&root, catalog, &BTreeMap::new()).unwrap();
        for (mission_id, expected) in [
            (1_110_013, 1),
            (1_210_012, 7),
            (1_310_053, 2),
            (1_510_062, 2),
            (1_610_023, 4),
            (2_110_012, 4),
            (2_310_012, 5),
        ] {
            assert_eq!(awake_progress(mission_id, &context, catalog), expected);
        }
    }
    // //// /按战斗队伍和动作统计计算觉醒任务 ////

    // //// 按关卡范围计算摇曳的迷宫任务 [@x380kkm 2026-08-29] ////
    #[test]
    fn scopes_labyrinth_progress_to_matching_quest_categories() {
        let catalog = mission_catalog().unwrap();
        let mission = catalog.categories[&2]
            .iter()
            .find(|mission| mission.id == 2)
            .unwrap();
        assert_eq!(mission.quest_categories, vec![6, 14, 13, 20]);

        let main_only = Map::from_iter([
            (
                "quest_progress".to_owned(),
                json!({"1": [{"quest_id": 1_001_001, "finished": true}]}),
            ),
            ("user_info".to_owned(), json!({"rank_point": 0})),
        ]);
        let context = build_context(&main_only, catalog, &BTreeMap::new()).unwrap();
        assert_eq!(
            compute_progress(2, mission, &context, &BTreeMap::new(), 1, catalog),
            0
        );

        let labyrinth = Map::from_iter([
            (
                "quest_progress".to_owned(),
                json!({
                    "1": [{"quest_id": 1_001_001, "finished": true}],
                    "13": [{"quest_id": 1_001, "finished": true}],
                }),
            ),
            ("user_info".to_owned(), json!({"rank_point": 0})),
        ]);
        let counters = BTreeMap::from([(
            scoped_battle_counter_key("single_battle_clear_count", 13),
            3,
        )]);
        let context = build_context(&labyrinth, catalog, &counters).unwrap();
        assert_eq!(
            compute_progress(2, mission, &context, &counters, 1, catalog),
            3
        );
    }
    // //// /按关卡范围计算摇曳的迷宫任务 ////
}
