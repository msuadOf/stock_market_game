//! 活动 World 显式停止结果；这里只记录 owned worker，不代表宿主 child exit。
use super::{ShutdownReport, World};
use crate::runtime::protocol::{RuntimeError, UpMsg, WorkerId};

/// 对外安全的稳定诊断字段；不携带 RuntimeError 或业务资产载荷。
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShutdownDiagnostic {
    pub kind: String,
    pub location: String,
    pub message: String,
}

/// 显式停止诊断；worker 使用 WorkerId::Display 的 group-N / desk-N 身份。
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShutdownIssue {
    pub stage: String,
    pub worker: Option<String>,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<ShutdownDiagnostic>,
    #[serde(skip)]
    pub(crate) structured_error: Option<RuntimeError>,
}

impl ShutdownIssue {
    /// 为宿主层创建不含 engine 内部错误载荷的安全诊断。
    pub fn new(stage: impl Into<String>, worker: Option<String>, detail: impl Into<String>) -> Self {
        Self { stage: stage.into(), worker, detail: detail.into(), diagnostic: None, structured_error: None }
    }
}

/// 一次真实 owned handle join 的结果，保留 worker 原错误或 join 错误详情。
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShutdownWorkerResult {
    pub worker: String,
    pub result: Result<(), String>,
    /// join 是否实际移交了 worker 自有 receiver。
    pub receiver_recovered: bool,
    /// receiver 的准入已收束且其队列已完整检查；不表示消息已处理。
    pub messages_drained: bool,
    /// 不从 Final 数、join 数或空队列推断业务资产已对账。
    pub assets_reconciled: bool,
    /// join 返回是否证明 worker/host child 已退出。
    pub child_released: bool,
    /// 保留 Rust 侧结构化错误及其载荷；当前 wire 不承诺传输 Final。
    #[serde(skip)]
    pub(crate) structured_error: Option<RuntimeError>,
}

impl ShutdownWorkerResult {
    /// 下行清理未完成时保留的真实 worker exit/消息载荷；取出前不会丢弃。
    pub fn retained_worker_payload(&self) -> Option<&super::protocol::RetainedWorkerPayload> {
        match &self.structured_error {
            Some(RuntimeError::DownlinkCleanupFailed { payload, .. }) => Some(payload),
            Some(RuntimeError::WorkerJoinPending { payload, .. }) => Some(payload),
            _ => None,
        }
    }

    /// 未处理下行消息的逐类诊断，不取走载荷且不声称完成资产对账。
    pub fn retained_message_assessment(&self) -> Option<super::RetainedMessageAssessment> {
        self.retained_worker_payload()?
            .inspect::<super::RetainedExitMessages, _>(super::RetainedExitMessages::assessment)
    }

    /// 对 timeout 或 collector 启动失败的 join 继续使用原句柄；再次未完成时，返回的错误仍携带新保留句柄。
    pub fn retry_pending_join(
        &self,
        deadline: crate::clock::Instant,
    ) -> Option<ShutdownWorkerResult> {
        let payload = self.retained_worker_payload()?;
        let retained = payload.take::<super::RetainedWorldJoin>()?;
        let who = payload.worker();
        let result = match super::join_world_worker(who, retained.handle, deadline, retained.context.clone()) {
            Ok((actual, exit)) => {
                debug_assert_eq!(actual, who);
                super::settle_worker_exit(exit, deadline, retained.context).1
            }
            Err(error) => Err(error),
        };
        let (mut workers, _) = completion_worker_results(vec![(who, result)]);
        Some(workers.remove(0))
    }

    /// 仅取得未送达 TaskDied 的原始 panic；成功时载荷只可取出一次。
    pub fn take_undelivered_panic(&self) -> Option<Box<dyn std::any::Any + Send>> {
        self.structured_error.as_ref()?.retained_panic_payload()?.take()
    }

    /// 未送达的 GroupFinal 仅供诊断，不代表资产已对账或允许重放；wire 不传输此载荷。
    pub fn retained_group_final(&self) -> Option<&crate::runtime::group::GroupFinalState> {
        match &self.structured_error {
            Some(RuntimeError::GroupFinalUndelivered { final_state, .. }) => Some(final_state.as_ref()),
            _ => None,
        }
    }

    /// 未送达的 DeskFinal 仅供诊断，不代表资产已对账或允许重放；wire 不传输此载荷。
    pub fn retained_desk_final(&self) -> Option<&crate::runtime::desk::DeskFinalState> {
        match &self.structured_error {
            Some(RuntimeError::DeskFinalUndelivered { final_state, .. }) => Some(final_state.as_ref()),
            _ => None,
        }
    }
}

