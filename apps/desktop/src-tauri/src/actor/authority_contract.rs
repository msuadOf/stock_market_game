//! 仅 host-parity 产品验证入口；使用实际 actor、磁盘归档 lane 和窗口 delivery。
use super::*;
use serde_json::{json, Value};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_input_requires_canonical_cursor_and_real_civil_date() {
        let valid = json!({"generation":"1","writerEpoch":"2","tick":"4","seq":"9","day":1,"settledDate":"2030-01-02"});
        assert_eq!(parse_checkpoint(valid.clone()).unwrap().seq, 9);
        let mut invalid = valid.clone();
        invalid["seq"] = json!("09");
        assert!(parse_checkpoint(invalid).is_err());
        let mut invalid = valid;
        invalid["settledDate"] = json!("2030-02-31");
        assert!(parse_checkpoint(invalid).is_err());
    }
    #[test]
    fn unsupported_injection_is_not_declared_as_covered() {
        assert!(require_action("archive-full").is_ok());
        assert!(require_action("multi-observer-stalled").is_err());
        assert_eq!(
            capabilities()["phaseMapping"]["restore-postcommit-io"],
            "baseline"
        );
    }
}

pub(super) const EXTENSIONS: &[&str] = &[
    "prepare-rejected-running-preserved",
    "shutdown-known-archive-failure-cleanup",
    "terminal-queue-full-prefix-or-gap",
    "stale-release-same-generation",
    "restore-postcommit-io",
    "shutdown-pending",
    "disconnect-resync-stale",
];
pub(super) fn capabilities() -> Value {
    let actions = engine::session::authority_contract::STEPS
        .iter()
        .copied()
        .chain(EXTENSIONS.iter().copied())
        .collect::<Vec<_>>();
    json!({"setup":engine::session::authority_contract::setup(),"seed":"7","verification":true,"scenarioVersion":1,"supportedActions":actions,
      "phaseMapping":{"restore-postcommit-io":"baseline"},"verificationControls":["arm-restore-postcommit-baseline-failure"],
      "notApplicable":{"shutdown-unknown-worker-exit-no-reuse":{"mechanism":"wasm-shared-memory-worker","reason":"Native actor没有共享WASM Memory的Worker退出复用机制"},"multi-observer-stalled":{"mechanism":"independent-observer-credit","reason":"Desktop实际只有一条有信用的IPC delivery；多个窗口listener不是独立lease观察者"}}})
}

