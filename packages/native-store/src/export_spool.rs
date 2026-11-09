use crate::{decode_day_end, validate_text, DayEndCandidate, StoreError};
use engine::session::protocol::SaveCandidateKey;
use rusqlite::{params, Connection, OptionalExtension};
use std::io::Write;

struct PayloadCounter {
    bytes: u64,
    limit: u64,
    exceeded: bool,
}

impl Write for PayloadCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let total = self.bytes.saturating_add(bytes.len() as u64);
        if total > self.limit {
            self.exceeded = true;
            return Err(std::io::Error::other("导出候选超过解码字节上限"));
        }
        self.bytes = total;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn payload_size(value: &impl serde::Serialize, decode_limit: u64) -> Result<u64, StoreError> {
    let mut counter = PayloadCounter {
        bytes: 0,
        limit: decode_limit,
        exceeded: false,
    };
    let result = serde_json::to_writer(&mut counter, value);
    if counter.exceeded {
        return Err(StoreError::ExportCandidateTooLarge {
            byte_limit: decode_limit,
        });
    }
    result?;
    Ok(counter.bytes)
}

/// 仅供已授权文件导出使用的临时队列；独立于自动存档槽，关闭连接即删除。
/// byte_limit 约束 payload 总字节；SQLite 页预算另留 4 MiB，cache 目标为 2 MiB。
pub struct DayEndExportSpool {
    connection: Connection,
    byte_limit: u64,
}

