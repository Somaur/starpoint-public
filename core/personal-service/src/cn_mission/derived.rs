use super::{
    build_context, compute_lifetime_progress, mission_catalog, ComputeContext, MissionCatalog,
    MissionDefinition, MissionKey,
};
use crate::{database::ServiceDatabase, PersonalServiceError};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub(super) fn chapter_final_quests() -> Result<BTreeMap<(i64, i64), i64>, String> {
    let fixture =
        crate::cn_battle_assets::load_battle_fixture().map_err(|error| error.to_string())?;
    let mut result = BTreeMap::<(i64, i64), i64>::new();
    for quest in fixture
        .quests
        .values()
        .filter(|quest| matches!(quest.category, 1 | 4))
    {
        let last = result
            .entry((quest.category, quest.quest_id / 1_000_000))
            .or_default();
        *last = (*last).max(quest.quest_id);
    }
    Ok(result)
}

pub(super) fn chapter_progress(
    mission: &MissionDefinition,
    context: &ComputeContext,
    catalog: &MissionCatalog,
) -> i64 {
    catalog
        .chapter_final_quests
        .iter()
        .filter(|((category, _), quest)| {
            mission.quest_categories.contains(category)
                && super::quest_matches_scope(mission, **quest)
                && context
                    .quest_progress
                    .get(category)
                    .is_some_and(|entries| entries.iter().any(|entry| entry.quest_id == **quest))
        })
        .count() as i64
}

// CN daily rollover is 05:00 UTC+8; weeks begin on Monday at that time.
fn period(category: i64, now: i64) -> Option<i64> {
    let day = (now + 3 * 3600).div_euclid(86400);
    match category {
        2 => Some(day),
        10 => Some((day + 3).div_euclid(7)),
        _ => None,
    }
}

pub(crate) fn prepare_snapshot_periods(
    serialized: &str,
    database: &ServiceDatabase,
    account_id: i64,
    now: i64,
) -> Result<String, PersonalServiceError> {
    let mut data = crate::cn_tutorial::decode_player_data(serialized)?;
    let root = data
        .as_object_mut()
        .ok_or_else(|| PersonalServiceError::new("stored CN player data is not an object"))?;
    prepare_periods(root, database, account_id, now)?;
    crate::cn_tutorial::encode_player_data(&data)
}

pub(super) fn mode_statistics(root: &Map<String, Value>) -> BTreeMap<(i64, i64), i64> {
    let info = root.get("user_info").and_then(Value::as_object);
    let mut result = BTreeMap::new();
    for (mode, suffix) in [(1, "single"), (2, "multi")] {
        for (kind, name) in [
            (0, "total_weak_point_attacks"),
            (1, "total_powerflips"),
            (2, "total_dashes"),
            (4, "total_skills"),
            (5, "total_fevers"),
            (7, "total_enemy_kills"),
        ] {
            result.insert(
                (mode, kind),
                info.and_then(|info| info.get(&format!("{name}_{suffix}")))
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
            );
        }
    }
    result
}

