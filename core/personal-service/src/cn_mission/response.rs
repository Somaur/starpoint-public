use serde_json::{json, Value};

// Mission rewards share the action's common response so the displayed wallet
// and inventory agree with the persisted reward receipts immediately.
pub(crate) fn sync_reward_response(response: &mut Value, player: &Value, now: i64) {
    if !response.is_object() {
        return;
    }
    if !response.get("user_info").is_some_and(Value::is_object) {
        response["user_info"] = json!({});
    }
    for field in [
        "free_vmoney",
        "vmoney",
        "free_mana",
        "paid_mana",
        "exp_pool",
    ] {
        if let Some(value) = player.get("user_info").and_then(|v| v.get(field)) {
            response["user_info"][field] = value.clone();
        }
    }
    if let Some(items) = player.get("item_list") {
        response["item_list"] = items.clone();
    }
    if let Some(equipments) = player.get("user_equipment_list").and_then(Value::as_object) {
        let target = response
            .as_object_mut()
            .unwrap()
            .entry("equipment_list")
            .or_insert_with(|| json!([]));
        if let Some(target) = target.as_array_mut() {
            for (id, value) in equipments {
                if let (Ok(id), Some(value)) = (id.parse::<i64>(), value.as_object()) {
                    let current = crate::cn_equipment::serialize_equipment(id, value);
                    if let Some(existing) = target
                        .iter_mut()
                        .find(|v| v["equipment_id"].as_i64() == Some(id))
                    {
                        existing
                            .as_object_mut()
                            .unwrap()
                            .extend(current.as_object().unwrap().clone());
                    } else {
                        target.push(current);
                    }
                }
            }
        }
    }
    if let Some(characters) = player.get("user_character_list").and_then(Value::as_object) {
        let target = response
            .as_object_mut()
            .unwrap()
            .entry("character_list")
            .or_insert_with(|| json!([]));
        if let Some(target) = target.as_array_mut() {
            for (id, value) in characters {
                if let Ok(id) = id.parse::<i64>() {
                    let current = crate::cn_character_reward::create_existing_character_response(
                        id, value, now,
                    );
                    if let Some(existing) = target
                        .iter_mut()
                        .find(|v| v["character_id"].as_i64() == Some(id))
                    {
                        existing
                            .as_object_mut()
                            .unwrap()
                            .extend(current.as_object().unwrap().clone());
                    } else {
                        target.push(current);
                    }
                }
            }
        }
    }
}
