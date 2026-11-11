//! 消费现有 session owner；不创建第二个 driver，不证明整个 memory 已停止。
use super::{GameSession, StepFatal};
use crate::runtime::world::{
    ShutdownCompletion, ShutdownDiagnostic, ShutdownFailure, ShutdownIssue,
};

/// None 表示该 session 没有 runtime driver，不代表共享 ComputeContext 没有线程。
#[derive(Clone, Debug, serde::Serialize)]
pub struct SessionShutdownReport {
    pub runtime: Option<ShutdownCompletion>,
}

impl GameSession {
    /// 先关闭已有 ingress，再消费已有 driver；错误不能令清理提前返回。
    pub fn shutdown(mut self) -> Result<SessionShutdownReport, ShutdownFailure> {
        let mut issues = Vec::new();
        if let Some(poison) = self.poison.take() {
            issues.push(session_poison_issue(poison));
        }
        if let Some(binding) = &self.ingress {
            if let Err(error) = binding.source.close() {
                issues.push(ShutdownIssue::new(
                    "ingress_close",
                    None,
                    error.to_string(),
                ));
            }
        }
        let mut completion = None;
        if let Some(driver) = self.runtime_driver.take() {
            let driver = match driver.into_inner() {
                Ok(driver) => driver,
                Err(poison) => {
                    issues.push(ShutdownIssue::new(
                        "runtime_driver_mutex",
                        None,
                        "runtime driver Mutex poison；仅取回 owner 作失败销毁",
                    ));
                    poison.into_inner()
                }
            };
            if issues.is_empty() {
                match driver.shutdown() {
                    Ok(report) => completion = Some(report),
                    Err(failure) => {
                        issues.push(failure.primary);
                        issues.extend(failure.secondary);
                        completion = failure.completion.map(|report| *report);
                    }
                }
            } else {
                let (report, secondary) = driver.shutdown_after_failure();
                completion = Some(report);
                issues.extend(secondary);
            }
        }
        if issues.is_empty() {
            Ok(SessionShutdownReport {
                runtime: completion,
            })
        } else {
            let primary = issues.remove(0);
            Err(ShutdownFailure {
                primary,
                secondary: issues,
                completion: completion.map(Box::new),
            })
        }
    }
}

/// Session fatal 只导出位置和文本诊断；不把 session 私有状态、资产快照或恢复载荷放进 wire。
fn session_poison_issue(fatal: StepFatal) -> ShutdownIssue {
    match &fatal {
        StepFatal::InvariantViolation {
            description,
            location,
        } => ShutdownIssue {
            stage: "session_poison".to_owned(),
            worker: None,
            // 保留既有 Debug 文本供旧调用方查看，新增字段提供可机器读取的安全诊断。
            detail: format!("{fatal:?}"),
            diagnostic: Some(ShutdownDiagnostic {
                kind: "InvariantViolation".to_owned(),
                location: location.clone(),
                message: description.clone(),
            }),
            structured_error: None,
        },
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests;
