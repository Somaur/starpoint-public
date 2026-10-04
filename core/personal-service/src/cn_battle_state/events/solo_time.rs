use super::{FinishBattleInput, RewardResult};
use crate::cn_activity;
use crate::cn_battle_assets::Reward;
use crate::cn_battle_rewards::apply_reward_at;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Stage {
    id: i64,
    event_id: i64,
    quest_id: i64,
    time_ms: i64,
    rewards: Vec<Reward>,
    degrees: Vec<i64>,
}
static STAGES: OnceLock<Result<Vec<Stage>, String>> = OnceLock::new();

// Claimed IDs are persisted separately from best time: old local builds saved
// clear records without paying these rewards. The next clear repairs that debt.
pub(in crate::cn_battle_state) fn finish(
    root: &mut Map<String, Value>,
    quest_id: i64,
    input: &FinishBattleInput<'_>,
    previous_time: Option<i64>,
    previous_score: Option<i64>,
    now: i64,
) -> Result<(Value, RewardResult), PersonalServiceError> {
    if !input.is_accomplished {
        return Ok((Value::Null, RewardResult::default()));
    }
    let stages = STAGES
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../../assets/cn-solo-time-rewards.json"))
                .map_err(|e| format!("invalid solo time rewards: {e}"))
        })
        .as_ref()
        .map_err(|e| PersonalServiceError::new(e.clone()))?;
    let event_id = stages
        .iter()
        .find(|s| s.quest_id == quest_id)
        .ok_or_else(|| PersonalServiceError::new("missing solo time reward contract"))?
        .event_id;
    let event = cn_activity::activate_battle_event_state(root, "solo_time_attack", event_id)?;
    let mut claimed = event
        .get("claimed_reward_ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let best = previous_time.map_or(input.elapsed_time_ms, |old| old.min(input.elapsed_time_ms));
    let pending: Vec<_> = stages
        .iter()
        .filter(|s| s.quest_id == quest_id && best <= s.time_ms && !claimed.contains(&json!(s.id)))
        .collect();
    let mut result = RewardResult::default();
    let mut ids = Vec::new();
    let mut degrees = Vec::new();
    for stage in pending {
        for reward in &stage.rewards {
            result.merge(apply_reward_at(root, reward, now)?);
        }
        ids.push(stage.id);
        degrees.extend(stage.degrees.iter().copied());
        claimed.push(json!(stage.id));
    }
    if !degrees.is_empty() {
        let earned = root
            .entry("earned_degree_ids")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| PersonalServiceError::new("stored degrees are invalid"))?;
        for id in &degrees {
            if !earned.contains(&json!(id)) {
                earned.push(json!(id));
            }
        }
    }
    cn_activity::activate_battle_event_state(root, "solo_time_attack", event_id)?
        .insert("claimed_reward_ids".to_owned(), json!(claimed));
    let characters: Map<String, Value> = input
        .main_character_ids
        .iter()
        .enumerate()
        .filter_map(|(i, id)| id.map(|id| (i.to_string(), json!(id))))
        .collect();
    Ok((
        json!({
            "elapsed_time_ms": input.elapsed_time_ms,
            "previous_best_elapsed_time_ms": previous_time,
            "previous_best_elapsed_score": previous_score,
            "main_character_ids": characters,
            "reward_ids": ids,
            "new_degree_ids": degrees,
        }),
        result,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pays_exact_solo_wallet_and_titles_without_mission_side_effects() {
        let mut root = json!({"user_info":{"free_vmoney":0},"item_list":{}})
            .as_object()
            .unwrap()
            .clone();
        let slow = super::super::milestone_test_input(420001, 100);
        assert!(
            finish(&mut root, 1001, &slow, None, None, 0)
                .unwrap()
                .1
                .vmoney
                == 0
        );
        let border = super::super::milestone_test_input(420000, 101);
        assert_eq!(
            finish(&mut root, 1001, &border, Some(420001), Some(100), 0)
                .unwrap()
                .1
                .vmoney,
            10
        );
        let fast = super::super::milestone_test_input(60000, 999);
        let (response, rewards) =
            finish(&mut root, 1001, &fast, Some(420000), Some(101), 0).unwrap();
        assert_eq!(rewards.vmoney, 990);
        assert_eq!(response["new_degree_ids"].as_array().unwrap().len(), 2);
        assert_eq!(root["user_info"]["free_vmoney"], 1000);
        let saved = root.clone();
        assert_eq!(
            finish(&mut root, 1001, &fast, Some(60000), Some(999), 0)
                .unwrap()
                .1
                .vmoney,
            0
        );
        assert_eq!(root, saved);
        let mut failed = fast;
        failed.is_accomplished = false;
        assert!(finish(&mut root, 1002, &failed, None, None, 0)
            .unwrap()
            .0
            .is_null());
        assert_eq!(root, saved);
    }
}
