use crate::cn_activity;
use crate::cn_battle_assets::Reward;
use crate::cn_battle_rewards::{apply_reward_at, RewardResult};
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Stage {
    id: i64,
    event_id: i64,
    score: i64,
    rewards: Vec<Reward>,
    degrees: Vec<i64>,
}
static STAGES: OnceLock<Result<Vec<Stage>, String>> = OnceLock::new();

pub(super) fn grant(
    root: &mut Map<String, Value>,
    event_id: i64,
    now: i64,
) -> Result<(Vec<i64>, Vec<i64>, RewardResult), PersonalServiceError> {
    let stages = STAGES
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "../../../assets/cn-carnival-score-rewards.json"
            ))
            .map_err(|error| format!("invalid carnival rewards: {error}"))
        })
        .as_ref()
        .map_err(|error| PersonalServiceError::new(error.clone()))?;
    let event = cn_activity::activate_battle_event_state(root, "carnival", event_id)?;
    let score = event
        .get("carnival_records")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| r.get("best_score").and_then(Value::as_i64))
        .fold(0_i64, i64::saturating_add);
    let claimed = event
        .get("claimed_score_reward_ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pending = stages
        .iter()
        .filter(|stage| {
            stage.event_id == event_id
                && score >= stage.score
                && !claimed.contains(&Value::from(stage.id))
        })
        .collect::<Vec<_>>();
    let mut ids = Vec::new();
    let mut degrees = Vec::new();
    let mut result = RewardResult::default();
    for stage in pending {
        for reward in &stage.rewards {
            result.merge(apply_reward_at(root, reward, now)?);
        }
        degrees.extend(stage.degrees.iter().copied());
        ids.push(stage.id);
    }
    if !degrees.is_empty() {
        let earned = root
            .entry("earned_degree_ids".to_owned())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| PersonalServiceError::new("stored degrees are invalid"))?;
        for id in &degrees {
            if !earned.contains(&Value::from(*id)) {
                earned.push(Value::from(*id));
            }
        }
    }
    if !ids.is_empty() {
        let event = cn_activity::activate_battle_event_state(root, "carnival", event_id)?;
        let mut received = claimed;
        received.extend(ids.iter().copied().map(Value::from));
        event.insert(
            "claimed_score_reward_ids".to_owned(),
            Value::Array(received),
        );
    }
    Ok((ids, degrees, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn carnival_threshold_rewards_and_titles_are_granted_once() {
        let mut root = json!({"user_info":{"free_vmoney":0,"free_mana":0,"exp_pool":0},"item_list":{},"user_equipment_list":{},"user_character_list":{}}).as_object().unwrap().clone();
        let event =
            cn_activity::activate_battle_event_state(&mut root, "carnival", 100000).unwrap();
        event.insert(
            "carnival_records".to_owned(),
            json!([{"folder_id":1,"best_score":10_000_000}]),
        );
        let (ids, degrees, rewards) = grant(&mut root, 100000, 0).unwrap();
        assert!(ids.contains(&10000000));
        assert!(degrees.contains(&61030));
        assert!(rewards.vmoney >= 500);
        assert!(root["item_list"]["14040"].as_i64().unwrap() >= 1);
        let saved = root.clone();
        let (ids, degrees, rewards) = grant(&mut root, 100000, 0).unwrap();
        assert!(ids.is_empty() && degrees.is_empty());
        assert_eq!(rewards.vmoney, 0);
        assert_eq!(root, saved);
    }
}