/// 已观察到的 World 收尾事实；Final 仅为核验后的计数，不传输期末账户树。
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShutdownCompletion {
    pub owned_workers: usize,
    pub joined_workers: usize,
    pub receiver_recovered: bool,
    pub messages_drained: bool,
    /// 当前 shutdown 协议不执行独立业务资产对账，固定为 false。
    pub assets_reconciled: bool,
    pub child_released: bool,
    pub worker_results: Vec<ShutdownWorkerResult>,
    pub group_finals: usize,
    pub desk_finals: usize,
    /// 仅测试见证：从真实 GroupFinal.observed 提炼，不进入生产 FFI。
    #[cfg(test)]
    #[serde(skip)]
    pub(crate) observed_group_shutdowns: Vec<String>,
}

/// 失败不能给成功 ack；completion 只在实际取得收尾事实后存在。
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShutdownFailure {
    pub primary: ShutdownIssue,
    pub secondary: Vec<ShutdownIssue>,
    pub completion: Option<Box<ShutdownCompletion>>,
}

impl World {
    /// 正常活动 World 先完成原消息协议；失败仍消费所有 owned handle。
    pub fn shutdown_with_completion(mut self) -> Result<ShutdownCompletion, ShutdownFailure> {
        let owned = self.n_groups + self.n_stocks;
        if let Some(reason) = self.dead_reason.clone() {
            let (completion, secondary) = self.stop_failed_with_completion();
            return Err(ShutdownFailure {
                primary: ShutdownIssue {
                    stage: "world_terminated".to_owned(),
                    worker: None,
                    detail: reason,
                    diagnostic: None,
                    structured_error: None,
                },
                secondary,
                completion: Some(Box::new(completion)),
            });
        }
        let mut stage = "shutdown_state";
        match self.shutdown_message_protocol(&mut stage) {
            Ok(mut report) => {
                report.worker_results.extend(self.join_worker_results());
                complete_shutdown_report(owned, report)
            }
            Err(error) => {
                let primary = runtime_issue(stage, error);
                let (completion, secondary) = self.stop_failed_with_completion();
                Err(ShutdownFailure {
                    primary,
                    secondary,
                    completion: Some(Box::new(completion)),
                })
            }
        }
    }

    /// 已毒化 owner 只作失败销毁；不执行健康 Drain，也不安装 Final。
    pub(crate) fn stop_failed_with_completion(
        mut self,
    ) -> (ShutdownCompletion, Vec<ShutdownIssue>) {
        let owned = self.n_groups + self.n_stocks;
        // 既有失败路径可能已真实 join 并保存报告；不能丢失这些已取得的事实。
        let mut results = match self.dead_finals.take() {
            Some(report) => report.worker_results,
            None => Vec::new(),
        };
        results.extend(std::mem::take(&mut self.dead_worker_results));
        self.sever();
        results.extend(self.join_worker_results());
        let mut issues = Vec::new();
        for message in self.up_rx.try_iter() {
            match message {
                UpMsg::Failed { who, error } => issues.push(ShutdownIssue {
                    stage: "shutdown_upstream".to_owned(),
                    worker: Some(who.to_string()),
                    detail: error.to_string(),
                    diagnostic: None,
                    structured_error: Some(error),
                }),
                UpMsg::TaskDied { who, payload } => issues.push(ShutdownIssue {
                    stage: "shutdown_upstream".to_owned(),
                    worker: Some(who.to_string()),
                    detail: super::prepared_stop::panic_detail(payload.as_ref()),
                    diagnostic: None,
                    structured_error: None,
                }),
                // 失败路径只耗尽未发布事实，不能把它们冒作已核验的正常 Final。
                _ => {}
            }
        }
        let (worker_results, worker_issues) = completion_worker_results(results);
        issues.extend(worker_issues);
        (
            ShutdownCompletion {
                owned_workers: owned,
                joined_workers: worker_results.iter().filter(|worker| worker.child_released).count(),
                receiver_recovered: worker_results.len() == owned && worker_results.iter().all(|worker| worker.receiver_recovered),
                messages_drained: worker_results.len() == owned && worker_results.iter().all(|worker| worker.messages_drained),
                assets_reconciled: worker_results.len() == owned && worker_results.iter().all(|worker| worker.assets_reconciled),
                child_released: worker_results.len() == owned && worker_results.iter().all(|worker| worker.child_released),
                worker_results,
                group_finals: 0,
                desk_finals: 0,
                #[cfg(test)]
                observed_group_shutdowns: Vec::new(),
            },
            issues,
        )
    }
}

