use crate::PersonalServiceError;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Catalog {
    points: BTreeMap<i64, Point>,
    campaigns: BTreeMap<i64, Campaign>,
}
#[derive(Deserialize)]
struct Point {
    max: i64,
    recovers: bool,
}
#[derive(Deserialize)]
struct Campaign {
    point_id: i64,
    additional: i64,
    start: Option<i64>,
    end: Option<i64>,
}
static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();

// The old default snapshot contains campaign IDs absent from CN Android.
// Derive campaigns from its actual master and preserve spent points within a day.
pub(super) fn synchronize(
    root: &mut Map<String, Value>,
    now: i64,
    reset: bool,
) -> Result<(), PersonalServiceError> {
    let catalog = CATALOG
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../assets/cn-daily-challenge.json"))
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| PersonalServiceError::new(e.clone()))?;
    let entries = root
        .entry("user_daily_challenge_point_list")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| PersonalServiceError::new("CN challenge points are invalid"))?;
    for (id, point) in &catalog.points {
        let campaigns: Vec<Value> = catalog
            .campaigns
            .iter()
            .filter(|(_, c)| {
                c.point_id == *id
                    && c.start.map_or(true, |start| now >= start)
                    && c.end.map_or(true, |end| now <= end)
            })
            .map(|(id, c)| json!({"campaign_id": id, "additional_point": c.additional}))
            .collect();
        let maximum = campaigns.iter().fold(point.max, |n, c| {
            n.saturating_add(c["additional_point"].as_i64().unwrap_or(0))
        });
        if let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry["id"].as_i64() == Some(*id))
        {
            entry["campaign_list"] = json!(campaigns);
            if reset && point.recovers {
                entry["point"] = json!(maximum);
            }
        } else {
            entries.push(json!({"id": id, "point": maximum, "campaign_list": campaigns}));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_foreign_campaign_ids_and_preserves_spent_points() {
        let mut root = Map::from_iter([(
            "user_daily_challenge_point_list".into(),
            json!([
                {"id": 251, "point": 0, "campaign_list": [{"campaign_id":2023013102,"additional_point":2}]}
            ]),
        )]);
        synchronize(&mut root, 1575273600, false).unwrap();
        let entries = root["user_daily_challenge_point_list"].as_array().unwrap();
        let point = entries.iter().find(|e| e["id"] == 251).unwrap();
        assert_eq!(point["point"], 0);
        assert_eq!(point["campaign_list"], json!([]));
        assert_eq!(entries.len(), 282);
        synchronize(&mut root, 1575360000, true).unwrap();
        let point = root["user_daily_challenge_point_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == 251)
            .unwrap();
        assert_eq!(point["point"], 999);
    }
}