impl DayEndExportSpool {
    pub fn new(byte_limit: u64) -> Result<Self, StoreError> {
        if byte_limit == 0 {
            return Err(StoreError::Invalid("导出队列容量必须大于零".into()));
        }
        // SQLite 空文件名创建由连接独占、关闭自动删除的临时文件，不能用 :memory:
        // 保存大候选，也不把导出历史加入用户的单槽主数据库。
        let connection = Connection::open("")?;
        connection.pragma_update(None, "temp_store", "FILE")?;
        let pages = byte_limit
            .div_ceil(4096)
            .saturating_add(1024)
            .min(4_294_967_294);
        connection.pragma_update(None, "page_size", 4096)?;
        connection.pragma_update(None, "auto_vacuum", "FULL")?;
        connection.pragma_update(None, "max_page_count", pages)?;
        connection.pragma_update(None, "cache_size", -2048)?;
        connection.pragma_update(None, "cache_spill", "ON")?;
        connection.pragma_update(None, "mmap_size", 0)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE subscribers (token TEXT PRIMARY KEY NOT NULL);
             CREATE TABLE candidates (seq TEXT PRIMARY KEY NOT NULL, civil_date TEXT NOT NULL, payload TEXT NOT NULL, bytes INTEGER NOT NULL CHECK(bytes > 0));
             CREATE TABLE owners (token TEXT NOT NULL REFERENCES subscribers(token) ON DELETE CASCADE, seq TEXT NOT NULL REFERENCES candidates(seq) ON DELETE CASCADE, PRIMARY KEY(token,seq));",
        )?;
        Ok(Self {
            connection,
            byte_limit,
        })
    }

    pub fn subscribe(&mut self, token: &str) -> Result<(), StoreError> {
        validate_text(token, "导出订阅 token")?;
        self.connection
            .execute("INSERT INTO subscribers(token) VALUES (?1)", [token])?;
        Ok(())
    }

    pub fn subscription_tokens(&self) -> Result<Vec<String>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT token FROM subscribers ORDER BY token")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn unsubscribe(&mut self, token: &str) -> Result<(), StoreError> {
        let transaction = self.connection.transaction()?;
        // 断连或换代后的重复清理是无副作用操作，不会复活订阅。
        transaction.execute("DELETE FROM subscribers WHERE token = ?1", [token])?;
        transaction.execute("DELETE FROM candidates WHERE NOT EXISTS (SELECT 1 FROM owners WHERE owners.seq = candidates.seq)", [])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn publish(&mut self, candidate: &DayEndCandidate) -> Result<(), StoreError> {
        let subscribed: bool =
            self.connection
                .query_row("SELECT EXISTS(SELECT 1 FROM subscribers)", [], |row| {
                    row.get(0)
                })?;
        if !subscribed {
            return Ok(());
        }
        let date = candidate
            .civil_clock
            .settled_through
            .ok_or_else(|| StoreError::Invalid("导出候选缺少成功日结日期".into()))?
            .to_iso();
        // 容量不足时只计数字节，不能先分配完整大字符串后才发现队列已满。
        let bytes = payload_size(
            candidate.as_slot(),
            engine::SaveDecodeLimits::default().max_total_bytes as u64,
        )?;
        let retained = self.retained_bytes()?;
        if bytes > self.byte_limit.saturating_sub(retained) {
            return Err(StoreError::ExportCapacity {
                retained_bytes: retained,
                candidate_bytes: bytes,
                byte_limit: self.byte_limit,
            });
        }
        let mut payload = Vec::with_capacity(bytes as usize);
        serde_json::to_writer(&mut payload, candidate.as_slot())?;
        let payload = String::from_utf8(payload)
            .map_err(|error| StoreError::Invalid(format!("导出候选 JSON 不是 UTF-8：{error}")))?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO candidates(seq,civil_date,payload,bytes) VALUES (?1,?2,?3,?4)",
            params![candidate.snapshot.seq.to_string(), date, payload, bytes],
        )?;
        transaction.execute(
            "INSERT INTO owners(token,seq) SELECT token,?1 FROM subscribers",
            [candidate.snapshot.seq.to_string()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn read(
        &self,
        token: &str,
        key: &SaveCandidateKey,
    ) -> Result<engine::SaveSlot, StoreError> {
        let payload = self.connection.query_row(
            "SELECT candidates.payload FROM candidates JOIN owners USING(seq) WHERE owners.token = ?1 AND candidates.seq = ?2 AND candidates.civil_date = ?3",
            params![token, key.seq.to_string(), key.settled_date.to_iso()],
            |row| row.get::<_, String>(0),
        ).optional()?.ok_or_else(|| StoreError::Invalid(format!("导出订阅没有候选 {} / {}；可能已确认、已断连或不属于该订阅", key.settled_date.to_iso(), key.seq)))?;
        let slot = decode_day_end(&payload)?;
        if slot.snapshot.seq != key.seq
            || slot.civil_clock.settled_through != Some(key.settled_date)
        {
            return Err(StoreError::Invalid(
                "导出候选 payload 与精确日期/seq 索引不一致".into(),
            ));
        }
        Ok(slot)
    }

    pub fn acknowledge(&mut self, token: &str, key: &SaveCandidateKey) -> Result<(), StoreError> {
        let transaction = self.connection.transaction()?;
        let removed = transaction.execute(
            "DELETE FROM owners WHERE token = ?1 AND seq = ?2 AND EXISTS (SELECT 1 FROM candidates WHERE candidates.seq = owners.seq AND civil_date = ?3)",
            params![token, key.seq.to_string(), key.settled_date.to_iso()],
        )?;
        if removed != 1 {
            return Err(StoreError::Invalid(
                "导出确认不属于订阅中的精确日终候选".into(),
            ));
        }
        transaction.execute("DELETE FROM candidates WHERE NOT EXISTS (SELECT 1 FROM owners WHERE owners.seq = candidates.seq)", [])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn retained_bytes(&self) -> Result<u64, StoreError> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(SUM(bytes),0) FROM candidates",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn pending_bytes(&self, token: &str) -> Result<u64, StoreError> {
        Ok(self.connection.query_row("SELECT COALESCE(SUM(bytes),0) FROM candidates JOIN owners USING(seq) WHERE owners.token = ?1", [token], |row| row.get(0))?)
    }

    pub fn reset(&mut self) -> Result<(), StoreError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM subscribers", [])?;
        transaction.execute("DELETE FROM candidates", [])?;
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_counter_matches_utf8_wire_size_and_rejects_before_allocation() {
        let value = "中文";
        assert_eq!(payload_size(&value, 8).unwrap(), 8);
        assert!(matches!(
            payload_size(&value, 7),
            Err(StoreError::ExportCandidateTooLarge { byte_limit: 7 })
        ));
    }

    #[test]
    fn temporary_database_has_explicit_disk_and_cache_budgets() {
        let spool = DayEndExportSpool::new(4096).unwrap();
        for (pragma, expected) in [
            ("temp_store", 1),
            ("cache_size", -2048),
            ("auto_vacuum", 1),
            ("max_page_count", 1025),
        ] {
            let actual: i64 = spool
                .connection
                .pragma_query_value(None, pragma, |row| row.get(0))
                .unwrap();
            assert_eq!(actual, expected, "{pragma}");
        }
    }
}
