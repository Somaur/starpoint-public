// Local save maintenance is transactional. Deletion retains a restorable game-data backup.
use super::mutations::{
    available_local_save_name, create_local_save_slot, find_local_save_slot,
    insert_local_save_snapshot, replace_local_save_data_with_labels,
};
use super::{local_save_storage_error, LocalSaveRevision, LocalSaveSlot, LocalSaveStoreError};
use crate::{database::ServiceDatabase, portable_save, PersonalServiceError};
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Map, Value};

fn read_for_change(
    tx: &Transaction<'_>,
    slot_id: i64,
    expected: &str,
) -> Result<(i64, String, Value), LocalSaveStoreError> {
    let (account_id, name, serialized) = tx
        .query_row(
            "SELECT s.account_id, s.name, p.data_json FROM local_save_slots s
         JOIN player_snapshots p ON p.account_id = s.account_id WHERE s.id = ?1",
            [slot_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2)?)),
        )
        .optional()
        .map_err(local_save_storage_error)?
        .ok_or(LocalSaveStoreError::NotFound)?;
    let data: Value = serde_json::from_str(&serialized).map_err(json_error)?;
    let etag =
        portable_save::calculate_payload_sha256(&data).map_err(LocalSaveStoreError::Storage)?;
    if etag != expected {
        return Err(LocalSaveStoreError::Busy);
    }
    Ok((account_id, name, data))
}

fn json_error(error: serde_json::Error) -> LocalSaveStoreError {
    LocalSaveStoreError::Storage(PersonalServiceError::new(format!(
        "invalid stored save JSON: {error}"
    )))
}

impl ServiceDatabase {
    pub(crate) fn edit_local_save_resources(
        &mut self,
        slot_id: i64,
        expected: &str,
        resources: &Map<String, Value>,
    ) -> Result<LocalSaveRevision, LocalSaveStoreError> {
        let tx = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let (account_id, _, mut data) = read_for_change(&tx, slot_id, expected)?;
        let busy: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM active_single_quests WHERE account_id = ?1)
             OR EXISTS(SELECT 1 FROM multiplayer_room_members WHERE account_id = ?1)",
                [account_id],
                |r| r.get(0),
            )
            .map_err(local_save_storage_error)?;
        if busy {
            return Err(LocalSaveStoreError::Busy);
        }
        let user = data
            .get_mut("user_info")
            .and_then(Value::as_object_mut)
            .ok_or(LocalSaveStoreError::InvalidState)?;
        for (key, value) in resources {
            user.insert(key.clone(), value.clone());
        }
        let serialized = serde_json::to_string(&data).map_err(json_error)?;
        insert_local_save_snapshot(&tx, slot_id, "Before resource edit", false)?;
        let revision = replace_local_save_data_with_labels(
            &tx,
            slot_id,
            &serialized,
            "Before resource edit",
            "Resource edit",
        )?;
        tx.commit().map_err(local_save_storage_error)?;
        Ok(revision)
    }

    pub(crate) fn delete_local_save_with_backup(
        &mut self,
        slot_id: i64,
        expected: &str,
    ) -> Result<i64, LocalSaveStoreError> {
        let tx = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let (account_id, name, data) = read_for_change(&tx, slot_id, expected)?;
        let busy: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM active_local_save_slots WHERE slot_id = ?1)
             OR EXISTS(SELECT 1 FROM multiplayer_ai_mates WHERE snapshot_id IN
                 (SELECT id FROM ai_team_snapshots WHERE slot_id = ?1))
             OR EXISTS(SELECT 1 FROM multiplayer_room_members WHERE account_id = ?2)",
                params![slot_id, account_id],
                |r| r.get(0),
            )
            .map_err(local_save_storage_error)?;
        if busy {
            return Err(LocalSaveStoreError::Busy);
        }
        let data =
            portable_save::sanitize_game_data(data).ok_or(LocalSaveStoreError::InvalidState)?;
        tx.execute("INSERT INTO deleted_local_save_backups
            (original_slot_id, name, data_json, deleted_at) VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![slot_id, name, serde_json::to_string(&data).map_err(json_error)?])
            .map_err(local_save_storage_error)?;
        let backup_id = tx.last_insert_rowid();
        tx.execute(
            "DELETE FROM ai_team_snapshot_heads WHERE slot_id = ?1",
            [slot_id],
        )
        .map_err(local_save_storage_error)?;
        tx.execute(
            "DELETE FROM ai_team_snapshots WHERE slot_id = ?1",
            [slot_id],
        )
        .map_err(local_save_storage_error)?;
        tx.execute("DELETE FROM local_save_heads WHERE slot_id = ?1", [slot_id])
            .map_err(local_save_storage_error)?;
        // Parents are always inserted before children, including branches after a restore.
        let revisions = {
            let mut q = tx
                .prepare(
                    "SELECT id FROM local_save_revisions WHERE slot_id = ?1 ORDER BY rowid DESC",
                )
                .map_err(local_save_storage_error)?;
            let rows = q
                .query_map([slot_id], |r| r.get::<_, String>(0))
                .map_err(local_save_storage_error)?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(local_save_storage_error)?
        };
        for id in revisions {
            tx.execute("DELETE FROM local_save_revisions WHERE id = ?1", [id])
                .map_err(local_save_storage_error)?;
        }
        tx.execute("DELETE FROM accounts WHERE id = ?1", [account_id])
            .map_err(local_save_storage_error)?;
        tx.commit().map_err(local_save_storage_error)?;
        Ok(backup_id)
    }

    pub(crate) fn deleted_local_save_backups(&self) -> Result<Vec<Value>, LocalSaveStoreError> {
        let mut q = self
            .connection
            .prepare(
                "SELECT id, original_slot_id, name, deleted_at, restored_slot_id
            FROM deleted_local_save_backups ORDER BY id DESC",
            )
            .map_err(local_save_storage_error)?;
        let rows = q.query_map([], |r| Ok(json!({"id":r.get::<_, i64>(0)?, "original_slot_id":r.get::<_, i64>(1)?,
            "name":r.get::<_, String>(2)?, "deleted_at":r.get::<_, String>(3)?, "restored_slot_id":r.get::<_, Option<i64>>(4)?})))
            .map_err(local_save_storage_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(local_save_storage_error)
    }

    pub(crate) fn restore_deleted_local_save(
        &mut self,
        backup_id: i64,
    ) -> Result<LocalSaveSlot, LocalSaveStoreError> {
        let tx = self
            .connection
            .transaction()
            .map_err(local_save_storage_error)?;
        let (name, data, restored) = tx.query_row(
            "SELECT name, data_json, restored_slot_id FROM deleted_local_save_backups WHERE id = ?1", [backup_id],
            |r| Ok((r.get::<_, String>(0)?,r.get::<_, String>(1)?,r.get::<_, Option<i64>>(2)?)))
            .optional().map_err(local_save_storage_error)?.ok_or(LocalSaveStoreError::NotFound)?;
        if let Some(slot) = restored
            .map(|id| find_local_save_slot(&tx, id))
            .transpose()?
            .flatten()
        {
            return Ok(slot);
        }
        let name = available_local_save_name(&tx, &name)?;
        let slot_id = create_local_save_slot(&tx, &name, &data)?;
        tx.execute(
            "UPDATE deleted_local_save_backups SET restored_slot_id = ?1 WHERE id = ?2",
            params![slot_id, backup_id],
        )
        .map_err(local_save_storage_error)?;
        tx.commit().map_err(local_save_storage_error)?;
        find_local_save_slot(&self.connection, slot_id)?.ok_or(LocalSaveStoreError::InvalidState)
    }
}
