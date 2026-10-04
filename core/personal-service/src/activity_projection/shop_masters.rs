// The client uses this master to decide whether to show the quantity slider.
pub(super) const MANIFEST: &[u8] =
    include_bytes!("../../assets/cn-mana-shop-master/manifest.json");
const MASTER: &[u8] =
    include_bytes!("../../assets/cn-mana-shop-master/treasure_shop.orderedmap");
const ENTRY: &str = "production/upload/47/3f4d20c2ee78c3af3f0bfc33494b690bc0b205";

pub(super) fn client_entries() -> Vec<(String, Vec<u8>)> {
    vec![(ENTRY.to_owned(), MASTER.to_vec())]
}

#[cfg(test)]
mod tests {
    use super::super::{ordered_map::decode_ordered_map, OrderedValue};
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn quantity_master_matches_server_limits_and_keeps_daily_stock() {
        let manifest: serde_json::Value = serde_json::from_slice(MANIFEST).unwrap();
        assert_eq!(manifest["entryPath"], ENTRY);
        assert_eq!(manifest["sha256"], format!("{:x}", Sha256::digest(MASTER)));
        let original: serde_json::Value =
            serde_json::from_str(include_str!("../../../../assets/cn-shop-limits.json")).unwrap();
        let OrderedValue::Map(rows) = decode_ordered_map(MASTER).unwrap() else {
            panic!("expected shop master map")
        };
        assert_eq!(rows.len(), manifest["rows"].as_u64().unwrap() as usize);
        for (id, row) in rows {
            let OrderedValue::Row(row) = row else { panic!("expected shop row") };
            if let Some(limit) = manifest["limits"].get(&id) {
                assert_eq!(row[7], "1");
                assert_eq!(row[21], limit.to_string());
                assert_eq!(row[23], original["2"][&id]["dailyStock"].to_string());
            }
        }
        assert_eq!(manifest["limits"]["200001"], 10);
    }
}
