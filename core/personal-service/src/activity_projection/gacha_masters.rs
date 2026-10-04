// Generated from the Android release, with the same region policy as the service.
// These masters must travel in a common archive so Android also receives them.
pub(super) const MANIFEST: &[u8] =
    include_bytes!("../../assets/cn-gacha-repair-masters/manifest.json");

const ENTRIES: [(&str, &[u8]); 4] = [
    (
        "production/upload/15/83d96aad4b9a46d19d19b6555d3f4232b29e25",
        include_bytes!("../../assets/cn-gacha-repair-masters/gacha.orderedmap"),
    ),
    (
        "production/upload/c8/79a1b712ae753afb0b5e796737fd141c8fa0ea",
        include_bytes!("../../assets/cn-gacha-repair-masters/gacha_feature_content.orderedmap"),
    ),
    (
        "production/upload/74/e7be1b0da0fd069aba147e2fd92da2b427d86e",
        include_bytes!("../../assets/cn-gacha-repair-masters/gacha_campaign.orderedmap"),
    ),
    (
        "production/upload/8c/82c8e05db4ce4f8a06bcd25f90bbda2967808b",
        include_bytes!("../../assets/cn-gacha-repair-masters/feature_banner.orderedmap"),
    ),
];

pub(super) fn client_entries() -> Vec<(String, Vec<u8>)> {
    ENTRIES
        .iter()
        .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::{ordered_map::decode_ordered_map, OrderedValue};
    use super::*;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;

    fn table(data: &[u8]) -> BTreeMap<String, OrderedValue> {
        match decode_ordered_map(data).unwrap() {
            OrderedValue::Map(rows) => rows.into_iter().collect(),
            _ => panic!("expected master map"),
        }
    }

    #[test]
    fn all_client_pools_have_feature_content_and_aliases_match_canonical() {
        let pools = table(ENTRIES[0].1);
        let features = table(ENTRIES[1].1);
        assert_eq!(pools.len(), 1057);
        assert_eq!(features.len(), pools.len());
        for id in pools.keys() {
            assert!(
                matches!(&features[id], OrderedValue::Map(rows) if !rows.is_empty()),
                "missing feature for {id}"
            );
        }
        let policy: serde_json::Value =
            serde_json::from_str(include_str!("../../../../assets/gacha-region-policy.json"))
                .unwrap();
        for kind in ["normalizedCoverageAliases", "temporaryAliases"] {
            for (alias, canonical) in policy[kind].as_object().unwrap() {
                assert_eq!(&features[alias], &features[&canonical.to_string()]);
            }
        }
        assert_eq!(features["61"], features["1"]);
    }

    #[test]
    fn embedded_master_bytes_match_the_signature_manifest() {
        let manifest: serde_json::Value = serde_json::from_slice(MANIFEST).unwrap();
        for ((entry, bytes), record) in ENTRIES.iter().zip(manifest["masters"].as_array().unwrap())
        {
            assert_eq!(*entry, record["entryPath"].as_str().unwrap());
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), record["sha256"]);
            assert_eq!(
                table(bytes).len(),
                record["rows"].as_u64().unwrap() as usize
            );
        }
    }
}
