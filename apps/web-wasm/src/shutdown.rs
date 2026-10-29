//! 指定 session 的唯一 Registry owner 消费入口；不改变 pause/drop_session 契约。
use super::{ingress_operation, SessionError, SessionRegistry};
use engine::runtime::world::{ShutdownFailure, ShutdownIssue};
use engine::session::SessionShutdownReport;

/// 仅关联本次请求的 handle；不增加 memory 范围的停止 authority。
#[derive(serde::Serialize)]
pub(super) struct SessionShutdownReply<T> {
    pub handle: u32,
    pub result: T,
}

impl SessionRegistry {
    pub(super) fn shutdown(&mut self, handle: u32) -> Result<SessionShutdownReport, ShutdownFailure> {
        self.shutdown_with_ingress_revoke(handle, |handle| {
            ingress_operation(|registry| {
                if let Some(service) = registry.remove(&handle) {
                    service.close()?;
                }
                Ok(())
            })
        })
    }

    /// 局部撤销错误 seam；生产调用只传真实 INGRESS 操作，不持其锁等待 join。
    fn shutdown_with_ingress_revoke(
        &mut self,
        handle: u32,
        revoke: impl FnOnce(u32) -> Result<(), SessionError>,
    ) -> Result<SessionShutdownReport, ShutdownFailure> {
        // 摘除唯一 owner 后，后续控制入口即不能进入；INGRESS 锁不跨 join。
        let session = self.sessions.remove(&handle).ok_or_else(|| ShutdownFailure {
            primary: ShutdownIssue { stage: "registry_handle".to_owned(), worker: None,
                detail: format!("未知或已消费的 session handle：{handle}") },
            secondary: Vec::new(), completion: None,
        })?;
        let revoke_failure = revoke(handle).err().map(|error| ShutdownIssue {
            stage: "registry_ingress_revoke".to_owned(), worker: None, detail: error.to_string(),
        });
        let stopped = session.shutdown();
        match (revoke_failure, stopped) {
            (None, result) => result,
            (Some(primary), Ok(report)) => Err(ShutdownFailure {
                primary, secondary: Vec::new(), completion: report.runtime,
            }),
            (Some(primary), Err(failure)) => {
                let mut secondary = vec![failure.primary];
                secondary.extend(failure.secondary);
                Err(ShutdownFailure { primary, secondary, completion: failure.completion })
            }
        }
    }
}

#[cfg(all(test, not(target_family = "wasm")))]
mod tests;
