//! 私有 actor 合同经真实 SQLite lane 和 Tauri delivery 执行；不冒充 Linux 窗口运行。
use super::*;
use engine::session::authority_contract::{self, ContractPort};
use engine::session::checkpoint::MAX_PENDING_CHECKPOINTS;
use engine::session::publication::lifecycle::ObserverLease;
use serde_json::{json, Value};

struct DesktopTrace {
    harness: ActorHarness,
    database: NativeDatabase,
    directory: std::path::PathBuf,
    wire: Arc<std::sync::Mutex<Vec<Value>>>,
    ack: AckRoute,
    restore_slot: Option<SaveSlot>,
    durable_key: Option<CheckpointKey>,
    failed_key: Option<CheckpointKey>,
    old_lease: Option<ObserverLease>,
    terminal: Option<HostFailure>,
}

impl DesktopTrace {
    fn new() -> Self {
        let game = ProtocolSession::new(authority_contract::setup(), 7).unwrap();
        let mut harness = ActorHarness::new_protocol_actor(
            game,
            false,
            PausePreferences::default(),
            "authority-contract",
            "authority-contract",
        );
        harness
            .actor
            .pacing
            .set_running(false, 0, native_async::now());
        let directory = std::env::temp_dir().join(format!(
            "desktop-authority-contract-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&directory).unwrap();
        let database = NativeDatabase::open(&directory.join("archive.sqlite")).unwrap();
        let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
        let completion = harness.cmd_tx.clone();
        harness.actor.archive_lane = Some(ArchiveLane::spawn(move |key, result| {
            completion
                .send(SessionCommand::ArchiveWriteCompleted { key, result })
                .expect("合同 actor 必须存活到归档排空");
        }));
        harness.actor.archive = Some((writer, database.clone()));
        let ack = AckRoute::default();
        harness.actor.delivery = Delivery::start(
            harness.app.handle().clone(),
            "authority-contract".into(),
            "authority-contract".into(),
            ack.clone(),
        );
        Self {
            harness,
            database,
            directory,
            wire: Arc::new(std::sync::Mutex::new(Vec::new())),
            ack,
            restore_slot: None,
            durable_key: None,
            failed_key: None,
            old_lease: None,
            terminal: None,
        }
    }

    async fn step(&mut self) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        let generation = self.harness.actor.generation;
        self.harness
            .actor
            .handle_command(SessionCommand::Step { generation, reply })
            .await;
        result
            .await
            .map_err(|error| error.to_string())?
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    async fn civil(&mut self) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        let generation = self.harness.actor.generation;
        self.harness
            .actor
            .handle_command(SessionCommand::AdvanceCivilDay { generation, reply })
            .await;
        result
            .await
            .map_err(|error| error.to_string())?
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    async fn reach_boundary(&mut self) -> Result<(), String> {
        let mut count = 0;
        while !self
            .harness
            .actor
            .game
            .civil_day_ready()
            .map_err(|error| error.to_string())?
        {
            self.step().await?;
            count += 1;
            if count > 1000 {
                return Err("合同 fixture 超过短日限制".into());
            }
        }
        Ok(())
    }
    async fn flush_archive(&mut self) -> Result<(), String> {
        self.harness
            .actor
            .archive_lane
            .as_ref()
            .ok_or("归档 lane 缺失")?
            .execute(|| Ok(()))
            .await
            .map_err(|error| error.to_string())?;
        while let Ok(command) = self.harness.actor.cmd_rx.try_recv() {
            if !matches!(&command, SessionCommand::ArchiveWriteCompleted { .. }) {
                return Err("归档 barrier 出现非收据命令".into());
            }
            self.harness.actor.handle_command(command).await;
        }
        Ok(())
    }
    fn read_persisted_slot(&self) -> Result<SaveSlot, String> {
        // 独立只读 SQLite 连接不申请第二个 writer lease，仍读取真实文件的已提交事务。
        let connection = native_store::rusqlite::Connection::open_with_flags(
            self.directory.join("archive.sqlite"),
            native_store::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|error| error.to_string())?;
        let payload: String = connection
            .query_row(
                "SELECT payload FROM archive_slots WHERE slot_id = ?1",
                ["current"],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        engine::decode_save_slot(payload.as_bytes(), &engine::SaveDecodeLimits::default())
            .map_err(|error| error.to_string())
    }
    async fn finish_day_durable(&mut self) -> Result<(), String> {
        self.reach_boundary().await?;
        self.civil().await?;
        self.flush_archive().await?;
        let stored = self.read_persisted_slot()?;
        let active = self
            .database
            .load("current")
            .map_err(|error| error.to_string())?
            .ok_or("活动数据库缺少归档")?;
        if active.snapshot.seq != stored.snapshot.seq
            || active.civil_clock.settled_through != stored.civil_clock.settled_through
        {
            return Err("独立只读连接与活动数据库的快槽不一致".into());
        }
        let key = self
            .harness
            .actor
            .archive_order
            .durable()
            .ok_or("真实收据未推进 durable")?;
        if stored.snapshot.seq != key.seq
            || stored.civil_clock.settled_through != Some(key.settled_date)
        {
            return Err("SQLite 与真实收据不一致".into());
        }
        if !stored.market_minute_bars.values().all(Vec::is_empty) {
            return Err("日终未清理分钟行情".into());
        }
        self.durable_key = Some(key);
        Ok(())
    }
    async fn inject_receipt(&mut self, key: CheckpointKey) {
        self.harness
            .actor
            .handle_command(SessionCommand::ArchiveWriteCompleted {
                key,
                result: Ok(()),
            })
            .await;
    }
    async fn rebind_observer(&mut self) -> Result<(), String> {
        let generation = self.harness.actor.generation;
        let current = self
            .harness
            .actor
            .delivery
            .inspect_observer()
            .await?
            .lease
            .ok_or("观察 lease 缺失")?;
        self.old_lease = Some(current);
        let lease = current.lease.checked_add(1).ok_or("观察 lease 溢出")?;
        self.ack.set_held(generation, lease, true).await?;
        let ordinal = self.harness.actor.delivery.offer_baseline_fence();
        let (reply, result) = oneshot::channel();
        self.harness
            .actor
            .delivery
            .resume_after(lease, ordinal, reply);
        result.await.map_err(|error| error.to_string())??;
        self.ack.set_held(generation, lease, false).await
    }
}

impl ContractPort for DesktopTrace {
    async fn perform(&mut self, id: &str) -> Result<(), String> {
        match id {
            "initial" => {
                self.harness.actor.delivery.inspect_observer().await?;
            }
            "manual" => self.step().await?,
            "without-observer" => self.finish_day_durable().await?,
            "slow-observer" => {
                use tauri::Listener;
                for name in [crate::ENGINE_EVENT_NAME, "engine-failure"] {
                    let wire = Arc::clone(&self.wire);
                    self.harness.app.listen(name, move |event| {
                        wire.lock().unwrap().push(json!({"event": name, "payload": serde_json::from_str::<Value>(event.payload()).unwrap()}));
                    });
                }
                self.harness.subscribe_engine_events();
                self.harness.subscribe_failures();
                self.finish_day_durable().await?;
                if self
                    .harness
                    .actor
                    .delivery
                    .inspect_observer()
                    .await?
                    .delivery
                    .accepted
                    != 0
                {
                    return Err("慢观察者不应有应用 ACK".into());
                }
            }
            "archive-full" => {
                self.reach_boundary().await?;
                let (entered, ready) = oneshot::channel();
                let (release, blocked) = std::sync::mpsc::channel();
                let lane = self
                    .harness
                    .actor
                    .archive_lane
                    .as_ref()
                    .ok_or("归档 lane 缺失")?;
                let waiting = lane
                    .try_execute(move || {
                        let _ = entered.send(());
                        blocked
                            .recv_timeout(Duration::from_secs(2))
                            .map_err(|error| StoreError::Invalid(error.to_string()))?;
                        Ok(())
                    })
                    .map_err(|error| error.to_string())?;
                ready.await.map_err(|error| error.to_string())?;
                for _ in 0..MAX_PENDING_CHECKPOINTS {
                    let _ = lane
                        .try_execute(|| Ok(()))
                        .map_err(|error| error.to_string())?;
                }
                let failed = self.civil().await;
                release.send(()).map_err(|error| error.to_string())?;
                waiting
                    .await
                    .map_err(|error| error.to_string())?
                    .map_err(|error| error.to_string())?;
                self.flush_archive().await?;
                if failed.is_err() || !self.harness.actor.archive_order.is_blocked() {
                    return Err("已提交日界应保留成功响应，同时归档进入 blocked".into());
                }
                self.failed_key = self
                    .harness
                    .actor
                    .archive_order
                    .trace()
                    .queued
                    .first()
                    .copied();
                self.restore_slot = Some(
                    self.harness
                        .actor
                        .game
                        .save()
                        .map_err(|error| error.to_string())?,
                );
            }
            "blocked" => {
                let before = (
                    self.harness.actor.game.tick(),
                    self.harness.actor.game.seq(),
                );
                if self.step().await.is_ok()
                    || before
                        != (
                            self.harness.actor.game.tick(),
                            self.harness.actor.game.seq(),
                        )
                {
                    return Err("blocked 仍推进市场".into());
                }
            }
            "old-receipt" => {
                let mut key = self.failed_key.ok_or("失败 key 缺失")?;
                key.writer_epoch = 0;
                self.inject_receipt(key).await;
            }
            "bad-receipt" => {
                let mut key = self.failed_key.ok_or("失败 key 缺失")?;
                key.seq += 1;
                self.inject_receipt(key).await;
            }
            "duplicate-receipt" => {
                self.inject_receipt(self.durable_key.ok_or("耐久 key 缺失")?)
                    .await
            }
            "prepare-rejected" => {
                let generation = self.harness.actor.generation;
                let slot = self.restore_slot.clone().ok_or("恢复档缺失")?;
                if self
                    .harness
                    .actor
                    .restore(generation - 1, Box::new(slot))
                    .await
                    .is_ok()
                    || self.harness.actor.generation != generation
                {
                    return Err("prepare 拒绝改变了会话".into());
                }
            }
            "replaced" => {
                let generation = self.harness.actor.generation;
                let slot = self.restore_slot.clone().ok_or("恢复档缺失")?;
                self.harness
                    .actor
                    .restore(generation, Box::new(slot))
                    .await
                    .map_err(|error| error.to_string())?;
                if self.harness.actor.generation != generation + 1 {
                    return Err("恢复没有换代".into());
                }
            }
            "old-generation" => {
                self.inject_receipt(self.failed_key.ok_or("失败 key 缺失")?)
                    .await;
                let generation = self.harness.actor.generation;
                let before = (
                    self.harness.actor.game.tick(),
                    self.harness.actor.game.seq(),
                );
                let (reply, result) = oneshot::channel();
                self.harness
                    .actor
                    .handle_command(SessionCommand::Step {
                        generation: generation - 1,
                        reply,
                    })
                    .await;
                if result.await.map_err(|error| error.to_string())?.is_ok()
                    || before
                        != (
                            self.harness.actor.game.tick(),
                            self.harness.actor.game.seq(),
                        )
                {
                    return Err("旧 generation 命令被接受或改变了会话".into());
                }
                let observer = self.harness.actor.delivery.inspect_observer().await?;
                let lease = observer.lease.ok_or("观察 lease 缺失")?;
                if self
                    .ack
                    .acknowledge(generation - 1, lease.lease, 0)
                    .await
                    .is_ok()
                    || self
                        .ack
                        .set_held(generation - 1, lease.lease, false)
                        .await
                        .is_ok()
                {
                    return Err("旧 generation 发布 ACK 或 release 被接受".into());
                }
                let after = self.harness.actor.delivery.inspect_observer().await?;
                if after.lease != observer.lease
                    || after.delivery.accepted != observer.delivery.accepted
                    || after.held != observer.held
                {
                    return Err("旧 generation 改变了真实观察流".into());
                }
            }
            "new-lease" => self.rebind_observer().await?,
            "old-lease" => {
                let old = self.old_lease.ok_or("旧 lease 缺失")?;
                if self
                    .ack
                    .acknowledge(old.generation, old.lease, 0)
                    .await
                    .is_ok()
                    || self
                        .ack
                        .set_held(old.generation, old.lease, false)
                        .await
                        .is_ok()
                {
                    return Err("旧 lease 解锁了新观察流".into());
                }
                let ordinal = self.harness.actor.delivery.offer_baseline_fence();
                let (reply, result) = oneshot::channel();
                self.harness
                    .actor
                    .delivery
                    .resume_after(old.lease, ordinal, reply);
                if result.await.map_err(|error| error.to_string())?.is_ok() {
                    return Err("旧 lease 的 cut 被接受".into());
                }
            }
            "prefix-failure" => {
                while self
                    .harness
                    .actor
                    .game
                    .civil_day_ready()
                    .map_err(|error| error.to_string())?
                {
                    self.civil().await?;
                }
                self.flush_archive().await?;
                self.step().await?;
                let failure = HostFailure::step(engine::session::StepFatal::InvariantViolation {
                    location: "authority.contract".into(),
                    description: "合同注入：已提交前缀之后失败".into(),
                });
                self.terminal = Some(failure.clone());
                self.harness.actor.stop_after_host_failure(failure);
            }
            "terminal-repeat" => {
                self.harness
                    .actor
                    .stop_after_host_failure(self.terminal.clone().ok_or("terminal 缺失")?);
                if self.step().await.is_ok() {
                    return Err("terminal 后仍允许 step".into());
                }
            }
            "shutdown" => {
                let (reply, result) = oneshot::channel();
                self.harness
                    .actor
                    .handle_command(SessionCommand::Shutdown { reply })
                    .await;
                result.await.map_err(|error| error.to_string())??;
                self.harness.actor.delivery.finish().await?;
                self.read_persisted_slot()?;
                let wire = self.wire.lock().unwrap();
                let terminals: Vec<_> = wire
                    .iter()
                    .enumerate()
                    .filter(|(_, value)| value["payload"]["code"] == "STEP_FATAL")
                    .collect();
                if terminals.len() != 1 {
                    return Err(format!(
                        "真实 IPC terminal 数量应为1，实际{}",
                        terminals.len()
                    ));
                }
                let (index, terminal) = terminals[0];
                let prefix_seen = wire[..index].iter().any(|value| {
                    value["event"] == crate::ENGINE_EVENT_NAME
                        && value["payload"]["timeline_id"] == self.harness.actor.timeline_id
                });
                let missing = terminal["payload"]["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("未收到"));
                if !prefix_seen && !missing {
                    return Err("terminal 未提供已交付前缀或明确缺口".into());
                }
            }
            _ => return Err(format!("未知合同步骤：{id}")),
        }
        Ok(())
    }

    async fn snapshot(&mut self, id: &str) -> Result<Value, String> {
        let archive = self.harness.actor.archive_order.trace();
        let observer = self.harness.actor.delivery.inspect_observer().await?;
        let lease = observer.lease.ok_or("真实观察 lease 缺失")?;
        let key_json = |key: CheckpointKey| json!({"generation":key.generation.to_string(),"writerEpoch":key.writer_epoch.to_string(),"seq":key.seq.to_string()});
        Ok(
            json!({"id":id,"generation":self.harness.actor.generation.to_string(),"day":self.harness.actor.game.day(),"tick":self.harness.actor.game.tick().to_string(),"seq":self.harness.actor.game.seq().to_string(),"paused":!self.harness.actor.pacing.is_running(),"terminal":self.harness.actor.pacing.fatal_failure().map(|failure|failure.code),"archive":{"queued":archive.queued.into_iter().map(key_json).collect::<Vec<_>>(),"durable":archive.durable.map(key_json),"blocked":archive.blocked},"observer":{"generation":lease.generation.to_string(),"lease":lease.lease.to_string(),"sent":observer.delivery.sent.to_string(),"flushed":observer.delivery.flushed.to_string(),"accepted":observer.delivery.accepted.to_string()}}),
        )
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_authority_script_runs_against_real_desktop_actor() {
    let mut adapter = DesktopTrace::new();
    let trace = authority_contract::exercise(&mut adapter).await.unwrap();
    assert_eq!(trace.len(), 17);
    assert!(trace.iter().any(|row| row["archive"]["blocked"] == true));
    println!(
        "desktop real SQLite and Tauri delivery authority trace: {}",
        serde_json::to_string(&trace).unwrap()
    );
    let directory = adapter.directory.clone();
    drop(adapter);
    std::fs::remove_dir_all(directory).unwrap();
}