pub(super) fn previous_notification(
    root: &Map<String, Value>,
    key: MissionKey,
    fallback: i64,
) -> i64 {
    if !matches!(key.category, 2 | 10) {
        return fallback;
    }
    root.get("mission_period_state")
        .and_then(|v| v.get(key.category.to_string()))
        .and_then(|v| v.get("notified"))
        .and_then(|v| v.get(key.mission_id.to_string()))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

pub(super) fn record_notification(root: &mut Map<String, Value>, key: MissionKey, progress: i64) {
    if !matches!(key.category, 2 | 10) {
        return;
    }
    if let Some(state) = root
        .get_mut("mission_period_state")
        .and_then(|v| v.get_mut(key.category.to_string()))
        .and_then(Value::as_object_mut)
    {
        let notified = state
            .entry("notified".to_owned())
            .or_insert_with(|| json!({}));
        if let Some(notified) = notified.as_object_mut() {
            notified.insert(key.mission_id.to_string(), Value::from(progress));
        }
    }
}

pub(super) fn stage_receipt(key: MissionKey, stage: i64, now: i64) -> String {
    match period(key.category, now) {
        Some(period) => format!(
            "rewards-v2:{}:{}:{stage}:{period}",
            key.category, key.mission_id
        ),
        None if key.category != 9 => {
            format!("rewards-v2:{}:{}:{stage}", key.category, key.mission_id)
        }
        None => format!("{}:{}:{stage}", key.category, key.mission_id),
    }
}

pub(crate) fn prepare_periods(
    root: &mut Map<String, Value>,
    database: &ServiceDatabase,
    account_id: i64,
    now: i64,
) -> Result<bool, PersonalServiceError> {
    let catalog = mission_catalog()?;
    let counters = database.mission_counters(account_id)?;
    let stored = database.mission_progress(account_id)?;
    let context = build_context(root, catalog, &counters)?;
    let mut periods = root
        .get("mission_period_state")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut changed = false;
    for category in [2, 10] {
        let key = category.to_string();
        let current = period(category, now).unwrap();
        if periods
            .get(&key)
            .and_then(|v| v.get("period"))
            .and_then(Value::as_i64)
            == Some(current)
        {
            continue;
        }
        // Upgrade existing saves once without discarding their recorded actions.
        // Later periods subtract the cumulative totals at the rollover boundary.
        let initialized = periods.contains_key(&key);
        let baselines: BTreeMap<String, i64> = catalog
            .categories
            .get(&category)
            .into_iter()
            .flatten()
            .map(|mission| {
                let value = if initialized {
                    compute_lifetime_progress(
                        category,
                        mission,
                        &context,
                        &counters,
                        stored.get(&(category, mission.id)).copied().unwrap_or(0),
                        catalog,
                    )
                } else {
                    0
                };
                (format!("{category}:{}", mission.id), value)
            })
            .collect();
        periods.insert(key, json!({"period":current,"baselines":baselines}));
        changed = true;
    }
    if changed {
        root.insert("mission_period_state".to_owned(), Value::Object(periods));
    }
    Ok(changed)
}

pub(super) fn period_baselines(root: &Map<String, Value>) -> BTreeMap<String, i64> {
    root.get("mission_period_state")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|periods| periods.values())
        .filter_map(|v| v.get("baselines").and_then(Value::as_object))
        .flat_map(|values| values.iter())
        .filter_map(|(key, value)| value.as_i64().map(|v| (key.clone(), v)))
        .collect()
}

pub(super) fn statistics(root: &Map<String, Value>) -> BTreeMap<i64, i64> {
    let info = root.get("user_info").and_then(Value::as_object);
    [
        (0, "total_weak_point_attacks"),
        (1, "total_powerflips"),
        (2, "total_dashes"),
        (4, "total_skills"),
        (5, "total_fevers"),
        (7, "total_enemy_kills"),
    ]
    .into_iter()
    .map(|(kind, key)| {
        (
            kind,
            info.and_then(|v| v.get(key))
                .and_then(Value::as_i64)
                .unwrap_or(0),
        )
    })
    .collect()
}

pub(super) fn compute_progress(
    category: i64,
    mission: &MissionDefinition,
    context: &ComputeContext,
    counters: &BTreeMap<String, i64>,
    previous: i64,
    catalog: &MissionCatalog,
    depth: usize,
) -> i64 {
    if depth > 12 {
        return 0;
    }
    if mission.pattern.as_deref() == Some("target_mission_clear")
        && !mission.target_mission_ids.is_empty()
    {
        return mission
            .target_mission_ids
            .iter()
            .filter(|id| {
                catalog
                    .categories
                    .get(&category)
                    .and_then(|missions| missions.iter().find(|m| m.id == **id))
                    .is_some_and(|dependency| {
                        let progress = compute_progress(
                            category,
                            dependency,
                            context,
                            counters,
                            0,
                            catalog,
                            depth + 1,
                        );
                        dependency
                            .stages
                            .first()
                            .and_then(|stage| stage.target)
                            .is_some_and(|target| progress >= target)
                    })
            })
            .count() as i64;
    }
    let lifetime =
        compute_lifetime_progress(category, mission, context, counters, previous, catalog);
    if matches!(category, 2 | 10) {
        lifetime
            .saturating_sub(
                context
                    .period_baselines
                    .get(&format!("{category}:{}", mission.id))
                    .copied()
                    .unwrap_or(0),
            )
            .max(0)
    } else {
        lifetime
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cn_rollover_and_receipts_are_period_scoped() {
        // 2026-10-03 04:59:59 / 05:00:00 CN.
        let before = 1790974799;
        assert_ne!(period(2, before), period(2, before + 1));
        let key = MissionKey {
            category: 2,
            mission_id: 4,
        };
        assert_ne!(
            stage_receipt(key, 1, before),
            stage_receipt(key, 1, before + 1)
        );
        assert_eq!(
            stage_receipt(
                MissionKey {
                    category: 9,
                    mission_id: 13
                },
                1,
                before
            ),
            "9:13:1"
        );
    }
}
