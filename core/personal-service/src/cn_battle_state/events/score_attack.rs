use super::{FinishBattleInput, RewardResult};
use crate::cn_activity;
use crate::cn_battle_assets::Reward;
use crate::cn_battle_rewards::apply_reward_at;
use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Stage {
    id: i64,
    event_id: i64,
    quest_id: i64,
    score: i64,
    rewards: Vec<Reward>,
}
static STAGES: OnceLock<Result<Vec<Stage>, String>> = OnceLock::new();
static RANKS: OnceLock<Result<std::collections::BTreeMap<String, [i64; 4]>, String>> =
    OnceLock::new();

pub(in crate::cn_battle_state) fn clear_rank(
    quest_id: i64,
    score: i64,
) -> Result<i64, PersonalServiceError> {
    let ranks = RANKS
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../../assets/cn-score-quest-ranks.json"))
                .map_err(|e| format!("invalid score ranks: {e}"))
        })
        .as_ref()
        .map_err(|e| PersonalServiceError::new(e.clone()))?;
    let thresholds = ranks
        .get(&quest_id.to_string())
        .ok_or_else(|| PersonalServiceError::new("missing score rank thresholds"))?;
    Ok(thresholds
        .iter()
        .enumerate()
        .filter(|(_, t)| score >= **t)
        .map(|(i, _)| i as i64 + 2)
        .max()
        .unwrap_or(1))
}

fn stages() -> Result<&'static Vec<Stage>, PersonalServiceError> {
    STAGES
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../../assets/cn-score-milestones.json"))
                .map_err(|e| format!("invalid score milestones: {e}"))
        })
        .as_ref()
        .map_err(|e| PersonalServiceError::new(e.clone()))
}

pub(in crate::cn_battle_state) fn is_accomplished(
    quest_id: i64,
    score: i64,
) -> Result<bool, PersonalServiceError> {
    let threshold = stages()?
        .iter()
        .filter(|s| s.quest_id == quest_id)
        .map(|s| s.score)
        .min()
        .ok_or_else(|| PersonalServiceError::new("missing score completion threshold"))?;
    Ok(score >= threshold)
}

pub(in crate::cn_battle_state) fn finish(
    root: &mut Map<String, Value>,
    quest_id: i64,
    input: &FinishBattleInput<'_>,
    previous_score: Option<i64>,
    now: i64,
) -> Result<(Value, RewardResult), PersonalServiceError> {
    let stages = stages()?;
    let event_id = stages
        .iter()
        .find(|s| s.quest_id == quest_id)
        .ok_or_else(|| PersonalServiceError::new("missing score milestone contract"))?
        .event_id;
    let event = cn_activity::activate_battle_event_state(root, "score_attack", event_id)?;
    let mut claimed: BTreeSet<i64> = event
        .get("claimed_reward_ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .collect();
    // Score attack can settle without defeating its boss. Eligibility is the score.
    let best = previous_score.unwrap_or(0).max(input.score);
    let pending: Vec<_> = stages
        .iter()
        .filter(|s| s.quest_id == quest_id && best >= s.score && !claimed.contains(&s.id))
        .collect();
    let mut result = RewardResult::default();
    let mut ids = Vec::new();
    for stage in pending {
        for reward in &stage.rewards {
            result.merge(apply_reward_at(root, reward, now)?);
        }
        ids.push(stage.id);
        claimed.insert(stage.id);
    }
    cn_activity::activate_battle_event_state(root, "score_attack", event_id)?
        .insert("claimed_reward_ids".to_owned(), json!(claimed));
    let characters: Map<String, Value> = input
        .main_character_ids
        .iter()
        .enumerate()
        .filter_map(|(i, id)| id.map(|id| (i.to_string(), json!(id))))
        .collect();
    Ok((
        json!({"main_character_ids": characters, "reward_ids": ids}),
        result,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grants_all_crossed_score_levels_once_and_keeps_secondary_items() {
        let mut root = json!({"user_info":{},"item_list":{}})
            .as_object()
            .unwrap()
            .clone();
        let low = super::super::milestone_test_input(120000, 1_049_999);
        let (response, rewards) = finish(&mut root, 1001, &low, None, 0).unwrap();
        assert_eq!(response["reward_ids"], json!([]));
        assert!(rewards.items.is_empty());
        let mut input = super::super::milestone_test_input(120000, 1_000_000_000);
        input.is_accomplished = false; // Score attack still pays achieved scores at timeout.
        let (response, _) = finish(&mut root, 1001, &input, None, 0).unwrap();
        assert_eq!(response["reward_ids"].as_array().unwrap().len(), 51);
        assert_eq!(root["item_list"]["40501"], 23);
        assert_eq!(root["item_list"]["40502"], 40);
        assert_eq!(response["main_character_ids"], json!({"0":1}));
        let saved = root.clone();
        assert!(finish(&mut root, 1001, &input, Some(input.score), 0)
            .unwrap()
            .1
            .items
            .is_empty());
        assert_eq!(root, saved);
        // Every Android quest has its own ledger; the top stage of some quests
        // contains several items, which the old single-coin implementation lost.
        let stages = STAGES.get().unwrap().as_ref().unwrap();
        let quests: BTreeSet<_> = stages.iter().map(|s| s.quest_id).collect();
        assert_eq!(quests.len(), 123);
        for quest in quests {
            let mut root = json!({"user_info":{},"item_list":{}})
                .as_object()
                .unwrap()
                .clone();
            let input = super::super::milestone_test_input(120000, i64::MAX);
            let (response, _) = finish(&mut root, quest, &input, None, 0).unwrap();
            let expected: Vec<_> = stages.iter().filter(|s| s.quest_id == quest).collect();
            let threshold = expected.iter().map(|s| s.score).min().unwrap();
            assert!(!is_accomplished(quest, threshold - 1).unwrap());
            assert!(is_accomplished(quest, threshold).unwrap());
            assert_eq!(clear_rank(quest, 0).unwrap(), 1);
            assert_eq!(clear_rank(quest, i64::MAX).unwrap(), 5);
            assert_eq!(
                response["reward_ids"].as_array().unwrap().len(),
                expected.len()
            );
            let mut counts = std::collections::BTreeMap::<i64, i64>::new();
            for stage in expected {
                for reward in &stage.rewards {
                    *counts.entry(reward.id.unwrap()).or_default() += reward.count.unwrap();
                }
            }
            for (id, count) in counts {
                assert_eq!(root["item_list"][id.to_string()], count);
            }
            let saved = root.clone();
            finish(&mut root, quest, &input, Some(input.score), 0).unwrap();
            assert_eq!(root, saved);
        }
    }
}
