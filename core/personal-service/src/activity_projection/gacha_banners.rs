// Generated management banners must also reach the native game resource cache.
use crate::PersonalServiceError;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
struct Catalog {
    activities: Vec<Activity>,
}

#[derive(Deserialize)]
struct Activity {
    kind: String,
    banner_key: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

pub(super) fn read_catalog(root: &Path) -> Result<Vec<u8>, PersonalServiceError> {
    match fs::read(root.join("activity-catalog.json")) {
        Ok(data) => Ok(data),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(PersonalServiceError::new(format!(
            "read banner catalog: {error}"
        ))),
    }
}

pub(super) fn client_entries(
    root: &Path,
    catalog: &[u8],
) -> Result<Vec<(String, Vec<u8>)>, PersonalServiceError> {
    if catalog.is_empty() {
        return Ok(Vec::new());
    }
    let catalog: Catalog = serde_json::from_slice(catalog)
        .map_err(|error| PersonalServiceError::new(format!("decode banner catalog: {error}")))?;
    let mut keys = BTreeSet::new();
    for activity in catalog.activities {
        if activity.kind != "gacha" || !activity.tags.iter().any(|tag| tag == "banner:generated") {
            continue;
        }
        let key = activity.banner_key.unwrap_or_default();
        let hash = key.strip_suffix(".png").unwrap_or_default();
        if hash.len() != 40
            || !hash
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(PersonalServiceError::new(
                "invalid generated gacha banner key",
            ));
        }
        keys.insert(key);
    }
    let mut entries = Vec::new();
    for key in keys {
        let path = root.join("activity-banners").join(&key);
        let metadata = fs::metadata(&path).map_err(|error| {
            PersonalServiceError::new(format!("missing gacha banner {key}: {error}"))
        })?;
        if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 {
            return Err(PersonalServiceError::new(format!(
                "invalid gacha banner size: {key}"
            )));
        }
        let mut data = fs::read(&path).map_err(|error| {
            PersonalServiceError::new(format!("read gacha banner {key}: {error}"))
        })?;
        validate_png(&data).map_err(|error| {
            PersonalServiceError::new(format!("invalid gacha banner {key}: {error}"))
        })?;
        // Match the resource format used by the existing client archive packager.
        data[1..4].copy_from_slice(b"png");
        entries.push((
            format!("production/upload/{}/{}", &key[..2], &key[2..40]),
            data,
        ));
    }
    Ok(entries)
}

fn validate_png(data: &[u8]) -> Result<(), &'static str> {
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") && !data.starts_with(b"\x89png\r\n\x1a\n") {
        return Err("bad PNG signature");
    }
    let mut position = 8;
    let mut has_header = false;
    let mut has_image = false;
    while position + 12 <= data.len() {
        let length = u32::from_be_bytes(data[position..position + 4].try_into().unwrap()) as usize;
        let end = position
            .checked_add(length)
            .and_then(|n| n.checked_add(12))
            .filter(|n| *n <= data.len())
            .ok_or("truncated PNG chunk")?;
        let kind = &data[position + 4..position + 8];
        let payload = &data[position + 8..end - 4];
        let crc = u32::from_be_bytes(data[end - 4..end].try_into().unwrap());
        if crc32fast::hash(&data[position + 4..end - 4]) != crc {
            return Err("PNG checksum mismatch");
        }
        if !has_header {
            if kind != b"IHDR" || payload.len() != 13 {
                return Err("missing PNG header");
            }
            let width = u32::from_be_bytes(payload[..4].try_into().unwrap());
            let height = u32::from_be_bytes(payload[4..8].try_into().unwrap());
            if width != 510 || height != 180 {
                return Err("unexpected banner dimensions");
            }
            has_header = true;
        } else if kind == b"IHDR" || kind == b"acTL" {
            return Err("duplicate header or animated PNG");
        }
        if kind == b"IDAT" {
            has_image = true;
        }
        if kind == b"IEND" {
            return if has_image && payload.is_empty() && end == data.len() {
                Ok(())
            } else {
                Err("invalid PNG end")
            };
        }
        position = end;
    }
    Err("incomplete PNG")
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;

    const KEY: &str = "8ed654129fbfc216fd473beca36107d5168c0498.png";

    fn png(width: u32) -> Vec<u8> {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = width.to_be_bytes().to_vec();
        header.extend_from_slice(&180u32.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut compressed = ZlibEncoder::new(Vec::new(), Compression::default());
        compressed
            .write_all(&vec![0; (width as usize * 3 + 1) * 180])
            .unwrap();
        for (kind, payload) in [
            (b"IHDR", header),
            (b"IDAT", compressed.finish().unwrap()),
            (b"IEND", vec![]),
        ] {
            data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            let start = data.len();
            data.extend_from_slice(kind);
            data.extend_from_slice(&payload);
            data.extend_from_slice(&crc32fast::hash(&data[start..]).to_be_bytes());
        }
        data
    }

    fn catalog(key: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"activities": [
            {"kind":"gacha", "banner_key":key, "tags":["banner:generated"]},
            {"kind":"gacha", "banner_key":key, "tags":["banner:generated"]},
            {"kind":"gacha", "banner_key":"ignored.png", "tags":[]},
            {"kind":"event", "banner_key":"ignored.png", "tags":["banner:generated"]}
        ]}))
        .unwrap()
    }

    #[test]
    fn projects_only_generated_banners_once_in_client_format() {
        let root = tempfile::TempDir::new().unwrap();
        fs::create_dir(root.path().join("activity-banners")).unwrap();
        let mut image = png(510);
        fs::write(root.path().join("activity-banners").join(KEY), &image).unwrap();
        let entries = client_entries(root.path(), &catalog(KEY)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].0,
            "production/upload/8e/d654129fbfc216fd473beca36107d5168c0498"
        );
        image[1..4].copy_from_slice(b"png");
        assert_eq!(entries[0].1, image);
    }

    #[test]
    fn rejects_missing_and_unsafe_banner_paths() {
        let root = tempfile::TempDir::new().unwrap();
        for key in [KEY, "../../outside.png", "", "aa.png"] {
            assert!(client_entries(root.path(), &catalog(key)).is_err());
        }
        assert!(client_entries(root.path(), &[]).unwrap().is_empty());
    }

    #[test]
    fn rejects_damaged_and_wrong_size_images() {
        assert!(validate_png(&png(510)).is_ok());
        assert!(validate_png(&png(511)).is_err());
        let mut image = png(510);
        image[50] ^= 1;
        assert!(validate_png(&image).is_err());
        let image = png(510);
        assert!(validate_png(&image[..image.len() - 1]).is_err());
        assert!(validate_png(b"not an image").is_err());
    }
}
