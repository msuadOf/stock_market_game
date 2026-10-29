//! 活动 World 显式停止结果；这里只记录 owned worker，不代表宿主 child exit。
use super::{ShutdownReport, World};
use crate::runtime::protocol::{RuntimeError, UpMsg, WorkerId};

/// 显式停止诊断；worker 使用 WorkerId::Display 的 group-N / desk-N 身份。
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShutdownIssue {
    pub stage: String,
    pub worker: Option<String>,
    pub detail: String,
}

/// 一次真实 owned handle join 的结果，保留 worker 原错误或 join 错误详情。
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShutdownWorkerResult {
    pub worker: String,
    pub result: Result<(), String>,
}

/// 已观察到的 World 收尾事实；Final 仅为核验后的计数，不传输期末账户树。
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShutdownCompletion {
    pub owned_workers: usize,
    pub joined_workers: usize,
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
    pub completion: Option<ShutdownCompletion>,
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
                },
                secondary,
                completion: Some(completion),
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
                    completion: Some(completion),
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
                }),
                UpMsg::TaskDied { who, payload } => issues.push(ShutdownIssue {
                    stage: "shutdown_upstream".to_owned(),
                    worker: Some(who.to_string()),
                    detail: super::prepared_stop::panic_detail(payload.as_ref()),
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
                joined_workers: worker_results.len(),
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
        joined_workers: worker_results.len(),
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
            completion: Some(completion),
        })
    }
}

fn runtime_issue(stage: &str, error: RuntimeError) -> ShutdownIssue {
    let worker = match &error {
        RuntimeError::WorkerFailed { who, .. } => Some(who.to_string()),
        RuntimeError::Disconnected { who, .. } => who.map(|who| who.to_string()),
        #[cfg(target_family = "wasm")]
        RuntimeError::HostAborted { who, .. } => who.map(|who| who.to_string()),
        _ => None,
    };
    ShutdownIssue {
        stage: stage.to_owned(),
        worker,
        detail: error.to_string(),
    }
}

fn completion_worker_results(
    results: Vec<(WorkerId, Result<(), RuntimeError>)>,
) -> (Vec<ShutdownWorkerResult>, Vec<ShutdownIssue>) {
    let mut issues = Vec::new();
    let workers = results
        .into_iter()
        .map(|(who, result)| {
            let result = result.map_err(|error| error.to_string());
            if let Err(detail) = &result {
                issues.push(ShutdownIssue {
                    stage: "worker_join".to_owned(),
                    worker: Some(who.to_string()),
                    detail: detail.clone(),
                });
            }
            ShutdownWorkerResult {
                worker: who.to_string(),
                result,
            }
        })
        .collect();
    (workers, issues)
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests;
