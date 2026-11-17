//! 真实 Server actor、SQLite 文件、ArchiveLane 与 publisher 组件的同脚本验收。
//! 本文件不启动 HTTP/WebSocket，不把组件轨迹冒充浏览器或 socket E2E。
use super::*;
use engine::session::authority_contract::{exercise, ContractPort};
use engine::session::checkpoint::{CheckpointKey, MAX_PENDING_CHECKPOINTS};
use serde_json::{json, Value};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_authority_script_uses_real_server_lane_sqlite_and_publisher() {
    let mut port = ServerTracePort::new().unwrap();
    let rows = exercise(&mut port).await.unwrap();
    assert_eq!(rows.len(), 17);
    assert_eq!(rows[4]["archive"]["blocked"], true);
    assert_eq!(rows[10]["generation"], "2");
    assert_eq!(rows[14]["terminal"], "STEP_FATAL");
    println!(
        "server actor+sqlite+publisher component trace: {}",
        serde_json::to_string(&rows).unwrap()
    );
}

struct DatabaseFile(std::path::PathBuf);
impl Drop for DatabaseFile {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let path = std::path::PathBuf::from(format!("{}{suffix}", self.0.display()));
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    if std::thread::panicking() {
                        eprintln!("验收 SQLite 清理次错：{error}");
                    } else {
                        panic!("验收 SQLite 清理失败：{error}");
                    }
                }
            }
        }
    }
}
struct ServerTracePort {
    actor: SessionActor,
    database: native_store::NativeDatabase,
    commands: mpsc::UnboundedSender<SessionCommand>,
    observer: Option<crate::routes::AuthorityTraceObserver>,
    observer_snapshot: Value,
    subject: OpaqueSubjectId,
    first_key: Option<CheckpointKey>,
    second_key: Option<CheckpointKey>,
    failed_key: Option<CheckpointKey>,
    replacement: Option<SaveSlot>,
    terminal: Option<HostFailure>,
    file: DatabaseFile,
}
impl ServerTracePort {
    fn new() -> Result<Self, String> {
        let file = DatabaseFile(
            std::env::temp_dir().join(format!("session-authority-{}.sqlite", uuid::Uuid::new_v4())),
        );
        let database = native_store::NativeDatabase::open(&file.0).map_err(|e| e.to_string())?;
        let writer = native_store::ArchiveWriter::activate(database.clone(), "trace")
            .map_err(|e| e.to_string())?;
        let game = ProtocolSession::new(engine::session::authority_contract::setup(), 7)
            .map_err(|e| e.to_string())?;
        let subject = OpaqueSubjectId::new("local-owner".into()).map_err(|e| e.to_string())?;
        let (commands, cmd_rx) = mpsc::unbounded_channel();
        let receipts = commands.clone();
        let lane = ArchiveLane::spawn(move |key, result| {
            if receipts
                .send(SessionCommand::ArchiveWriteCompleted { key, result })
                .is_err()
            {
                eprintln!("验收 actor 收据入口提前关闭");
            }
        });
        let (event_tx, events) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        drop(events);
        let actor = SessionActor {
            ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
            members: Arc::new(RwLock::new(game.trading_subject_accounts())),
            controllers: std::collections::BTreeSet::from([subject.clone()]),
            archive: Some(writer),
            archive_lane: Some(lane),
            archive_epoch: 1,
            archive_order: ArchiveAuthority::new(1, 1),
            injected_step_failure: None,
            pacing: ServerPacing::new(1_000, game.tick(), native_async::now()),
            game,
            runtime_topology: Default::default(),
            cmd_rx,
            event_tx,
            observers: Vec::new(),
            session_id: "authority-trace".into(),
            public_revision: 0,
            timeline_generation: 1,
            pause_preferences: PausePreferences::default(),
            fatal_failure: None,
            deferred_drop_reported: false,
        };
        Ok(Self {
            actor,
            database,
            commands,
            observer: None,
            observer_snapshot: Value::Null,
            subject,
            first_key: None,
            second_key: None,
            failed_key: None,
            replacement: None,
            terminal: None,
            file,
        })
    }
    async fn step(&mut self) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        self.actor
            .handle_command(SessionCommand::Step {
                generation: self.actor.timeline_generation,
                reply,
            })
            .await;
        result
            .await
            .map_err(|e| e.to_string())?
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    async fn civil(&mut self) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        self.actor
            .handle_command(SessionCommand::AdvanceCivilDay {
                generation: self.actor.timeline_generation,
                reply,
            })
            .await;
        result
            .await
            .map_err(|e| e.to_string())?
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    async fn day(&mut self) -> Result<(), String> {
        for _ in 0..1_000 {
            if self
                .actor
                .game
                .civil_day_ready()
                .map_err(|e| e.to_string())?
            {
                return self.civil().await;
            }
            self.step().await?;
        }
        Err("短夹具未在1000拍内到达日界".into())
    }
    async fn drain_archive(&mut self) -> Result<(), String> {
        self.actor
            .archive_lane
            .as_ref()
            .ok_or("缺少真实ArchiveLane")?
            .execute(|| Ok(()))
            .await
            .map_err(|e| e.to_string())?;
        while let Ok(command) = self.actor.cmd_rx.try_recv() {
            if !matches!(&command, SessionCommand::ArchiveWriteCompleted { .. }) {
                return Err("归档排空遇到非预期命令".into());
            }
            self.actor.handle_command(command).await;
        }
        Ok(())
    }
    fn assert_durable_file(&self) -> Result<CheckpointKey, String> {
        let key = self
            .actor
            .archive_order
            .durable()
            .ok_or("实际收据未确认durable")?;
        // 只读重开磁盘文件，不申请第二个 Native writer lease。
        let reopened = native_store::rusqlite::Connection::open_with_flags(
            &self.file.0,
            native_store::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| e.to_string())?;
        let payload: String = reopened
            .query_row(
                "SELECT payload FROM archive_slots WHERE slot_id = ?1",
                ["trace"],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        let slot: SaveSlot = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
        ProtocolSession::restore(&slot).map_err(|e| e.to_string())?;
        let connected = self
            .database
            .load("trace")
            .map_err(|e| e.to_string())?
            .ok_or("活动SQLite连接未找到checkpoint")?;
        if connected.snapshot.seq != slot.snapshot.seq {
            return Err("重开SQLite与活动连接的durable事实不同".into());
        }
        if slot.snapshot.seq != key.seq
            || slot.civil_clock.settled_through != Some(key.settled_date)
        {
            return Err("SQLite实际日终与durable receipt不符".into());
        }
        Ok(key)
    }
    async fn receipt(&mut self, key: CheckpointKey) {
        self.actor
            .handle_command(SessionCommand::ArchiveWriteCompleted {
                key,
                result: Ok(()),
            })
            .await;
    }
    async fn restore(&mut self, slot: SaveSlot) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        self.actor
            .handle_command(SessionCommand::Restore {
                slot: Box::new(slot),
                reply,
            })
            .await;
        result
            .await
            .map_err(|e| e.to_string())?
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn attach(&mut self) -> Result<(), String> {
        let attachment = self
            .actor
            .attach_observer(self.subject.clone())
            .map_err(|e| e.to_string())?;
        match &mut self.observer {
            Some(observer) => observer.replace(attachment),
            None => self.observer = Some(crate::routes::AuthorityTraceObserver::attach(attachment)),
        }
        self.observer_snapshot = self.observer.as_ref().unwrap().snapshot();
        Ok(())
    }
    async fn fill_archive_lane(&mut self) -> Result<std::sync::mpsc::Sender<()>, String> {
        let (started, ready) = oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let lane = self
            .actor
            .archive_lane
            .as_ref()
            .ok_or("缺少真实ArchiveLane")?;
        let _blocking_job = lane
            .try_execute(move || {
                let _ = started.send(());
                blocked
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .map_err(|e| {
                        native_store::StoreError::Invalid(format!("验收归档栅栏未释放：{e}"))
                    })?;
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        ready.await.map_err(|e| e.to_string())?;
        for _ in 0..MAX_PENDING_CHECKPOINTS {
            let _pending = lane.try_execute(|| Ok(())).map_err(|e| e.to_string())?;
        }
        Ok(release)
    }
}

fn checkpoint_json(key: CheckpointKey) -> Value {
    json!({"generation":key.generation.to_string(),"writerEpoch":key.writer_epoch.to_string(),"seq":key.seq.to_string()})
}

impl ContractPort for ServerTracePort {
    async fn perform(&mut self, id: &str) -> Result<(), String> {
        match id {
            "initial" => {
                self.attach()?;
                if self.actor.pacing.is_running() {
                    return Err("初始会话应暂停".into());
                }
            }
            "manual" => self.step().await?,
            "without-observer" => {
                self.observer_snapshot = self
                    .observer
                    .as_ref()
                    .ok_or("未初始化真实publisher")?
                    .snapshot();
                self.observer = None;
                self.day().await?;
                self.drain_archive().await?;
                self.first_key = Some(self.assert_durable_file()?);
                if !self.actor.observers.is_empty() {
                    return Err("已断观察者未被实际producer移除".into());
                }
            }
            "slow-observer" => {
                self.attach()?;
                // 整个自然日不轮询publisher，不发任何应用或传输ACK。
                self.day().await?;
                self.drain_archive().await?;
                self.second_key = Some(self.assert_durable_file()?);
                if self.second_key.unwrap().seq
                    <= self.first_key.ok_or("缺少首日真实durable收据")?.seq
                {
                    return Err("真实归档未按日推进".into());
                }
                if self.observer.as_ref().unwrap().snapshot()["accepted"] != "0" {
                    return Err("慢观察者意外收到ACK".into());
                }
            }
            "archive-full" => {
                let release = self.fill_archive_lane().await?;
                let advanced = self.day().await;
                release.send(()).map_err(|e| e.to_string())?;
                advanced?;
                self.drain_archive().await?;
                if !self.actor.archive_order.is_blocked() {
                    return Err("真实队列满未阻塞authority".into());
                }
                let trace = self.actor.archive_order.trace();
                self.failed_key = trace.queued.last().copied();
                if self.failed_key.is_none() {
                    return Err("队列满丢弃sealed checkpoint".into());
                }
                self.replacement = Some(self.actor.game.save().map_err(|e| e.to_string())?);
                if self.assert_durable_file()? != self.second_key.unwrap() {
                    return Err("队列满冒充写入成功".into());
                }
            }
            "blocked" => {
                let before = (self.actor.game.tick(), self.actor.game.seq());
                let (reply, result) = oneshot::channel();
                self.actor
                    .handle_command(SessionCommand::SetRunning {
                        running: true,
                        reply,
                    })
                    .await;
                if result.await.map_err(|e| e.to_string())?.is_ok() || self.step().await.is_ok() {
                    return Err("归档blocked可被继续/单步绕过".into());
                }
                if self
                    .actor
                    .ingress
                    .read()
                    .map_err(|e| e.to_string())?
                    .1
                    .enqueue_player_intent(
                        AccountId(0),
                        Intent::Cancel {
                            code: engine::StockId(0),
                            id: engine::OrderId(1),
                        },
                    )
                    .is_ok()
                {
                    return Err("真实直通ingress绕过blocked".into());
                }
                if before != (self.actor.game.tick(), self.actor.game.seq()) {
                    return Err("blocked仍推进".into());
                }
            }
            "old-receipt" => {
                let before = self.actor.archive_order.durable();
                let mut key = self.failed_key.ok_or("缺少失败key")?;
                key.writer_epoch = 0;
                self.receipt(key).await;
                if self.actor.archive_order.durable() != before {
                    return Err("旧writer收据推进durable".into());
                }
            }
            "bad-receipt" => {
                let before = self.actor.archive_order.durable();
                let mut key = self.failed_key.ok_or("缺少失败key")?;
                key.seq += 1;
                self.receipt(key).await;
                if !self.actor.archive_order.is_blocked()
                    || self.actor.archive_order.durable() != before
                {
                    return Err("乱序收据被接受为durable".into());
                }
                if self.actor.pacing.latest_failure().map(|f| f.code)
                    != Some("DAY_END_ARCHIVE_FAILED")
                {
                    return Err("坏收据未显式报告".into());
                }
            }
            "duplicate-receipt" => {
                let before = self.actor.archive_order.durable();
                self.receipt(self.second_key.ok_or("缺少实际durable收据")?)
                    .await;
                if self.actor.archive_order.durable() != before {
                    return Err("重复收据推进durable".into());
                }
            }
            "prepare-rejected" => {
                let generation = self.actor.timeline_generation;
                let before = (self.actor.game.tick(), self.actor.game.seq());
                let mut invalid = self.replacement.clone().ok_or("缺少sealed checkpoint")?;
                invalid.setup.ticks_per_day = 0;
                if self.restore(invalid).await.is_ok() {
                    return Err("无效replacement未被prepare拒绝".into());
                }
                if self.actor.timeline_generation != generation
                    || before != (self.actor.game.tick(), self.actor.game.seq())
                {
                    return Err("prepare拒绝改变旧authority".into());
                }
            }
            "replaced" => {
                let generation = self.actor.timeline_generation;
                self.restore(self.replacement.clone().ok_or("缺少sealed checkpoint")?)
                    .await?;
                if self.actor.timeline_generation != generation + 1
                    || self.actor.archive_order.is_blocked()
                {
                    return Err("真实restore未安装新代或保留旧blocked".into());
                }
                self.attach()?;
            }
            "old-generation" => {
                self.receipt(self.failed_key.ok_or("缺少旧代key")?).await;
                if self.actor.archive_order.durable().is_some() {
                    return Err("旧代receipt污染新代durable".into());
                }
                if !self
                    .observer
                    .as_mut()
                    .ok_or("缺少publisher")?
                    .reject_old_generation()
                {
                    return Err("publisher接纳旧代包".into());
                }
                let (reply, result) = oneshot::channel();
                self.actor
                    .handle_command(SessionCommand::Step {
                        generation: 1,
                        reply,
                    })
                    .await;
                if result.await.map_err(|e| e.to_string())?.is_ok() {
                    return Err("actor接纳旧代命令".into());
                }
            }
            "new-lease" => self.attach()?,
            "old-lease" => {
                if !self
                    .observer
                    .as_mut()
                    .ok_or("缺少publisher")?
                    .reject_old_lease()
                {
                    return Err("新观察lease接纳旧ACK/release/cut".into());
                }
            }
            "prefix-failure" => {
                while self
                    .actor
                    .game
                    .civil_day_ready()
                    .map_err(|e| e.to_string())?
                {
                    self.civil().await?;
                    self.drain_archive().await?;
                }
                let fatal = engine::session::StepFatal::InvariantViolation {
                    location: "authority.contract".into(),
                    description: "实际actor在一个成功tick后注入失败".into(),
                };
                self.terminal = Some(HostFailure::step(&fatal));
                self.actor.injected_step_failure = Some((
                    1,
                    fatal,
                    Arc::new(std::sync::Mutex::new(StepFailureCapture::default())),
                ));
                self.actor.run_protocol_batch(3);
                if self.actor.pacing.fatal_failure().is_none() {
                    return Err("实际runner未锁存注入fatal".into());
                }
                let kinds = self.observer.as_mut().ok_or("缺少publisher")?.poll_kinds();
                let prefix = kinds
                    .iter()
                    .position(|kind| kind == "prefix")
                    .ok_or("真实publisher未读到成功prefix")?;
                let terminal = kinds
                    .iter()
                    .position(|kind| kind == "STEP_FATAL")
                    .ok_or("真实publisher未读到terminal")?;
                if prefix >= terminal {
                    return Err("terminal抢在成功prefix前面".into());
                }
            }
            "terminal-repeat" => {
                let before = (self.actor.game.tick(), self.actor.game.seq());
                self.actor
                    .stop_with_failure(self.terminal.clone().ok_or("缺少terminal")?);
                if self.step().await.is_ok() {
                    return Err("terminal后单步成功".into());
                }
                if before != (self.actor.game.tick(), self.actor.game.seq()) {
                    return Err("terminal后事实改变".into());
                }
                if self
                    .observer
                    .as_mut()
                    .ok_or("缺少publisher")?
                    .poll_kinds()
                    .iter()
                    .any(|kind| kind == "STEP_FATAL")
                {
                    return Err("terminal重复发布".into());
                }
            }
            "shutdown" => {
                let (reply, result) = oneshot::channel();
                self.actor
                    .handle_command(SessionCommand::Shutdown { reply })
                    .await;
                result.await.map_err(|e| e.to_string())??;
                self.actor.archive_order.ensure_drained()?;
                if self.actor.archive_order.durable().is_some() {
                    self.assert_durable_file()?;
                }
                if !self.commands.is_closed() {
                    return Err("shutdown未关闭控制通道".into());
                }
            }
            _ => return Err(format!("未知脚本步骤：{id}")),
        }
        Ok(())
    }
    async fn snapshot(&mut self, id: &str) -> Result<Value, String> {
        if let Some(observer) = &self.observer {
            self.observer_snapshot = observer.snapshot();
        }
        let archive = self.actor.archive_order.trace();
        // 每个durable游标都回读真实SQLite；不从内存候选反推落盘成功。
        if archive.durable.is_some() {
            self.assert_durable_file()?;
        }
        Ok(
            json!({"id":id,"generation":self.actor.timeline_generation.to_string(),"day":self.actor.game.day(),"tick":self.actor.game.tick().to_string(),"seq":self.actor.game.seq().to_string(),"paused":!self.actor.pacing.is_running(),"terminal":self.actor.pacing.fatal_failure().map(|f|f.code),"archive":{"queued":archive.queued.into_iter().map(checkpoint_json).collect::<Vec<_>>(),"durable":archive.durable.map(checkpoint_json),"blocked":archive.blocked},"observer":self.observer_snapshot}),
        )
    }
}
