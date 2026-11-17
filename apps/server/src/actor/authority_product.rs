//! 仅验证构建接入真实运行 actor；客户端观察与 ACK 必须从真实 WebSocket 取证。
use super::*;
use serde_json::{json, Value};

pub(crate) const ACTIONS: &[&str] = &[
    "complete-day",
    "postcommit-catalog-failure",
    "initial",
    "manual",
    "without-observer",
    "slow-observer",
    "archive-full",
    "blocked",
    "old-receipt",
    "bad-receipt",
    "duplicate-receipt",
    "prepare-rejected",
    "replaced",
    "old-generation",
    "new-lease",
    "old-lease",
    "prefix-failure",
    "terminal-repeat",
    "shutdown",
    "prepare-rejected-running-preserved",
    "shutdown-known-archive-failure-cleanup",
    "shutdown-pending",
    "restore-postcommit-io",
    "multi-observer-stalled",
    "disconnect-resync-stale",
    "terminal-queue-full-prefix-or-gap",
    "stale-release-same-generation",
];

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ContractCheckpoint {
    generation: String,
    writer_epoch: String,
    tick: String,
    seq: String,
    settled_date: String,
    day: u32,
}
impl ContractCheckpoint {
    fn key(self) -> Result<CheckpointKey, String> {
        fn number(value: &str) -> Result<u64, String> {
            let parsed = value.parse::<u64>().map_err(|error| error.to_string())?;
            if parsed.to_string() != value {
                return Err("非规范十进制 checkpoint 标识".into());
            }
            Ok(parsed)
        }
        let generation = number(&self.generation)?;
        let writer_epoch = number(&self.writer_epoch)?;
        if generation == 0 || writer_epoch == 0 {
            return Err("合同代际不得为零".into());
        }
        Ok(CheckpointKey {
            generation,
            writer_epoch,
            tick: number(&self.tick)?,
            seq: number(&self.seq)?,
            settled_date: engine::CivilDate::from_iso(&self.settled_date)
                .map_err(|error| error.to_string())?,
            day: self.day,
        })
    }
}
fn key_json(key: CheckpointKey) -> Value {
    json!({"generation":key.generation.to_string(),"writerEpoch":key.writer_epoch.to_string(),
        "tick":key.tick.to_string(),"seq":key.seq.to_string(),"settledDate":key.settled_date.to_iso(),"day":key.day})
}
impl SessionHandles {
    pub(crate) async fn authority_contract(
        &self,
        generation: u64,
        id: String,
        checkpoint: Option<ContractCheckpoint>,
        snapshot_only: bool,
    ) -> Result<Value, String> {
        let (reply, result) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::AuthorityContract {
                generation,
                id,
                checkpoint,
                snapshot_only,
                reply,
            })
            .map_err(|error| error.to_string())?;
        result.await.map_err(|error| error.to_string())?
    }
}
impl SessionActor {
    fn contract_snapshot(&self, id: &str) -> Value {
        let trace = self.archive_order.trace();
        json!({"id":id,"generation":self.timeline_generation.to_string(),"day":self.game.day(),
            "tick":self.game.tick().to_string(),"seq":self.game.seq().to_string(),
            "paused":!self.pacing.is_running(),"terminal":self.pacing.fatal_failure().map(|failure|failure.code),
            "archive":{"queued":trace.queued.into_iter().map(key_json).collect::<Vec<_>>(),"durable":trace.durable.map(key_json),"blocked":trace.blocked}})
    }
    async fn contract_step(&mut self) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        Box::pin(self.handle_command(SessionCommand::Step {
            generation: self.timeline_generation,
            reply,
        }))
        .await;
        result
            .await
            .map_err(|error| error.to_string())?
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    async fn contract_civil(&mut self) -> Result<(), String> {
        let (reply, result) = oneshot::channel();
        Box::pin(self.handle_command(SessionCommand::AdvanceCivilDay {
            generation: self.timeline_generation,
            reply,
        }))
        .await;
        result
            .await
            .map_err(|error| error.to_string())?
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    async fn contract_day(&mut self) -> Result<(), String> {
        for _ in 0..1000 {
            if self
                .game
                .civil_day_ready()
                .map_err(|error| error.to_string())?
            {
                return self.contract_civil().await;
            }
            self.contract_step().await?;
        }
        Err("验证短日超过 1000 拍".into())
    }
    async fn contract_drain(&mut self) -> Result<(), String> {
        self.archive_lane
            .as_ref()
            .ok_or("缺少真实归档 lane")?
            .execute(|| Ok(()))
            .await
            .map_err(|error| error.to_string())?;
        // 验证请求串行发送；意外业务请求不能丢弃，按实际邮箱顺序处理。
        while let Ok(command) = self.cmd_rx.try_recv() {
            Box::pin(self.handle_command(command)).await;
        }
        Ok(())
    }
    async fn contract_restore(&mut self, slot: SaveSlot) -> Result<RestoreCommitted, String> {
        let (reply, result) = oneshot::channel();
        Box::pin(self.handle_command(SessionCommand::Restore {
            slot: Box::new(slot),
            reply,
        }))
        .await;
        result
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
    }
    pub(super) async fn contract_action(
        &mut self,
        generation: u64,
        id: &str,
        checkpoint: Option<ContractCheckpoint>,
        snapshot_only: bool,
    ) -> Result<Value, String> {
        if generation != self.timeline_generation {
            return Err("验证请求 generation 已过期".into());
        }
        if !ACTIONS.contains(&id) {
            return Err(format!("验证动作尚未接线：{id}"));
        }
        if snapshot_only {
            return Ok(self.contract_snapshot(id));
        }
        let mut assertions = serde_json::Map::new();
        let mut shutdown_failure = None;
        let mut replacement_evidence = None;
        match id {
            "initial" => {
                if self.pacing.is_running() {
                    return Err("验证必须先通过产品控制暂停".into());
                }
            }
            "manual" => self.contract_step().await?,
            "complete-day"
            | "without-observer"
            | "slow-observer"
            | "multi-observer-stalled"
            | "disconnect-resync-stale" => {
                self.contract_day().await?;
                self.contract_drain().await?;
            }
            "archive-full" => {
                let lane = self.archive_lane.as_ref().ok_or("缺少真实归档 lane")?;
                let (started, ready) = oneshot::channel();
                let (release, blocked) = std::sync::mpsc::channel();
                let job = lane
                    .try_execute(move || {
                        let _ = started.send(());
                        blocked
                            .recv_timeout(std::time::Duration::from_secs(2))
                            .map_err(|error| {
                                native_store::StoreError::Invalid(error.to_string())
                            })?;
                        Ok(())
                    })
                    .map_err(|error| error.to_string())?;
                ready.await.map_err(|error| error.to_string())?;
                let mut jobs = Vec::new();
                for _ in 0..engine::session::checkpoint::MAX_PENDING_CHECKPOINTS {
                    jobs.push(
                        lane.try_execute(|| Ok(()))
                            .map_err(|error| error.to_string())?,
                    );
                }
                let advanced = self.contract_day().await;
                release.send(()).map_err(|error| error.to_string())?;
                job.await
                    .map_err(|error| error.to_string())?
                    .map_err(|error| error.to_string())?;
                for job in jobs {
                    job.await
                        .map_err(|error| error.to_string())?
                        .map_err(|error| error.to_string())?;
                }
                advanced?;
                self.contract_drain().await?;
                if !self.archive_order.is_blocked() || self.archive_order.trace().queued.is_empty()
                {
                    return Err("队列满未保留 blocked sealed checkpoint".into());
                }
            }
            "blocked" => {
                let before = (self.game.tick(), self.game.seq());
                if self.contract_step().await.is_ok()
                    || before != (self.game.tick(), self.game.seq())
                {
                    return Err("blocked 仍可推进".into());
                }
            }
            "old-receipt" | "bad-receipt" | "duplicate-receipt" | "old-generation" => {
                let mut key = checkpoint
                    .ok_or("必须传入此前真实采样的 checkpoint")?
                    .key()?;
                if id == "old-receipt" {
                    key.writer_epoch = 0;
                }
                if id == "bad-receipt" {
                    key.seq = key.seq.checked_add(1).ok_or("seq 耗尽")?;
                }
                let before = self.archive_order.durable();
                self.finish_archive_write(key, Ok(()));
                if before != self.archive_order.durable() {
                    return Err("拒绝/重复收据改变 durable".into());
                }
                if id == "bad-receipt" && !self.archive_order.is_blocked() {
                    return Err("坏收据未 blocked".into());
                }
            }
            "prepare-rejected" | "prepare-rejected-running-preserved" => {
                if id == "prepare-rejected-running-preserved" {
                    self.contract_day().await?;
                    self.contract_drain().await?;
                }
                let mut slot = self.game.save().map_err(|error| error.to_string())?;
                if id == "prepare-rejected-running-preserved" {
                    self.pacing
                        .control_running(
                            true,
                            self.game.tick(),
                            native_async::now(),
                            self.archive_order.blocked_reason(),
                        )
                        .map_err(|error| error.to_string())?;
                }
                let before = (
                    self.timeline_generation,
                    self.game.tick(),
                    self.game.seq(),
                    self.pacing.is_running(),
                );
                slot.setup.ticks_per_day = 0;
                if self.contract_restore(slot).await.is_ok()
                    || before
                        != (
                            self.timeline_generation,
                            self.game.tick(),
                            self.game.seq(),
                            self.pacing.is_running(),
                        )
                {
                    return Err("prepare 拒绝改变旧 owner 意图/事实".into());
                }
                if id == "prepare-rejected-running-preserved" {
                    assertions.insert(
                        "runningPreserved".into(),
                        json!(before.3 && self.pacing.is_running()),
                    );
                }
            }
            "postcommit-catalog-failure" => {
                let committed_generation = self.timeline_generation;
                let error = engine::session::CompanyCatalogViewError::UnknownListedStock {
                    company: engine::company::CompanyId(7),
                };
                if self.finish_public_baseline(Err(error)).is_ok()
                    || self.timeline_generation != committed_generation
                    || self.pacing.fatal_failure().is_none()
                {
                    return Err("独立 catalog 故障未保留已提交代际".into());
                }
                assertions.insert("separateCatalogFailureVisible".into(), json!(true));
            }
            "restore-postcommit-io" => {
                self.contract_day().await?;
                self.contract_drain().await?;
                let slot = self.game.save().map_err(|error| error.to_string())?;
                let result = self.contract_restore(slot).await?;
                let committed_generation = self.timeline_generation;
                // Server 主 restore 已返回回执；模拟的是随后独立 catalog 资源失败，非主响应假回滚。
                let error = engine::session::CompanyCatalogViewError::UnknownListedStock {
                    company: engine::company::CompanyId(7),
                };
                if self.finish_public_baseline(Err(error)).is_ok()
                    || self.timeline_generation != committed_generation
                    || self.pacing.fatal_failure().is_none()
                {
                    return Err("commit 后独立 catalog 失败没有保留新代/显式失败".into());
                }
                replacement_evidence = Some(
                    json!({"phase":"separate-catalog","replacement":result.replacement,"failure":self.pacing.fatal_failure()}),
                );
                assertions.insert("committedGenerationRetained".into(), json!(true));
                assertions.insert("separateCatalogFailureVisible".into(), json!(true));
            }
            "replaced" => {
                let slot = self.game.save().map_err(|error| error.to_string())?;
                self.contract_restore(slot).await?;
            }
            // 这些动作只采样；观察行为必须由真实客户端实施并用 WS InspectObserver 取证。
            "new-lease" | "old-lease" | "stale-release-same-generation" => {}
            "prefix-failure" | "terminal-queue-full-prefix-or-gap" => {
                if id == "terminal-queue-full-prefix-or-gap" {
                    if self
                        .observers
                        .iter()
                        .all(|observer| observer.producer.is_observer_closed())
                    {
                        return Err("必须先实际挂接并 hold 观察流".into());
                    }
                    // 保持产品真实容量；通过真实市场命令产生超过容量的更新，不合成帧。
                    for _ in 0..=publication::OBSERVER_PACKET_LIMIT.get() {
                        if self
                            .game
                            .civil_day_ready()
                            .map_err(|error| error.to_string())?
                        {
                            self.contract_civil().await?;
                            self.contract_drain().await?;
                        } else {
                            self.contract_step().await?;
                        }
                    }
                    // 是否形成缺口仍由真实 WS terminal.missing 取证，不能在 actor 伪造。
                    assertions.insert("actualUpdatesExceedQueueCapacity".into(), json!(true));
                }
                for _ in 0..7 {
                    if !self
                        .game
                        .civil_day_ready()
                        .map_err(|error| error.to_string())?
                    {
                        break;
                    }
                    self.contract_civil().await?;
                    self.contract_drain().await?;
                }
                self.injected_step_failure = Some((
                    1,
                    engine::session::StepFatal::InvariantViolation {
                        location: "authority.product.contract".into(),
                        description: "验证构建：一个成功 tick 后失败".into(),
                    },
                    Arc::new(std::sync::Mutex::new(StepFailureCapture::default())),
                ));
                self.run_protocol_batch(3);
                if self.pacing.fatal_failure().is_none() {
                    return Err("真实 runner 未产生 terminal".into());
                }
            }
            "terminal-repeat" => {
                let failure = self
                    .pacing
                    .fatal_failure()
                    .cloned()
                    .ok_or("缺少已锁存 terminal")?;
                self.stop_with_failure(failure);
                if self.contract_step().await.is_ok() {
                    return Err("terminal 后仍推进".into());
                }
            }
            "shutdown-known-archive-failure-cleanup" => {
                Box::pin(self.contract_action(generation, "archive-full", None, false)).await?;
                let queued = self.archive_order.trace().queued;
                let result = self.shutdown_authority().await;
                if result.is_ok() || !self.cmd_rx.is_closed() || queued.is_empty() {
                    return Err("失败归档关闭未保留可见错误/关闭事实".into());
                }
                if queued
                    .iter()
                    .any(|key| Some(*key) == self.archive_order.durable())
                {
                    return Err("未确认候选冒充 durable".into());
                }
                shutdown_failure = result.err();
                assertions.insert("archiveFailureVisible".into(), json!(true));
                assertions.insert(
                    "ownerClosed".into(),
                    json!(self.cmd_rx.is_closed() && !self.pacing.is_running()),
                );
                assertions.insert("queuedNotDurable".into(), json!(true));
            }
            "shutdown-pending" => {
                self.contract_day().await?;
                // lane 尚未回 actor 确认，此时必须是真实 queued；shutdown 自己排空收据。
                if self.archive_order.pending_count() == 0 {
                    return Err("未建立在途归档，不能声称覆盖 pending shutdown".into());
                }
                self.shutdown_authority().await?;
                if self.archive_order.pending_count() != 0 || self.archive_order.durable().is_none()
                {
                    return Err("shutdown 未确认真实耐久收据".into());
                }
                assertions.insert("pendingDrainedToDurable".into(), json!(true));
            }
            "shutdown" => self.shutdown_authority().await?,
            _ => return Err(format!("验证动作尚未实现：{id}")),
        }
        let mut row = self.contract_snapshot(id);
        if !assertions.is_empty() {
            row["assertions"] = Value::Object(assertions);
        }
        if let Some(evidence) = replacement_evidence {
            row["replacementEvidence"] = evidence;
        }
        if let Some(failure) = shutdown_failure {
            row["shutdownFailure"] = json!(failure);
        }
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_input_preserves_exact_identity_and_rejects_noncanonical_values() {
        let raw = json!({"generation":"1","writerEpoch":"1","tick":"4","seq":"9","day":1,"settledDate":"2030-01-02"});
        let key: ContractCheckpoint = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(key.key().unwrap().seq, 9);
        for (field, value) in [
            ("generation", "0"),
            ("writerEpoch", "01"),
            ("seq", "-1"),
            ("settledDate", "2030-02-31"),
        ] {
            let mut invalid = raw.clone();
            invalid[field] = json!(value);
            let key: ContractCheckpoint = serde_json::from_value(invalid).unwrap();
            assert!(key.key().is_err());
        }
    }
}