fn require_action(id: &str) -> Result<(), String> {
    if engine::session::authority_contract::STEPS.contains(&id)
        || EXTENSIONS.contains(&id)
        || id == "arm-restore-postcommit-baseline-failure"
    {
        Ok(())
    } else {
        Err(format!("未实现的 Desktop 产品合同动作：{id}"))
    }
}
fn decimal(value: &str) -> Result<u64, String> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("合同游标必须为规范十进制整数串".into());
    }
    value.parse().map_err(|e| format!("合同游标溢出：{e}"))
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckpointInput {
    generation: String,
    writer_epoch: String,
    tick: String,
    seq: String,
    day: u32,
    settled_date: String,
}
fn parse_checkpoint(value: Value) -> Result<CheckpointKey, String> {
    let input: CheckpointInput = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let generation = decimal(&input.generation)?;
    let writer_epoch = decimal(&input.writer_epoch)?;
    if generation == 0 || writer_epoch == 0 {
        return Err("合同代际不得为零".into());
    }
    Ok(CheckpointKey {
        generation,
        writer_epoch,
        tick: decimal(&input.tick)?,
        seq: decimal(&input.seq)?,
        day: input.day,
        settled_date: engine::CivilDate::from_iso(&input.settled_date)
            .map_err(|e| e.to_string())?,
    })
}
fn key_json(key: CheckpointKey) -> Value {
    json!({"generation":key.generation.to_string(),"writerEpoch":key.writer_epoch.to_string(),"tick":key.tick.to_string(),"seq":key.seq.to_string(),"day":key.day,"settledDate":key.settled_date.to_iso()})
}
impl<R: Runtime> SessionActor<R> {
    fn contract_tick(&mut self) -> Result<(), String> {
        let outcome = authority::manual_tick(self);
        if let Some(failure) = outcome.failure {
            self.stop_after_host_failure(failure);
        }
        outcome.result.map(|_| ()).map_err(|e| e.to_string())
    }
    fn contract_civil(&mut self) -> Result<(), String> {
        let pause = self.pause_preferences;
        let outcome = authority::manual_civil(self, pause);
        if let Some(failure) = outcome.failure {
            self.stop_after_host_failure(failure);
        }
        outcome.result.map(|_| ()).map_err(|e| e.to_string())
    }
    fn contract_reach_boundary(&mut self) -> Result<(), String> {
        for _ in 0..1_000 {
            if self.game.civil_day_ready().map_err(|e| e.to_string())? {
                return Ok(());
            }
            self.contract_tick()?;
        }
        Err("合同短场景未在1000拍内到达日界".into())
    }
    async fn contract_drain_archive(&mut self) -> Result<(), String> {
        self.archive_lane
            .as_ref()
            .ok_or("合同缺少真实归档lane")?
            .execute(|| Ok(()))
            .await
            .map_err(|e| e.to_string())?;
        while let Ok(command) = self.cmd_rx.try_recv() {
            // 邮箱中先到的其他命令仍经真实入口处理，不能为了验证悄悄丢弃。
            Box::pin(self.handle_command(command)).await;
        }
        Ok(())
    }
    fn contract_verify_disk(&self) -> Result<(), String> {
        if let Some(key) = self.archive_order.durable() {
            let (_, database) = self.archive.as_ref().ok_or("合同缺少实际SQLite")?;
            let slot = database
                .load("current")
                .map_err(|e| e.to_string())?
                .ok_or("实际磁盘快槽为空")?;
            if slot.snapshot.seq != key.seq
                || slot.civil_clock.settled_through != Some(key.settled_date)
            {
                return Err("实际磁盘日终与已接纳durable收据不符".into());
            }
        }
        Ok(())
    }
    pub(super) async fn contract_snapshot(
        &mut self,
        generation: u64,
        id: &str,
    ) -> Result<Value, String> {
        self.generation_response(generation, Ok(()))
            .map_err(|e| e.to_string())?;
        require_action(id)?;
        self.contract_verify_disk()?;
        let archive = self.archive_order.trace();
        let observer = self.delivery.inspect_observer().await?;
        let lease = observer.lease.ok_or("实际delivery缺少观察lease")?;
        Ok(
            json!({"id":id,"generation":self.generation.to_string(),"timelineId":self.timeline_id,"day":self.game.day(),"tick":self.game.tick().to_string(),"seq":self.game.seq().to_string(),"paused":!self.pacing.is_running(),"lifecycle":{"commandClosed":self.cmd_rx.is_closed(),"archiveLaneReleased":self.archive_lane.is_none()},"terminal":self.pacing.fatal_failure().map(|f|f.code),"archive":{"queued":archive.queued.into_iter().map(key_json).collect::<Vec<_>>(),"durable":archive.durable.map(key_json),"blocked":archive.blocked},"observer":{"generation":lease.generation.to_string(),"lease":lease.lease.to_string(),"sent":observer.delivery.sent.to_string(),"flushed":observer.delivery.flushed.to_string(),"accepted":observer.delivery.accepted.to_string(),"held":observer.held}}),
        )
    }
    pub(super) async fn contract_step(
        &mut self,
        generation: u64,
        id: &str,
        checkpoint: Option<Value>,
    ) -> Result<Value, String> {
        self.generation_response(generation, Ok(()))
            .map_err(|e| e.to_string())?;
        require_action(id)?;
        let mut extra = Value::Null;
        match id {
            "arm-restore-postcommit-baseline-failure" => {
                self.contract_catalog_failure = true;
            }
            "prepare-rejected-running-preserved" => {
                self.contract_reach_boundary()?;
                self.contract_civil()?;
                self.contract_drain_archive().await?;
                self.pacing
                    .control_running(
                        true,
                        self.game.tick(),
                        native_async::now(),
                        self.archive_order.blocked_reason(),
                    )
                    .map_err(|e| e.to_string())?;
                let before = (
                    self.generation,
                    self.game.tick(),
                    self.game.seq(),
                    self.pacing.is_running(),
                );
                let mut invalid = self.game.save().map_err(|e| e.to_string())?;
                invalid.setup.ticks_per_day = 0;
                if self
                    .restore(self.generation, Box::new(invalid))
                    .await
                    .is_ok()
                    || before
                        != (
                            self.generation,
                            self.game.tick(),
                            self.game.seq(),
                            self.pacing.is_running(),
                        )
                {
                    return Err("prepare拒绝改变健康owner运行意图".into());
                }
                extra = json!({"phase":"prepare","runningBefore":before.3,"runningAfter":self.pacing.is_running()});
            }
            "shutdown-known-archive-failure-cleanup" => {
                Box::pin(self.contract_step(self.generation, "archive-full", None)).await?;
                let error = self
                    .shutdown_complete()
                    .await
                    .err()
                    .ok_or("blocked关闭不能伪报durable成功")?;
                if !self.cmd_rx.is_closed() || self.archive_lane.is_some() {
                    return Err("已知归档失败关闭未清理owner".into());
                }
                let ipc_message =
                    crate::map_send_error(super::SendCommandError::Rejected(error.clone()));
                extra = json!({"shutdown":{"kind":"known-archive-failure","message":error,"ipcMessage":ipc_message,"commandClosed":self.cmd_rx.is_closed(),"laneReleased":self.archive_lane.is_none()}});
            }
            "shutdown-pending" => {
                self.contract_reach_boundary()?;
                self.contract_civil()?;
                let pending = self.archive_order.trace().queued;
                if pending.is_empty() {
                    return Err("关闭夹具没有实际待接纳归档".into());
                }
                self.shutdown_complete().await?;
                extra = json!({"pendingBefore":pending.into_iter().map(key_json).collect::<Vec<_>>(),"commandClosed":self.cmd_rx.is_closed()});
            }
            "restore-postcommit-io" => {
                self.contract_reach_boundary()?;
                self.contract_civil()?;
                self.contract_drain_archive().await?;
                let slot = self.game.save().map_err(|e| e.to_string())?;
                let previous = self.generation;
                self.contract_catalog_failure = true;
                let restored = self.restore(previous, Box::new(slot)).await;
                self.contract_catalog_failure = false;
                match restored {
                    Err(BaselineError::Committed {
                        failure,
                        replacement,
                        timeline_id,
                    }) => {
                        if self.generation != previous + 1 {
                            return Err("已提交baseline失败错误回滚代际".into());
                        }
                        extra = json!({"phase":"baseline","failure":failure,"replacement":replacement,"timelineId":timeline_id});
                    }
                    _ => return Err("没有触发实际commit之后baseline失败路径".into()),
                }
            }
            "terminal-queue-full-prefix-or-gap" => {
                let observation = self.delivery.inspect_observer().await?;
                if !observation.held {
                    return Err("窗口必须先通过实际IPC hold观察lease".into());
                }
                for _ in 0..=engine::session::publication::OBSERVER_PACKET_LIMIT.get() {
                    while self.game.civil_day_ready().map_err(|e| e.to_string())? {
                        self.contract_civil()?;
                        self.contract_drain_archive().await?;
                    }
                    self.contract_tick()?;
                }
                self.stop_after_host_failure(HostFailure::step(
                    engine::session::StepFatal::InvariantViolation {
                        location: "authority.contract.full".into(),
                        description: "真实held观察队列填满后注入terminal".into(),
                    },
                ));
                extra = json!({"heldBefore":observation.held,"actualPublications":engine::session::publication::OBSERVER_PACKET_LIMIT.get()+1});
            }
            // 这两项由真实窗口执行断线/重挂及旧lease IPC，actor只提供真实状态采样。
            "stale-release-same-generation" | "disconnect-resync-stale" => {}
            "initial" => {
                if self.game.tick() != 0 || self.pacing.is_running() || self.archive.is_none() {
                    return Err("合同须从暂停且有实际磁盘归档的新会话开始".into());
                }
            }
            "manual" => self.contract_tick()?,
            "without-observer" | "slow-observer" => {
                self.contract_reach_boundary()?;
                self.contract_civil()?;
                self.contract_drain_archive().await?;
                if self.archive_order.durable().is_none() {
                    return Err("日终尚无实际存储收据".into());
                }
            }
            "archive-full" => {
                self.contract_reach_boundary()?;
                let (entered, ready) = oneshot::channel();
                let (release, blocked) = std::sync::mpsc::channel();
                let lane = self.archive_lane.as_ref().ok_or("合同缺少归档lane")?;
                let waiting = lane
                    .try_execute(move || {
                        let _ = entered.send(());
                        blocked
                            .recv_timeout(std::time::Duration::from_secs(2))
                            .map_err(|e| StoreError::Invalid(e.to_string()))?;
                        Ok(())
                    })
                    .map_err(|e| e.to_string())?;
                ready.await.map_err(|e| e.to_string())?;
                for _ in 0..engine::session::checkpoint::MAX_PENDING_CHECKPOINTS {
                    let _pending = lane.try_execute(|| Ok(())).map_err(|e| e.to_string())?;
                }
                let committed = self.contract_civil();
                release.send(()).map_err(|e| e.to_string())?;
                waiting
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())?;
                committed?;
                self.contract_drain_archive().await?;
                if !self.archive_order.is_blocked() || self.archive_order.trace().queued.is_empty()
                {
                    return Err("真实队列满未保留sealed归档并阻塞".into());
                }
            }
            "blocked" => {
                let before = (self.game.tick(), self.game.seq());
                let reason = self.archive_order.blocked_reason();
                if self
                    .pacing
                    .control_running(true, self.game.tick(), native_async::now(), reason)
                    .is_ok()
                    || self.contract_tick().is_ok()
                    || before != (self.game.tick(), self.game.seq())
                {
                    return Err("blocked被控制命令绕过".into());
                }
            }
            "old-receipt" | "bad-receipt" | "duplicate-receipt" | "old-generation" => {
                let mut key =
                    parse_checkpoint(checkpoint.ok_or("收据动作缺少实际历史checkpoint")?)?;
                if id == "old-receipt" {
                    key.writer_epoch = key.writer_epoch.saturating_sub(1);
                }
                if id == "bad-receipt" {
                    key.seq = key.seq.checked_add(1).ok_or("合同seq溢出")?;
                }
                let before = self.archive_order.durable();
                self.finish_archive_write(key, Ok(()));
                if before != self.archive_order.durable() {
                    return Err("陈旧/异常收据污染真实durable".into());
                }
            }
            "prepare-rejected" => {
                let before = (
                    self.generation,
                    self.game.tick(),
                    self.game.seq(),
                    self.pacing.is_running(),
                );
                let mut slot = self.game.save().map_err(|e| e.to_string())?;
                slot.setup.ticks_per_day = 0;
                if self.restore(self.generation, Box::new(slot)).await.is_ok()
                    || before
                        != (
                            self.generation,
                            self.game.tick(),
                            self.game.seq(),
                            self.pacing.is_running(),
                        )
                {
                    return Err("prepare拒绝改变旧owner".into());
                }
            }
            "replaced" => {
                let slot = self.game.save().map_err(|e| e.to_string())?;
                self.restore(self.generation, Box::new(slot))
                    .await
                    .map_err(|e| e.to_string())?;
            }
            // 真实窗口负责listener/hold/ACK/cut；此处只读取实际delivery结果，不模拟它。
            "new-lease" | "old-lease" => {}
            "prefix-failure" => {
                while self.game.civil_day_ready().map_err(|e| e.to_string())? {
                    self.contract_civil()?;
                }
                self.contract_drain_archive().await?;
                self.contract_tick()?;
                self.stop_after_host_failure(HostFailure::step(
                    engine::session::StepFatal::InvariantViolation {
                        location: "authority.contract".into(),
                        description: "验证注入：已提交前缀之后失败".into(),
                    },
                ));
            }
            "terminal-repeat" => {
                let failure = self
                    .pacing
                    .fatal_failure()
                    .cloned()
                    .ok_or("没有已锁存terminal")?;
                let before = (self.game.tick(), self.game.seq());
                self.stop_after_host_failure(failure);
                if self.contract_tick().is_ok() || before != (self.game.tick(), self.game.seq()) {
                    return Err("terminal后仍可推进".into());
                }
            }
            "shutdown" => {
                self.shutdown_complete().await?;
            }
            _ => unreachable!("动作已经由白名单验证"),
        }
        // shutdown也先捕获真实终态返回；驱动不得在邮箱关闭后再次发snapshot。
        let mut row = self.contract_snapshot(self.generation, id).await?;
        if !extra.is_null() {
            row["evidence"] = extra;
        }
        Ok(row)
    }
}
