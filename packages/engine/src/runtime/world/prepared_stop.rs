//! 只回收私有 prepared World；不 Drain、不 Abort、不安装或发布 Final。
use super::*;

#[derive(Debug)]
pub(crate) struct StopReport {
    pub joined_workers: usize,
    pub discarded_messages: usize,
}

pub(super) fn panic_detail(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_owned();
    }
    format!("非字符串 panic payload，type_id={:?}", payload.type_id())
}

/// getter 不得用 UpMsg 的省略 Debug 丢弃 panic 载荷详情。
pub(super) fn read_failure(stage: &str, message: UpMsg) -> RuntimeError {
    match message {
        UpMsg::Failed { who, error } => RuntimeError::WorkerFailed {
            who,
            error: Box::new(error),
        },
        UpMsg::TaskDied { who, payload } => RuntimeError::ProtocolInvariant {
            detail: format!(
                "{stage} 收到 worker {who} panic：{}",
                panic_detail(payload.as_ref())
            ),
        },
        other => RuntimeError::ProtocolInvariant {
            detail: format!("{stage} 收到非预期消息：{other:?}"),
        },
    }
}

impl World {
    pub(crate) fn stop_without_drain(mut self) -> Result<StopReport, RuntimeError> {
        self.worker_shutdown.store(true, Ordering::SeqCst);
        self.group_txs.clear();
        self.desk_txs.clear();
        let mut errors = Vec::new();
        let mut joined_workers = 0;
        for (who, handle) in std::mem::take(&mut self.handles) {
            joined_workers += 1;
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => errors.push(format!("worker {who}：{error}")),
                Err(error) => {
                    #[cfg(not(target_family = "wasm"))]
                    let detail = panic_detail(error.as_ref());
                    #[cfg(target_family = "wasm")]
                    let detail = error.to_string();
                    errors.push(format!("join {who} 失败：{detail}"));
                }
            }
        }
        let mut discarded_messages = 0;
        for message in self.up_rx.try_iter() {
            discarded_messages += 1;
            match message {
                UpMsg::Failed { who, error } => {
                    errors.push(format!("未交付 worker {who} 错误：{error}"))
                }
                UpMsg::TaskDied { who, payload } => errors.push(format!(
                    "未交付 worker {who} panic：{}",
                    panic_detail(payload.as_ref())
                )),
                _ => {}
            }
        }
        let report = StopReport {
            joined_workers,
            discarded_messages,
        };
        if errors.is_empty() {
            Ok(report)
        } else {
            Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "prepared World 无 Drain 回收失败：{report:?}；全部错误={errors:?}"
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_read_failure_keeps_original_worker_panic_detail() {
        let error = read_failure(
            "static Full",
            UpMsg::TaskDied {
                who: WorkerId::Group(1),
                payload: Box::new("prepared-worker-original-panic".to_owned()),
            },
        );
        assert!(error.to_string().contains("prepared-worker-original-panic"));
        assert!(error.to_string().contains("static Full"));
    }
}