/// 只转换已取得的正常协议报告；内部 worker Err 不能被旧 Ok(report) 洗成成功。
fn complete_shutdown_report(
    owned_workers: usize,
    report: ShutdownReport,
) -> Result<ShutdownCompletion, ShutdownFailure> {
    #[cfg(test)]
    let observed_group_shutdowns = report
        .group_finals
        .iter()
        .filter(|group| {
            group
                .observed
                .iter()
                .any(|observed| matches!(observed, crate::runtime::group::Observed::Shutdown))
        })
        .map(|group| format!("group-{}", group.group))
        .collect();
    let (worker_results, mut issues) = completion_worker_results(report.worker_results);
    let completion = ShutdownCompletion {
        owned_workers,
        joined_workers: worker_results.iter().filter(|worker| worker.child_released).count(),
        receiver_recovered: worker_results.len() == owned_workers && worker_results.iter().all(|worker| worker.receiver_recovered),
        messages_drained: worker_results.len() == owned_workers && worker_results.iter().all(|worker| worker.messages_drained),
        assets_reconciled: worker_results.len() == owned_workers && worker_results.iter().all(|worker| worker.assets_reconciled),
        child_released: worker_results.len() == owned_workers && worker_results.iter().all(|worker| worker.child_released),
        worker_results,
        group_finals: report.group_finals.len(),
        desk_finals: report.desk_finals.len(),
        #[cfg(test)]
        observed_group_shutdowns,
    };
    if completion.joined_workers != owned_workers {
        issues.insert(
            0,
            ShutdownIssue {
                stage: "join_count".to_owned(),
                worker: None,
                detail: format!(
                    "owned={owned_workers}，实际join={}",
                    completion.joined_workers
                ),
                diagnostic: None,
                structured_error: None,
            },
        );
    }
    if issues.is_empty() {
        Ok(completion)
    } else {
        let primary = issues.remove(0);
        Err(ShutdownFailure {
            primary,
            secondary: issues,
            completion: Some(Box::new(completion)),
        })
    }
}

fn runtime_issue(stage: &str, error: RuntimeError) -> ShutdownIssue {
    fn worker_of(error: &RuntimeError) -> Option<String> {
        match error {
        RuntimeError::GroupFinalUndelivered { who, .. } => Some(who.to_string()),
        RuntimeError::DeskFinalUndelivered { who, .. } => Some(who.to_string()),
        RuntimeError::WorkerFailed { who, .. } => Some(who.to_string()),
        RuntimeError::Disconnected { who, .. } => who.map(|who| who.to_string()),
        RuntimeError::HostAborted { who, .. } => who.map(|who| who.to_string()),
        RuntimeError::HostAbortedCleanup { primary, .. } => worker_of(primary),
        RuntimeError::DayEndRollbackHostAborted { host_abort, .. } => worker_of(host_abort),
        _ => None,
        }
    }
    let worker = worker_of(&error);
    ShutdownIssue {
        stage: stage.to_owned(),
        worker,
        detail: error.to_string(),
        diagnostic: None,
        structured_error: Some(error),
    }
}

fn completion_worker_results(
    results: Vec<(WorkerId, Result<(), RuntimeError>)>,
) -> (Vec<ShutdownWorkerResult>, Vec<ShutdownIssue>) {
    let mut issues = Vec::new();
    let workers = results
        .into_iter()
        .map(|(who, result)| {
            let (result, structured_error) = match result {
                Ok(()) => (Ok(()), None),
                Err(error) => (Err(error.to_string()), Some(error)),
            };
            let (receiver_recovered, messages_drained, assets_reconciled, child_released) = match &structured_error {
                Some(RuntimeError::DownlinkCleanupFailed { payload, assets_reconciled, .. }) => {
                    let messages_drained = payload
                        .inspect::<super::RetainedExitMessages, _>(|retained| matches!(
                            &retained.drain_state,
                            super::RetainedExitDrainState::DrainedWithMessages,
                        ))
                        .unwrap_or(false);
                    (true, messages_drained, *assets_reconciled, true)
                }
                Some(RuntimeError::WorkerJoinFailed { receiver_recovered, child_released, .. }) => (*receiver_recovered, false, false, *child_released),
                Some(RuntimeError::WorkerJoinPending { .. }) => (false, false, false, false),
                _ => (true, true, false, true),
            };
            if let Err(detail) = &result {
                issues.push(ShutdownIssue {
                    stage: "worker_join".to_owned(),
                    worker: Some(who.to_string()),
                    detail: detail.clone(),
                    diagnostic: None,
                    structured_error: structured_error.clone(),
                });
            }
            ShutdownWorkerResult {
                worker: who.to_string(),
                result,
                receiver_recovered,
                messages_drained,
                assets_reconciled,
                child_released,
                structured_error,
            }
        })
        .collect();
    (workers, issues)
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests;
