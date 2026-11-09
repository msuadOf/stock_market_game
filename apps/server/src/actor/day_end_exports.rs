//! 已授权文件导出的连接级所有权；临时 spool 不承担自动存档职责。

use super::*;
use std::collections::BTreeMap;

pub const DEFAULT_DAY_END_EXPORT_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct DayEndExportFailure {
    pub subscription: String,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub enum DayEndExportRequest {
    Subscribe {
        connection: String,
        failures: mpsc::UnboundedSender<DayEndExportFailure>,
    },
    Unsubscribe {
        connection: String,
        subscription: String,
    },
    Read {
        subscription: String,
        candidate: engine::session::protocol::SaveCandidateKey,
    },
    Acknowledge {
        subscription: String,
        candidate: engine::session::protocol::SaveCandidateKey,
    },
}

#[derive(Debug)]
pub enum DayEndExportResponse {
    Subscribed {
        subscription: String,
        after_seq: u64,
    },
    Completed,
    Saved(Box<SaveSlot>),
}

struct Subscriber {
    subject: OpaqueSubjectId,
    generation: u64,
    connection: String,
    failures: mpsc::UnboundedSender<DayEndExportFailure>,
}

pub(super) struct DayEndExports {
    byte_limit: u64,
    spool: Option<native_store::DayEndExportSpool>,
    subscribers: BTreeMap<String, Subscriber>,
}

fn rejected(error: impl std::fmt::Display) -> SendCommandError {
    SendCommandError::Rejected(error.to_string())
}

impl DayEndExports {
    pub(super) fn new(byte_limit: u64) -> Self {
        Self {
            byte_limit,
            spool: None,
            subscribers: BTreeMap::new(),
        }
    }

    pub(super) fn has_subscribers(&self) -> bool {
        !self.subscribers.is_empty()
    }

    fn owner(
        &self,
        generation: u64,
        subject: &OpaqueSubjectId,
        token: &str,
    ) -> Result<(), SendCommandError> {
        match self.subscribers.get(token) {
            Some(owner) if owner.generation == generation && &owner.subject == subject => Ok(()),
            _ => Err(rejected(
                "DAY_END_EXPORT_UNAVAILABLE: 导出订阅已失效或不属于当前主体及 generation",
            )),
        }
    }

    fn remove(&mut self, token: &str) -> Result<(), native_store::StoreError> {
        if let Some(spool) = &mut self.spool {
            spool.unsubscribe(token)?;
        }
        self.subscribers.remove(token);
        if self.subscribers.is_empty() {
            self.spool = None;
        }
        Ok(())
    }

    fn cancel(
        &mut self,
        token: &str,
        code: &'static str,
        message: &str,
    ) -> Result<(), native_store::StoreError> {
        if let Some(owner) = self.subscribers.get(token) {
            if owner
                .failures
                .send(DayEndExportFailure {
                    subscription: token.to_owned(),
                    code,
                    message: message.to_owned(),
                })
                .is_err()
            {
                debug!("文件导出连接已关闭；回收其临时候选");
            }
        }
        self.remove(token)
    }

    pub(super) fn reset(&mut self, code: &'static str, message: &str) {
        for (token, owner) in &self.subscribers {
            if owner
                .failures
                .send(DayEndExportFailure {
                    subscription: token.clone(),
                    code,
                    message: message.to_owned(),
                })
                .is_err()
            {
                debug!("文件导出连接已关闭；释放临时 spool");
            }
        }
        self.subscribers.clear();
        self.spool = None;
    }

    pub(super) fn publish(&mut self, candidate: &native_store::DayEndCandidate) {
        loop {
            let Some(spool) = &mut self.spool else {
                return;
            };
            match spool.publish(candidate) {
                Ok(()) => return,
                Err(error @ native_store::StoreError::ExportCapacity { .. }) => {
                    // 超限只停止占用最多的导出；其他订阅和自动存档继续。
                    let slowest = self
                        .subscribers
                        .keys()
                        .map(|token| {
                            spool
                                .pending_bytes(token)
                                .map(|bytes| (bytes, token.clone()))
                        })
                        .collect::<Result<Vec<_>, _>>();
                    let token = match slowest {
                        Ok(rows) => rows.into_iter().max().map(|(_, token)| token),
                        Err(cause) => {
                            self.reset(
                                "DAY_END_EXPORT_FAILED",
                                &format!("导出队列资源核对失败：{cause}"),
                            );
                            return;
                        }
                    };
                    let Some(token) = token else {
                        self.spool = None;
                        return;
                    };
                    if let Err(cause) = self.cancel(&token, "DAY_END_EXPORT_CAPACITY", &format!("日终文件导出已停止：{error}；请重新授权文件后继续，交易与主存档不受影响")) {
                        self.reset("DAY_END_EXPORT_FAILED", &format!("导出队列回收失败：{cause}"));
                        return;
                    }
                }
                Err(error) => {
                    self.reset(
                        "DAY_END_EXPORT_FAILED",
                        &format!("日终文件导出已停止：{error}；交易与主存档不受影响"),
                    );
                    return;
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn retained_bytes(&self) -> Result<u64, native_store::StoreError> {
        self.spool
            .as_ref()
            .map_or(Ok(0), |spool| spool.retained_bytes())
    }
}

impl SessionActor {
    fn validate_export_generation(
        &self,
        generation: u64,
        subject: &OpaqueSubjectId,
    ) -> Result<(), SendCommandError> {
        if generation != self.timeline_generation {
            return Err(rejected("STALE_SESSION_GENERATION"));
        }
        self.require_market_control(subject)
    }

    pub(super) fn subscribe_day_end_exports(
        &mut self,
        generation: u64,
        subject: &OpaqueSubjectId,
        connection: &str,
        failures: mpsc::UnboundedSender<DayEndExportFailure>,
    ) -> Result<String, SendCommandError> {
        self.validate_export_generation(generation, subject)?;
        if self
            .day_end_exports
            .subscribers
            .values()
            .any(|owner| owner.connection == connection)
        {
            return Err(rejected("该连接已有文件导出订阅；请先取消原订阅"));
        }
        if failures.is_closed() {
            return Err(rejected("文件导出连接已经关闭"));
        }
        if self.day_end_exports.spool.is_none() {
            self.day_end_exports.spool = Some(
                native_store::DayEndExportSpool::new(self.day_end_exports.byte_limit)
                    .map_err(rejected)?,
            );
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.day_end_exports
            .spool
            .as_mut()
            .expect("订阅前已初始化 spool")
            .subscribe(&token)
            .map_err(rejected)?;
        self.day_end_exports.subscribers.insert(
            token.clone(),
            Subscriber {
                subject: subject.clone(),
                generation,
                connection: connection.to_owned(),
                failures,
            },
        );
        Ok(token)
    }

    pub(super) fn unsubscribe_day_end_exports(
        &mut self,
        generation: u64,
        subject: &OpaqueSubjectId,
        connection: &str,
        token: &str,
    ) -> Result<(), SendCommandError> {
        self.validate_export_generation(generation, subject)?;
        self.day_end_exports.owner(generation, subject, token)?;
        if self.day_end_exports.subscribers[token].connection != connection {
            return Err(rejected("文件导出订阅不属于当前连接"));
        }
        self.day_end_exports.remove(token).map_err(rejected)
    }

    pub(super) fn read_day_end_export(
        &self,
        generation: u64,
        subject: &OpaqueSubjectId,
        token: &str,
        candidate: &engine::session::protocol::SaveCandidateKey,
    ) -> Result<SaveSlot, SendCommandError> {
        self.validate_export_generation(generation, subject)?;
        self.day_end_exports.owner(generation, subject, token)?;
        self.day_end_exports
            .spool
            .as_ref()
            .ok_or_else(|| rejected("导出队列不存在"))?
            .read(token, candidate)
            .map_err(rejected)
    }

    pub(super) fn ack_day_end_export(
        &mut self,
        generation: u64,
        subject: &OpaqueSubjectId,
        token: &str,
        candidate: &engine::session::protocol::SaveCandidateKey,
    ) -> Result<(), SendCommandError> {
        self.validate_export_generation(generation, subject)?;
        self.day_end_exports.owner(generation, subject, token)?;
        self.day_end_exports
            .spool
            .as_mut()
            .ok_or_else(|| rejected("导出队列不存在"))?
            .acknowledge(token, candidate)
            .map_err(rejected)
    }

    pub(super) fn drop_day_end_export_connection(&mut self, connection: &str) {
        let tokens: Vec<_> = self
            .day_end_exports
            .subscribers
            .iter()
            .filter(|(_, owner)| owner.connection == connection)
            .map(|(token, _)| token.clone())
            .collect();
        for token in tokens {
            if let Err(error) = self.day_end_exports.cancel(
                &token,
                "DAY_END_EXPORT_CONNECTION_CLOSED",
                "文件导出连接已关闭或重新同步，请重新授权文件导出",
            ) {
                self.day_end_exports
                    .reset("DAY_END_EXPORT_FAILED", &format!("断连清理失败：{error}"));
                break;
            }
        }
    }
}

impl SessionHandles {
    pub async fn day_end_export(
        &self,
        generation: u64,
        subject: OpaqueSubjectId,
        request: DayEndExportRequest,
    ) -> Result<DayEndExportResponse, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::DayEndExport {
                generation,
                subject,
                request,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)?
    }

    pub fn drop_day_end_export_connection(&self, connection: String) {
        if self
            .cmd_tx
            .send(SessionCommand::DropDayEndExportConnection { connection })
            .is_err()
        {
            debug!("actor 已退出，导出 spool 随 actor 释放");
        }
    }
}
