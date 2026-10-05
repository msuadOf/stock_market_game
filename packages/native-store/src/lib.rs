pub mod identities;
mod writer_lock;

pub use rusqlite;

use engine::session::protocol::ProtocolSession;
use engine::SaveSlot;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("SQLite 操作失败：{0}")]
    Sql(#[from] rusqlite::Error),
    #[error("存档 JSON 无效：{0}")]
    Json(#[from] serde_json::Error),
    #[error("日终存档校验失败：{0}")]
    Session(#[from] engine::SessionError),
    #[error("Native 存储失败：{0}")]
    Invalid(String),
}

#[derive(Clone)]
pub struct NativeDatabase {
    connection: Arc<Mutex<Connection>>,
    generations: Arc<Mutex<BTreeMap<String, WriteGeneration>>>,
    file_lock: Option<Arc<std::fs::File>>,
}

struct WriteGeneration {
    generation: u64,
    last_seq: Option<u64>,
}

const ARCHIVES_SQL: &str = "CREATE TABLE archive_slots (slot_id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL, payload TEXT NOT NULL, civil_date TEXT NOT NULL, tick TEXT NOT NULL)";
const SUBJECTS_SQL: &str = "CREATE TABLE subjects (subject_id TEXT PRIMARY KEY NOT NULL, username TEXT UNIQUE, password_hash TEXT, CHECK ((username IS NULL AND password_hash IS NULL) OR (username IS NOT NULL AND password_hash IS NOT NULL)))";
const CREDENTIALS_SQL: &str = "CREATE TABLE credentials (credential_hash TEXT PRIMARY KEY NOT NULL, subject_id TEXT NOT NULL REFERENCES subjects(subject_id))";
const GRANTS_SQL: &str = "CREATE TABLE market_control_grants (market_id TEXT NOT NULL, subject_id TEXT NOT NULL REFERENCES subjects(subject_id), PRIMARY KEY (market_id, subject_id))";
const SELECTION_SQL: &str = "CREATE TABLE archive_selection (singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1), slot_id TEXT REFERENCES archive_slots(slot_id) ON DELETE SET NULL)";

#[derive(Clone, Debug, Serialize)]
pub struct ArchiveMetadata {
    pub slot_id: String,
    pub name: String,
    pub civil_date: String,
    pub tick: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveSelection {
    Uninitialized,
    Selected(String),
    Cleared,
}

#[derive(Clone)]
pub struct ArchiveWriter {
    database: NativeDatabase,
    slot_id: String,
    generation: u64,
}

#[derive(Serialize)]
#[serde(transparent)]
pub struct DayEndCandidate {
    slot: SaveSlot,
}

impl DayEndCandidate {
    pub fn capture(game: &ProtocolSession, key: &engine::session::protocol::SaveCandidateKey) -> Result<Self, StoreError> {
        let slot = game.save_candidate(key)?;
        ProtocolSession::restore(&slot)?;
        Ok(Self { slot })
    }

    pub fn as_slot(&self) -> &SaveSlot {
        &self.slot
    }
}

impl std::ops::Deref for DayEndCandidate {
    type Target = SaveSlot;

    fn deref(&self) -> &Self::Target {
        &self.slot
    }
}

impl NativeDatabase {
    pub fn selection(&self) -> Result<ArchiveSelection, StoreError> {
        self.with_connection(|connection| {
            let selected = connection
                .query_row(
                    "SELECT slot_id FROM archive_selection WHERE singleton = 1",
                    [],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?;
            Ok(match selected {
                None => ArchiveSelection::Uninitialized,
                Some(None) => ArchiveSelection::Cleared,
                Some(Some(slot_id)) => ArchiveSelection::Selected(slot_id),
            })
        })
    }

    pub fn select(&self, slot_id: &str) -> Result<(), StoreError> {
        validate_text(slot_id, "slot_id")?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            transaction.execute("INSERT INTO archive_selection(singleton,slot_id) VALUES (1,?1) ON CONFLICT(singleton) DO UPDATE SET slot_id=excluded.slot_id", [slot_id])?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if path.as_os_str().is_empty() || path == Path::new(":memory:") {
            return Err(StoreError::Invalid(
                "生产 SQLite 必须使用真实文件路径".into(),
            ));
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                StoreError::Invalid(format!(
                    "无法创建 SQLite 目录 {}：{error}",
                    parent.display()
                ))
            })?;
        }
        let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)
            .map_err(|error| StoreError::Invalid(format!("无法打开 SQLite writer 锁 {}：{error}", path.display())))?;
        writer_lock::try_lock(&lock).map_err(|error| StoreError::Invalid(format!("SQLite 文件 {} 已被另一 Native writer 使用或无法取得独占 writer lock：{error}；请关闭另一个 Server/Desktop 实例后重试", path.display())))?;
        let mut database = Self::from_connection(Connection::open(path)?)?;
        database.file_lock = Some(Arc::new(lock));
        if !path.is_file() {
            return Err(StoreError::Invalid(format!(
                "SQLite 未建立真实文件 {}",
                path.display()
            )));
        }
        Ok(database)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, StoreError> {
        let mode: String = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        if mode != "delete" && mode != "memory" {
            return Err(StoreError::Invalid(format!(
                "不支持 SQLite journal_mode={mode}；当前存档不使用 WAL 或旧数据库迁移"
            )));
        }
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != 0 {
            return Err(StoreError::Invalid(
                "数据库含旧 user_version；只支持当前完整结构，不执行迁移".into(),
            ));
        }
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL")?;
        let transaction = connection.transaction()?;
        let existing = {
            let mut statement = transaction.prepare(
                "SELECT name, sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<Result<BTreeMap<_, _>, _>>()?
        };
        let expected = BTreeMap::from([
            ("archive_slots".to_owned(), ARCHIVES_SQL.to_owned()),
            ("credentials".to_owned(), CREDENTIALS_SQL.to_owned()),
            ("subjects".to_owned(), SUBJECTS_SQL.to_owned()),
            ("market_control_grants".to_owned(), GRANTS_SQL.to_owned()),
            ("archive_selection".to_owned(), SELECTION_SQL.to_owned()),
        ]);
        if existing.is_empty() {
            for sql in [
                ARCHIVES_SQL,
                SUBJECTS_SQL,
                CREDENTIALS_SQL,
                GRANTS_SQL,
                SELECTION_SQL,
            ] {
                transaction.execute_batch(sql)?;
            }
        } else if existing != expected {
            return Err(StoreError::Invalid(
                "SQLite 结构不是当前完整 archive/identity 结构；请选择新数据库，不执行迁移".into(),
            ));
        }
        transaction.commit()?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            generations: Arc::new(Mutex::new(BTreeMap::new())),
            file_lock: None,
        })
    }

    pub fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| StoreError::Invalid(format!("SQLite lock poisoned: {error}")))?;
        operation(&mut connection)
    }

    pub fn list(&self) -> Result<Vec<ArchiveMetadata>, StoreError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT slot_id, name, civil_date, tick FROM archive_slots ORDER BY slot_id",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            rows.map(|row| {
                let (slot_id, name, civil_date, tick) = row?;
                engine::CivilDate::from_iso(&civil_date).map_err(|error| {
                    StoreError::Invalid(format!("槽 {slot_id} 日期元数据无效：{error}"))
                })?;
                let parsed_tick = tick.parse::<u64>().map_err(|error| {
                    StoreError::Invalid(format!("槽 {slot_id} tick 元数据无效：{error}"))
                })?;
                if parsed_tick.to_string() != tick {
                    return Err(StoreError::Invalid(format!(
                        "槽 {slot_id} tick 元数据不是规范十进制整数"
                    )));
                }
                Ok(ArchiveMetadata {
                    slot_id,
                    name,
                    civil_date,
                    tick: parsed_tick,
                })
            })
            .collect()
        })
    }

    pub fn load(&self, slot_id: &str) -> Result<Option<SaveSlot>, StoreError> {
        validate_text(slot_id, "slot_id")?;
        self.with_connection(|connection| {
            let payload = connection
                .query_row(
                    "SELECT payload FROM archive_slots WHERE slot_id = ?1",
                    [slot_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            payload.map(|payload| decode_day_end(&payload)).transpose()
        })
    }

    pub fn copy(
        &self,
        source: &str,
        target: &str,
        name: &str,
    ) -> Result<ArchiveMetadata, StoreError> {
        validate_text(source, "source slot_id")?;
        validate_text(target, "target slot_id")?;
        validate_text(name, "name")?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let payload = transaction.query_row("SELECT payload FROM archive_slots WHERE slot_id = ?1", [source], |row| row.get::<_, String>(0)).optional()?.ok_or_else(|| StoreError::Invalid(format!("复制来源槽 {source} 不存在")))?;
            let slot = decode_day_end(&payload)?;
            let civil_date = settled_date(&slot)?;
            transaction.execute("INSERT INTO archive_slots(slot_id,name,payload,civil_date,tick) VALUES (?1,?2,?3,?4,?5)", params![target, name, payload, civil_date, slot.snapshot.tick.to_string()])?;
            transaction.commit()?;
            Ok(ArchiveMetadata { slot_id: target.into(), name: name.into(), civil_date, tick: slot.snapshot.tick })
        })
    }

    pub fn rename(&self, slot_id: &str, name: &str) -> Result<(), StoreError> {
        validate_text(slot_id, "slot_id")?;
        validate_text(name, "name")?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if transaction.execute(
                "UPDATE archive_slots SET name = ?1 WHERE slot_id = ?2",
                params![name, slot_id],
            )? != 1
            {
                return Err(StoreError::Invalid(format!("重命名槽 {slot_id} 不存在")));
            }
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn delete(&self, slot_id: &str) -> Result<(), StoreError> {
        validate_text(slot_id, "slot_id")?;
        let mut generations = self
            .generations
            .lock()
            .map_err(|error| StoreError::Invalid(format!("存档代际锁失效：{error}")))?;
        let next = generations.get(slot_id).map_or(Ok(1), |state| {
            state
                .generation
                .checked_add(1)
                .ok_or_else(|| StoreError::Invalid("存档代际耗尽".into()))
        })?;
        self.delete_payload(slot_id)?;
        generations.insert(
            slot_id.into(),
            WriteGeneration {
                generation: next,
                last_seq: None,
            },
        );
        Ok(())
    }

    fn delete_payload(&self, slot_id: &str) -> Result<(), StoreError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            if transaction.execute("DELETE FROM archive_slots WHERE slot_id = ?1", [slot_id])? != 1
            {
                return Err(StoreError::Invalid(format!("删除槽 {slot_id} 不存在")));
            }
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn journal_mode(&self) -> Result<String, StoreError> {
        self.with_connection(|connection| {
            Ok(connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?)
        })
    }

    pub fn grant_market_control(
        &self,
        market_id: &str,
        subject_id: &str,
    ) -> Result<(), StoreError> {
        validate_text(market_id, "market_id")?;
        validate_text(subject_id, "subject_id")?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            transaction.execute("INSERT INTO market_control_grants(market_id,subject_id) VALUES (?1,?2) ON CONFLICT(market_id,subject_id) DO NOTHING", params![market_id, subject_id])?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn market_controllers(&self, market_id: &str) -> Result<Vec<String>, StoreError> {
        validate_text(market_id, "market_id")?;
        self.with_connection(|connection| {
            let mut statement = connection.prepare("SELECT subject_id FROM market_control_grants WHERE market_id = ?1 ORDER BY subject_id")?;
            let rows = statement.query_map([market_id], |row| row.get(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }
}

impl ArchiveWriter {
    pub fn rename_slot(&self, slot_id: &str, name: &str) -> Result<(), StoreError> {
        let generations = self.database.generations.lock().map_err(|error| StoreError::Invalid(format!("存档代际锁失效：{error}")))?;
        self.require_current_owner(&generations)?;
        self.database.rename(slot_id, name)
    }

    pub fn copy_slot(
        &self,
        slot_id: &str,
        target: &str,
        name: &str,
    ) -> Result<ArchiveMetadata, StoreError> {
        let generations = self.database.generations.lock().map_err(|error| StoreError::Invalid(format!("存档代际锁失效：{error}")))?;
        self.require_current_owner(&generations)?;
        self.database.copy(slot_id, target, name)
    }
    pub fn delete_slot(&self, slot_id: &str, new_slot_id: &str) -> Result<Self, StoreError> {
        validate_text(slot_id, "slot_id")?;
        let mut generations = self.database.generations.lock().map_err(|error| StoreError::Invalid(format!("存档代际锁失效：{error}")))?;
        self.require_current_owner(&generations)?;
        if slot_id != self.slot_id {
            let next = generations.get(slot_id).map_or(Ok(1), |state| state.generation.checked_add(1).ok_or_else(|| StoreError::Invalid("存档代际耗尽".into())))?;
            self.database.delete_payload(slot_id)?;
            generations.insert(slot_id.into(), WriteGeneration { generation: next, last_seq: None });
            return Ok(self.clone());
        }
        validate_text(new_slot_id, "new_slot_id")?;
        if new_slot_id == slot_id {
            return Err(StoreError::Invalid(
                "删除活动槽必须使用新的内存保存目标".into(),
            ));
        }
        let current = generations
            .get(slot_id)
            .ok_or_else(|| StoreError::Invalid("存档目标未激活".into()))?;
        if current.generation != self.generation {
            return Err(StoreError::Invalid("旧时间线不能删除当前活动槽".into()));
        }
        let old_next = current
            .generation
            .checked_add(1)
            .ok_or_else(|| StoreError::Invalid("存档代际耗尽".into()))?;
        if generations.contains_key(new_slot_id) {
            return Err(StoreError::Invalid("删除后的保存目标必须是新槽".into()));
        }
        self.database.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM archive_slots WHERE slot_id = ?1)",
                [new_slot_id],
                |row| row.get(0),
            )?;
            if exists {
                return Err(StoreError::Invalid("删除后的保存目标必须是新槽".into()));
            }
            if transaction.execute("DELETE FROM archive_slots WHERE slot_id = ?1", [slot_id])? != 1
            {
                return Err(StoreError::Invalid(format!("删除槽 {slot_id} 不存在")));
            }
            transaction.commit()?;
            Ok(())
        })?;
        generations.insert(
            slot_id.into(),
            WriteGeneration {
                generation: old_next,
                last_seq: None,
            },
        );
        generations.insert(
            new_slot_id.into(),
            WriteGeneration {
                generation: 1,
                last_seq: None,
            },
        );
        Ok(Self {
            database: self.database.clone(),
            slot_id: new_slot_id.into(),
            generation: 1,
        })
    }

    fn require_current_owner(&self, generations: &BTreeMap<String, WriteGeneration>) -> Result<(), StoreError> {
        if generations.get(&self.slot_id).is_none_or(|state| state.generation != self.generation) {
            return Err(StoreError::Invalid("旧时间线的 writer 已失效，不能修改当前存档目标或槽元数据".into()));
        }
        Ok(())
    }

    pub fn select(&self, slot_id: &str) -> Result<(), StoreError> {
        if slot_id != self.slot_id {
            return Err(StoreError::Invalid(
                "请先成功加载目标槽，再记录启动槽选择".into(),
            ));
        }
        let generations = self
            .database
            .generations
            .lock()
            .map_err(|error| StoreError::Invalid(format!("存档代际锁失效：{error}")))?;
        if generations
            .get(&self.slot_id)
            .is_none_or(|state| state.generation != self.generation)
        {
            return Err(StoreError::Invalid("旧时间线的启动槽选择请求已失效".into()));
        }
        self.database.select(slot_id)
    }

    pub fn activate(database: NativeDatabase, slot_id: &str) -> Result<Self, StoreError> {
        validate_text(slot_id, "slot_id")?;
        let generation = {
            let mut generations = database
                .generations
                .lock()
                .map_err(|error| StoreError::Invalid(format!("存档代际锁失效：{error}")))?;
            let next = generations.get(slot_id).map_or(Ok(1), |state| {
                state
                    .generation
                    .checked_add(1)
                    .ok_or_else(|| StoreError::Invalid("存档代际耗尽".into()))
            })?;
            generations.insert(
                slot_id.into(),
                WriteGeneration {
                    generation: next,
                    last_seq: None,
                },
            );
            next
        };
        Ok(Self {
            database,
            slot_id: slot_id.into(),
            generation,
        })
    }

    pub fn renew(&self, slot_id: Option<&str>) -> Result<Self, StoreError> {
        self.renew_after(slot_id, || Ok(()))
    }

    pub fn renew_after(
        &self,
        slot_id: Option<&str>,
        commit: impl FnOnce() -> Result<(), StoreError>,
    ) -> Result<Self, StoreError> {
        let target = slot_id.unwrap_or(&self.slot_id);
        validate_text(target, "slot_id")?;
        let mut generations = self
            .database
            .generations
            .lock()
            .map_err(|error| StoreError::Invalid(format!("存档代际锁失效：{error}")))?;
        if generations.get(&self.slot_id).is_none_or(|state| state.generation != self.generation) {
            return Err(StoreError::Invalid("旧时间线的 writer 不能抢占当前保存目标或提交新时间线".into()));
        }
        let next_generation = |slot_id: &str| {
            generations.get(slot_id).map_or(Ok(1), |state| {
                state
                    .generation
                    .checked_add(1)
                    .ok_or_else(|| StoreError::Invalid("存档代际耗尽".into()))
            })
        };
        let target_generation = next_generation(target)?;
        let old_generation = next_generation(&self.slot_id)?;
        commit()?;
        if target != self.slot_id {
            generations.insert(
                self.slot_id.clone(),
                WriteGeneration {
                    generation: old_generation,
                    last_seq: None,
                },
            );
        }
        generations.insert(
            target.into(),
            WriteGeneration {
                generation: target_generation,
                last_seq: None,
            },
        );
        Ok(Self {
            database: self.database.clone(),
            slot_id: target.into(),
            generation: target_generation,
        })
    }

    pub fn save_day_end(&self, candidate: &DayEndCandidate) -> Result<(), StoreError> {
        let slot = candidate.as_slot();
        ProtocolSession::restore(slot)?;
        let civil_date = settled_date(slot)?;
        let payload = serde_json::to_string(slot)?;
        let mut generations = self
            .database
            .generations
            .lock()
            .map_err(|error| StoreError::Invalid(format!("存档代际锁失效：{error}")))?;
        let state = generations
            .get_mut(&self.slot_id)
            .ok_or_else(|| StoreError::Invalid("存档目标未激活".into()))?;
        if state.generation != self.generation
            || state.last_seq.is_some_and(|seq| seq >= slot.snapshot.seq)
        {
            return Err(StoreError::Invalid(
                "旧日终写入已失效，不能覆盖当前时间线存档".into(),
            ));
        }
        self.database.with_connection(|connection| {
            let transaction = connection.transaction()?;
            transaction.execute("INSERT INTO archive_slots(slot_id,name,payload,civil_date,tick) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(slot_id) DO UPDATE SET payload=excluded.payload,civil_date=excluded.civil_date,tick=excluded.tick", params![self.slot_id, self.slot_id, payload, civil_date, slot.snapshot.tick.to_string()])?;
            transaction.execute("INSERT INTO archive_selection(singleton,slot_id) VALUES (1,?1) ON CONFLICT(singleton) DO UPDATE SET slot_id=excluded.slot_id", [&self.slot_id])?;
            transaction.commit()?;
            Ok(())
        })?;
        state.last_seq = Some(slot.snapshot.seq);
        Ok(())
    }
}

fn validate_text(value: &str, field: &str) -> Result<(), StoreError> {
    if value.trim().is_empty() || value.contains('\0') {
        return Err(StoreError::Invalid(format!("{field} 不可为空或含 NUL")));
    }
    Ok(())
}

fn settled_date(slot: &SaveSlot) -> Result<String, StoreError> {
    slot.civil_clock
        .settled_through
        .map(|date| date.to_iso())
        .ok_or_else(|| StoreError::Invalid("存档必须包含已完成自然日日结日期".into()))
}

fn decode_day_end(payload: &str) -> Result<SaveSlot, StoreError> {
    let slot = engine::decode_save_slot(payload.as_bytes(), &engine::SaveDecodeLimits::default())?;
    ProtocolSession::restore(&slot)?;
    Ok(slot)
}
