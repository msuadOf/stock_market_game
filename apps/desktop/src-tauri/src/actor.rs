//! 每个会话的权威市场状态由一个 actor 独占（ADR-0005 §5）。
//!
//! 市场节拍和推进由共享 runner 决定；LibOS 等待控制命令或市场唤醒。
//! actor 只交接已冻结的输出，独立 Tauri 发送任务负责编码与发送。
//! Player 与 NPC 共用 ingress，控制命令用邮箱与 oneshot 传递结果。
//! 本地模式的 Player 意图仍路由给 AccountId(0)。

#[cfg(test)]
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use engine::session::checkpoint::{
    CapturedCheckpoint, CheckpointKey, CheckpointOrder, ReceiptDisposition,
};
use engine::session::protocol::{
    CivilUpdate, EngineUpdate, PausePreferences, PlayerWorkingOrder, ProtocolSession, TickBatch,
    TickFrame,
};
#[cfg(test)]
use engine::session::runner::fixed_tick_duration as fixed_tick_interval;
use engine::session::runner::{
    apply_cycle_pacing, run_cycle, CycleError, DrivePort, Pacing as DesktopPacing, Permit, WakePlan,
};
pub use engine::session::runner::{SpeedMetrics, BASE_TICK_MS};
use engine::{
    calendar::CivilDate,
    company::{PublicReportPage, PublicReportQuery, PublicReportSummary},
    session::HistoricalStockData,
    AccountId, Intent, SaveSlot, SessionError, SessionSetup, Snapshot,
};
use libos::native_async::{self, WaitPlan, Wake};
use libos::native_async::{mpsc, oneshot, Mutex as AsyncMutex};
use native_store::{
    archive_lane::ArchiveLane, ArchiveSelection, ArchiveWriter, NativeDatabase, StoreError,
};
use serde::Serialize;
use tauri::{AppHandle, Runtime};

use crate::EngineEventPayload;
mod delivery;
use delivery::{AckRoute, Delivery, Packet};
mod failure;
pub(crate) use failure::HostFailure;
#[cfg(test)]
mod correction_recovery_tests;
#[cfg(test)]
mod fatal_tests;
#[cfg(test)]
mod protocol_tests;

#[cfg(test)]
const UI_PUBLISH_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Debug, Clone, Serialize)]
pub struct RestoreResult {
    pub snapshot: Snapshot,
    pub working_orders: Vec<PlayerWorkingOrder>,
    pub publication_ordinal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_checkpoint: Option<engine::session::protocol::SaveCandidateKey>,
    pub stock_catalog: Vec<engine::session::StockIdentityEntry>,
    pub company_catalog: Vec<engine::session::CompanyCatalogEntry>,
    pub civil_date: CivilDate,
    pub timeline_id: String,
    pub generation: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmationQuery {
    #[serde(deserialize_with = "required_cursor")]
    before_receipt: Option<String>,
}

fn required_cursor<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    <Option<String> as serde::Deserialize>::deserialize(deserializer)
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
pub enum HistoryQuery {
    MarketHistory(engine::session::MarketHistoryRequest),
    CurrentMinuteHistory(engine::session::CurrentMinuteHistoryRequest),
    PersonalTradeHistory(engine::session::PersonalTradeHistoryRequest),
    PersonalTradeConfirmations(ConfirmationQuery),
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerationResponse<T> {
    pub generation: String,
    pub value: T,
}

#[derive(Debug, Serialize)]
pub struct CompanyReadResponse {
    pub generation: String,
    pub owner: AccountId,
    pub value: engine::session::protocol::CompanyReadResult,
}

#[derive(Debug)]
pub enum CompanyReadHostError {
    Session { message: String },
    Query(engine::session::protocol::CompanyReadError),
}

fn diagnostic_generation_response(
    requested_generation: u64,
    current_generation: u64,
    read: impl FnOnce() -> engine::NpcDecisionDiagnostics,
) -> Result<GenerationResponse<engine::NpcDecisionDiagnostics>, SessionError> {
    if requested_generation != current_generation {
        // 1310：世代过期；ctx[0]=请求世代，ctx[1]=当前世代。
        return Err(SessionError::InvalidSave(
            engine::error::ValidationFault::new(
                1310,
                vec![requested_generation, current_generation],
            ),
        ));
    }
    Ok(GenerationResponse {
        generation: requested_generation.to_string(),
        value: read(),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum CompanyWriteHostError {
    #[error("公司写命令宿主失败：{message}")]
    Session { message: String },
    #[error("{0}")]
    Command(#[from] engine::session::protocol::CompanyWriteError),
    #[error("公司写命令已触发引擎致命错误：{0}")]
    Fatal(#[source] engine::session::StepFatal),
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum CompanyWriteHostFailure {
    Command(engine::session::protocol::CompanyWriteFailure),
    Fatal(HostFailure),
    Session { message: String },
}

impl CompanyWriteHostError {
    pub(crate) fn public_failure(&self) -> CompanyWriteHostFailure {
        match self {
            Self::Session { message } => CompanyWriteHostFailure::Session {
                message: message.clone(),
            },
            Self::Command(error) => CompanyWriteHostFailure::Command(error.public_failure()),
            Self::Fatal(fatal) => CompanyWriteHostFailure::Fatal(HostFailure::step(fatal.clone())),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RightsSubscriptionResponse {
    pub generation: String,
    pub receipt: engine::session::RightsSubscriptionReceipt,
}

#[derive(Debug, Serialize)]
pub struct CompanyProposalResponse {
    pub generation: String,
    pub receipt: engine::session::PlayerCompanyCommandReceipt,
}

#[derive(Debug, Serialize)]
pub struct ReportCorrectionResponse {
    pub generation: String,
    pub operation_id: String,
}

/// 发给 actor 的命令；控制回执表示 actor 已应用，不只是进入 mpsc。
///
/// `reply` 用 `Result` 而非裸值：engine 失败（`SessionError`）显式上抛，绝不静默吞（铁律二）。
#[derive(Debug)]
pub enum SessionCommand {
    SubscribeRightsOffering {
        generation: u64,
        event_id: String,
        requested_shares: u64,
        reply: oneshot::Sender<Result<RightsSubscriptionResponse, CompanyWriteHostError>>,
    },
    ProposePlayerCompanyAction {
        generation: u64,
        proposal: engine::session::PlayerCompanyProposal,
        reply: oneshot::Sender<Result<CompanyProposalResponse, CompanyWriteHostError>>,
    },
    EnqueueReportCorrection {
        generation: u64,
        request: engine::session::CompanyReportCorrection,
        reply: oneshot::Sender<Result<ReportCorrectionResponse, CompanyWriteHostError>>,
    },
    CancelReportCorrection {
        generation: u64,
        operation_id: String,
        reply: oneshot::Sender<Result<ReportCorrectionResponse, CompanyWriteHostError>>,
    },
    CompanyRead {
        generation: u64,
        query: engine::session::protocol::CompanyReadQuery,
        reply: oneshot::Sender<Result<CompanyReadResponse, CompanyReadHostError>>,
    },
    /// 入队玩家意图（当前固定玩家 0）。Ok=已入队，Err=engine 拒绝。
    Enqueue {
        player_id: AccountId,
        intent: Intent,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    /// 取完整快照。
    Snapshot { reply: oneshot::Sender<Snapshot> },
    PlayerWorkingOrders {
        generation: u64,
        reply: oneshot::Sender<Result<GenerationResponse<serde_json::Value>, SessionError>>,
    },
    HostHistoryQuery {
        generation: u64,
        query: HistoryQuery,
        reply: oneshot::Sender<Result<GenerationResponse<serde_json::Value>, String>>,
    },
    StockHistory {
        generation: u64,
        code: engine::StockId,
        reply: oneshot::Sender<Result<GenerationResponse<HistoricalStockData>, SessionError>>,
    },
    InitialAllocation {
        generation: u64,
        reply: oneshot::Sender<
            Result<GenerationResponse<engine::session::InitialAllocation>, SessionError>,
        >,
    },
    QueryBaseline {
        generation: u64,
        reply: oneshot::Sender<Result<RestoreResult, BaselineError>>,
    },
    ResumeObserver {
        generation: u64,
        ordinal: u64,
        reply: oneshot::Sender<Result<(), String>>,
    },
    /// 取不含 360 日历史的轻量运行快照，供高倍率跨日同步。
    RuntimeSnapshot { reply: oneshot::Sender<Snapshot> },
    CivilDate {
        generation: u64,
        reply: oneshot::Sender<Result<GenerationResponse<CivilDate>, SessionError>>,
    },
    PublicReports {
        generation: u64,
        query: PublicReportQuery,
        reply: oneshot::Sender<Result<GenerationResponse<PublicReportPage>, SessionError>>,
    },
    PublicReportById {
        generation: u64,
        id: String,
        reply: oneshot::Sender<Result<GenerationResponse<PublicReportSummary>, SessionError>>,
    },
    NpcDecisionDiagnostics {
        generation: u64,
        account: AccountId,
        reply: oneshot::Sender<
            Result<GenerationResponse<engine::NpcDecisionDiagnostics>, SessionError>,
        >,
    },
    /// 读取设定速度与最近完成的实际 tick/现实秒采样。
    SpeedMetrics {
        reply: oneshot::Sender<SpeedMetrics>,
    },
    /// 生成可持久化存档。actor 独占会话，因此读取与 step 严格串行。
    Save {
        generation: u64,
        candidate: Option<engine::session::protocol::SaveCandidateKey>,
        reply: oneshot::Sender<Result<SaveSlot, SessionError>>,
    },
    /// 原子恢复存档：只有完整校验和重建成功后才替换当前会话。
    Restore {
        generation: u64,
        slot: Box<SaveSlot>,
        reply: oneshot::Sender<Result<RestoreResult, BaselineError>>,
    },
    #[cfg(feature = "host-parity")]
    AdvanceCivilDay {
        generation: u64,
        reply: oneshot::Sender<Result<CivilUpdate, SessionError>>,
    },
    #[cfg(feature = "host-parity")]
    Step {
        generation: u64,
        reply: oneshot::Sender<Result<EngineUpdate, SessionError>>,
    },
    /// 改变步进倍速（仅调整 interval，不触发立即 step）。
    SetSpeed {
        speed: f64,
        reply: oneshot::Sender<Result<(), SessionError>>,
    },
    /// 暂停或恢复步进，保留会话状态与事件订阅。
    SetRunning {
        running: bool,
        reply: oneshot::Sender<()>,
    },
    SetPausePreferences {
        preferences: PausePreferences,
        reply: oneshot::Sender<()>,
    },
    ArchiveWriteCompleted {
        key: CheckpointKey,
        result: Result<(), String>,
    },
    ReadQuickArchive {
        reply: oneshot::Sender<Result<Option<SaveSlot>, StoreError>>,
    },
    /// 永久结束 actor；用于页面卸载/应用退出释放资源。
    Shutdown { reply: oneshot::Sender<()> },
}

/// 一个 session 的对外句柄：命令发送端（克隆廉价）。
/// `SessionManager` 持 `Arc<SessionHandles>`，Tauri command 经 manager 取克隆与 actor 通信。
/// 原始命令发送端不可由外部访问，命令必须通过受控方法提交。
/// ```compile_fail,E0616
/// fn bypass(handles: &stock_market_game_lib::actor::SessionHandles) {
///     let _ = handles.cmd_tx.clone();
/// }
/// ```
#[derive(Clone)]
pub struct SessionHandles {
    cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    ingress: Arc<RwLock<(u64, engine::SharedSessionIngress)>>,
    ack_route: AckRoute,
}

impl SessionHandles {
    pub async fn subscribe_rights_offering(
        &self,
        generation: u64,
        event_id: String,
        requested_shares: u64,
    ) -> Result<RightsSubscriptionResponse, CompanyWriteHostError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SubscribeRightsOffering {
                generation,
                event_id,
                requested_shares,
                reply,
            })
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?;
        received
            .await
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?
    }

    pub async fn propose_player_company_action(
        &self,
        generation: u64,
        proposal: engine::session::PlayerCompanyProposal,
    ) -> Result<CompanyProposalResponse, CompanyWriteHostError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::ProposePlayerCompanyAction {
                generation,
                proposal,
                reply,
            })
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?;
        received
            .await
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?
    }

    pub async fn enqueue_report_correction(
        &self,
        generation: u64,
        request: engine::session::CompanyReportCorrection,
    ) -> Result<ReportCorrectionResponse, CompanyWriteHostError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::EnqueueReportCorrection {
                generation,
                request,
                reply,
            })
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?;
        received
            .await
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?
    }

    pub async fn cancel_report_correction(
        &self,
        generation: u64,
        operation_id: String,
    ) -> Result<ReportCorrectionResponse, CompanyWriteHostError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CancelReportCorrection {
                generation,
                operation_id,
                reply,
            })
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?;
        received
            .await
            .map_err(|error| CompanyWriteHostError::Session {
                message: error.to_string(),
            })?
    }

    pub async fn company_read_query(
        &self,
        generation: u64,
        query: engine::session::protocol::CompanyReadQuery,
    ) -> Result<CompanyReadResponse, CompanyReadHostError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CompanyRead {
                generation,
                query,
                reply,
            })
            .map_err(|error| CompanyReadHostError::Session {
                message: error.to_string(),
            })?;
        received
            .await
            .map_err(|error| CompanyReadHostError::Session {
                message: error.to_string(),
            })?
    }
    /// 便捷：入队玩家意图（固定 `AccountId(0)`），立即登记到共享 ingress。
    pub async fn enqueue(&self, generation: u64, intent: Intent) -> Result<(), SendCommandError> {
        self.enqueue_as(generation, AccountId(0), intent).await
    }

    /// 入队指定玩家意图（联机多账户预留，当前由 player 0 调用）。
    pub async fn enqueue_as(
        &self,
        generation: u64,
        player_id: AccountId,
        intent: Intent,
    ) -> Result<(), SendCommandError> {
        if self.cmd_tx.is_closed() {
            return Err(SendCommandError::ActorGone);
        }
        let ingress = self.ingress.read().map_err(|error| {
            SendCommandError::Rejected(format!("session ingress lifecycle lock poisoned: {error}"))
        })?;
        if generation != ingress.0 {
            return Err(SendCommandError::Rejected(format!(
                "stale session generation {generation}; current generation is {}",
                ingress.0
            )));
        }
        ingress
            .1
            .enqueue_player_intent(player_id, intent)
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    /// 取完整快照。actor 关闭 → `ActorGone`（绝不静默返回空快照）。
    pub async fn snapshot(&self) -> Result<Snapshot, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Snapshot { reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn player_working_orders(
        &self,
        generation: u64,
    ) -> Result<GenerationResponse<serde_json::Value>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PlayerWorkingOrders {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn host_history_query(
        &self,
        generation: u64,
        query: HistoryQuery,
    ) -> Result<GenerationResponse<serde_json::Value>, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::HostHistoryQuery {
                generation,
                query,
                reply: tx,
            })
            .map_err(|_| "actor 已关闭".to_owned())?;
        rx.await.map_err(|_| "actor 已关闭".to_owned())?
    }

    pub async fn query_stock_history(
        &self,
        generation: u64,
        code: engine::StockId,
    ) -> Result<GenerationResponse<HistoricalStockData>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::StockHistory {
                generation,
                code,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn initial_allocation(
        &self,
        generation: u64,
    ) -> Result<GenerationResponse<engine::session::InitialAllocation>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::InitialAllocation {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn query_baseline(
        &self,
        generation: u64,
    ) -> Result<RestoreResult, BaselineRequestError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::QueryBaseline {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(BaselineRequestError::Baseline)
    }

    pub async fn acknowledge_observer(&self, generation: u64, ordinal: u64) -> Result<(), String> {
        self.ack_route.acknowledge(generation, ordinal).await
    }

    pub async fn set_observer_held(&self, generation: u64, held: bool) -> Result<(), String> {
        self.ack_route.set_held(generation, held).await
    }

    pub async fn resume_observer(&self, generation: u64, ordinal: u64) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::ResumeObserver {
                generation,
                ordinal,
                reply: tx,
            })
            .map_err(|_| "OBSERVER_CLOSED: 市场会话已关闭".to_owned())?;
        rx.await
            .map_err(|_| "OBSERVER_CLOSED: 观察流已关闭".to_owned())?
    }

    /// 取轻量运行快照，避免跨日反复复制全部历史 K 线。
    pub async fn runtime_snapshot(&self) -> Result<Snapshot, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::RuntimeSnapshot { reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn civil_date(
        &self,
        generation: u64,
    ) -> Result<GenerationResponse<CivilDate>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::CivilDate {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn public_reports(
        &self,
        generation: u64,
        query: PublicReportQuery,
    ) -> Result<GenerationResponse<PublicReportPage>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicReports {
                generation,
                query,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn public_report_by_id(
        &self,
        generation: u64,
        id: String,
    ) -> Result<GenerationResponse<PublicReportSummary>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::PublicReportById {
                generation,
                id,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn npc_decision_diagnostics(
        &self,
        generation: u64,
        account: AccountId,
    ) -> Result<GenerationResponse<engine::NpcDecisionDiagnostics>, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::NpcDecisionDiagnostics {
                generation,
                account,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn speed_metrics(&self) -> Result<SpeedMetrics, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SpeedMetrics { reply: tx })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await.map_err(|_| SendCommandError::ActorGone)
    }

    /// Captures only the latest completed day when no key is supplied, or the exact
    /// completed-day candidate named by a CivilUpdate key; it never synthesizes a save.
    pub async fn save(
        &self,
        generation: u64,
        candidate: Option<engine::session::protocol::SaveCandidateKey>,
    ) -> Result<SaveSlot, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Save {
                generation,
                candidate,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(failure::save_error_message(error)))
    }

    pub async fn read_quick_archive(&self) -> Result<Option<SaveSlot>, SendCommandError> {
        let (reply, received) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::ReadQuickArchive { reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        received
            .await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn restore(
        &self,
        generation: u64,
        slot: SaveSlot,
    ) -> Result<RestoreResult, BaselineRequestError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Restore {
                generation,
                slot: Box::new(slot),
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(BaselineRequestError::Baseline)
    }

    #[cfg(feature = "host-parity")]
    pub async fn advance_civil_day(
        &self,
        generation: u64,
    ) -> Result<CivilUpdate, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::AdvanceCivilDay {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    #[cfg(feature = "host-parity")]
    pub async fn step(&self, generation: u64) -> Result<EngineUpdate, SendCommandError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Step {
                generation,
                reply: tx,
            })
            .map_err(|_| SendCommandError::ActorGone)?;
        rx.await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    /// 改变倍速并等待 actor 应用；actor 已关闭则 `ActorGone`。
    pub async fn set_speed(&self, speed: f64) -> Result<(), SendCommandError> {
        let (reply, applied) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetSpeed { speed, reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        applied
            .await
            .map_err(|_| SendCommandError::ActorGone)?
            .map_err(|error| SendCommandError::Rejected(error.to_string()))
    }

    pub async fn set_running(&self, running: bool) -> Result<(), SendCommandError> {
        let (reply, applied) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetRunning { running, reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        applied.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn shutdown(&self) -> Result<(), SendCommandError> {
        let (reply, applied) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::Shutdown { reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        applied.await.map_err(|_| SendCommandError::ActorGone)
    }

    pub async fn set_pause_preferences(
        &self,
        preferences: PausePreferences,
    ) -> Result<(), SendCommandError> {
        let (reply, applied) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SetPausePreferences { preferences, reply })
            .map_err(|_| SendCommandError::ActorGone)?;
        applied.await.map_err(|_| SendCommandError::ActorGone)
    }
}

/// 命令投递失败（actor 已退出 / engine 拒绝意图）。显式可见，不静默吞。
#[derive(Debug, thiserror::Error)]
pub enum BaselineError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error("无法切换桌面存档时间线：{0}")]
    Archive(#[from] StoreError),
    #[error("公开公司身份目录构造失败")]
    Catalog(#[from] engine::session::CompanyCatalogViewError),
}

#[derive(Debug, thiserror::Error)]
pub enum BaselineRequestError {
    #[error(transparent)]
    Command(#[from] SendCommandError),
    #[error(transparent)]
    Baseline(#[from] BaselineError),
}

#[derive(Debug, thiserror::Error)]
pub enum SendCommandError {
    /// actor task 已退出（session 被 drop 或 panic）。
    #[error("会话不存在或已退出（命令通道关闭）")]
    ActorGone,
    /// engine 拒绝意图（如未知玩家；单玩家正常路径不应触发，但显式上抛）。
    #[error("引擎拒绝指令：{0}")]
    Rejected(String),
}

/// Session 注册表：`DashMap<session_id, Arc<SessionHandles>>` 不便引入（桌面端无 dashmap 依赖），
/// 这里用 `tokio::sync::Mutex<HashMap>` 替代——写少（仅 create），读多但无热路径竞争，足够。
#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<AsyncMutex<std::collections::HashMap<String, Arc<SessionHandles>>>>,
    archive_database: Arc<std::sync::OnceLock<NativeDatabase>>,
    base_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum DesktopSessionInitError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error("无法启动桌面存档：{0}")]
    Archive(#[from] StoreError),
}

fn load_selected_archive(database: &NativeDatabase) -> Result<Option<SaveSlot>, StoreError> {
    match database.selection()? {
        ArchiveSelection::Selected(slot) => database
            .load(&slot)?
            .map(Some)
            .ok_or_else(|| StoreError::Invalid(format!("桌面当前存档槽 {slot} 不存在"))),
        ArchiveSelection::Uninitialized | ArchiveSelection::Cleared => Ok(None),
    }
}

impl SessionManager {
    /// 默认基准（`BASE_TICK_MS`）。
    pub fn default_base() -> Self {
        Self {
            sessions: Arc::new(AsyncMutex::new(std::collections::HashMap::new())),
            archive_database: Arc::new(std::sync::OnceLock::new()),
            base_ms: BASE_TICK_MS,
        }
    }

    pub fn initialize_archive(&self, database: NativeDatabase) -> Result<(), StoreError> {
        self.archive_database
            .set(database)
            .map_err(|_| StoreError::Invalid("桌面存档数据库重复初始化".into()))
    }

    pub async fn read_startup_archive(&self) -> Result<Option<SaveSlot>, StoreError> {
        let database = self
            .archive_database
            .get()
            .cloned()
            .ok_or_else(|| StoreError::Invalid("桌面存档数据库未初始化".into()))?;
        native_async::spawn_blocking(move || load_selected_archive(&database))
            .await
            .map_err(|error| StoreError::Invalid(format!("启动读档任务异常退出：{error}")))?
    }

    /// 创建新 session：构造 `GameSession` → 建 mpsc → spawn actor task → 注册。
    ///
    /// 失败显式返回 `SessionError`（构造非法参数），绝不静默吞（铁律二）。
    /// `app` 传入供 actor `emit` 事件给前端。
    pub async fn new_session<R: Runtime>(
        &self,
        setup: SessionSetup,
        seed: u64,
        app: AppHandle<R>,
    ) -> Result<String, DesktopSessionInitError> {
        let game = ProtocolSession::new(setup, seed)?;
        let archive = if let Some(database) = self.archive_database.get().cloned() {
            // writer 租约的互斥可能被上个会话的事务持有，不阻塞 Tokio executor。
            let writer_db = database.clone();
            let writer =
                native_async::spawn_blocking(move || ArchiveWriter::activate(writer_db, "current"))
                    .await
                    .map_err(|error| {
                        StoreError::Invalid(format!("桌面存档租约任务异常退出：{error}"))
                    })??;
            Some((writer, database))
        } else {
            None
        };
        let session_id = uuid::Uuid::new_v4().to_string();

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let ingress = Arc::new(RwLock::new((1, game.shared_ingress())));
        let ack_route = AckRoute::default();
        let archive_lane = archive.as_ref().map(|_| {
            let completion = cmd_tx.downgrade();
            ArchiveLane::spawn(move |key, result| {
                if let Err(error) = &result {
                    eprintln!(
                        "桌面异步日终归档失败 gen={} seq={}: {error}",
                        key.generation, key.seq
                    );
                }
                if let Some(completion) = completion.upgrade() {
                    let _ = completion.send(SessionCommand::ArchiveWriteCompleted { key, result });
                }
            })
        });
        let handles = Arc::new(SessionHandles {
            cmd_tx,
            ingress: Arc::clone(&ingress),
            ack_route: ack_route.clone(),
        });

        // 异步锁：Tauri command 在 Tokio runtime 内调用，禁止 blocking_lock 导致 panic。
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), handles.clone());

        let actor = SessionActor {
            ingress,
            #[cfg(test)]
            injected_step_failure: None,
            pacing: DesktopPacing::new(self.base_ms, game.tick(), native_async::now()),
            game,
            cmd_rx,
            session_id: session_id.clone(),
            delivery: Delivery::start(app, session_id.clone(), session_id.clone(), ack_route),
            timeline_id: session_id.clone(),
            generation: 1,
            pause_preferences: PausePreferences::default(),
            archive,
            archive_lane,
            archive_order: CheckpointOrder::new(1, 1),
            archive_epoch: 1,
        };
        native_async::spawn(actor.run());
        Ok(session_id)
    }

    /// 查询 session 句柄（克隆 `Arc<SessionHandles>`）。未知返回 `None`（不静默）。
    pub async fn lookup(&self, session_id: &str) -> Option<Arc<SessionHandles>> {
        self.sessions.lock().await.get(session_id).map(Arc::clone)
    }

    /// 从注册表移除会话并返回最后一个管理句柄，供 shutdown 命令结束 actor。
    pub async fn remove(&self, session_id: &str) -> Option<Arc<SessionHandles>> {
        self.sessions.lock().await.remove(session_id)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::default_base()
    }
}

#[cfg(test)]
#[derive(Default)]
struct StepFailureCapture {
    saved: Option<serde_json::Value>,
    origin: Option<engine::RuntimeMirrorOrigin>,
    business: Option<engine::session::StateHash>,
    frames: Vec<engine::session::protocol::TickFrame>,
}

/// actor 独占 `ProtocolSession`；IPC 不在市场执行任务中运行。
struct SessionActor<R: Runtime> {
    ingress: Arc<RwLock<(u64, engine::SharedSessionIngress)>>,
    #[cfg(test)]
    injected_step_failure: Option<(
        usize,
        engine::session::StepFatal,
        Arc<std::sync::Mutex<StepFailureCapture>>,
    )>,
    pacing: DesktopPacing,
    game: ProtocolSession,
    cmd_rx: mpsc::UnboundedReceiver<SessionCommand>,
    session_id: String,
    /// IPC 独立发送，actor 只向公共观察流交接。
    delivery: Delivery<R>,
    /// 每次成功读档都会更换；前端据此拒绝晚到的旧时间线 IPC。
    timeline_id: String,
    generation: u64,
    pause_preferences: PausePreferences,
    archive: Option<(ArchiveWriter, NativeDatabase)>,
    archive_lane: Option<ArchiveLane>,
    archive_order: CheckpointOrder,
    archive_epoch: u64,
}

#[cfg(test)]
struct ActorHarness {
    app: tauri::App<tauri::test::MockRuntime>,
    actor: SessionActor<tauri::test::MockRuntime>,
    cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    events_rx: Option<mpsc::UnboundedReceiver<String>>,
    failures_rx: Option<mpsc::UnboundedReceiver<String>>,
}

#[cfg(test)]
impl ActorHarness {
    fn new_protocol_actor(
        game: ProtocolSession,
        fastest: bool,
        pause_preferences: PausePreferences,
        session_id: &str,
        timeline_id: &str,
    ) -> Self {
        let app = tauri::test::mock_app();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let actor = SessionActor {
            ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
            injected_step_failure: None,
            pacing: {
                let mut pacing = DesktopPacing::new(1, game.tick(), native_async::now());
                pacing.set_running(true, game.tick(), native_async::now());
                if fastest {
                    pacing
                        .apply_speed(f64::INFINITY, game.tick(), native_async::now())
                        .unwrap();
                }
                pacing
            },
            game,
            cmd_rx,
            session_id: session_id.to_owned(),
            delivery: Delivery::inline_test(app.handle().clone()),
            timeline_id: timeline_id.to_owned(),
            generation: 1,
            pause_preferences,
            archive: None,
            archive_lane: None,
            archive_order: CheckpointOrder::new(1, 1),
            archive_epoch: 1,
        };
        Self {
            app,
            actor,
            cmd_tx,
            events_rx: None,
            failures_rx: None,
        }
    }

    fn subscribe_engine_events(&mut self) -> &mut mpsc::UnboundedReceiver<String> {
        if self.events_rx.is_none() {
            self.events_rx = Some(self.subscribe(crate::ENGINE_EVENT_NAME));
        }
        self.events_rx
            .as_mut()
            .expect("engine-event 订阅必须已建立")
    }

    fn subscribe_failures(&mut self) -> &mut mpsc::UnboundedReceiver<String> {
        if self.failures_rx.is_none() {
            self.failures_rx = Some(self.subscribe("engine-failure"));
        }
        self.failures_rx
            .as_mut()
            .expect("engine-failure 订阅必须已建立")
    }

    fn subscribe(&self, event_name: &str) -> mpsc::UnboundedReceiver<String> {
        use tauri::Listener;

        let (sender, receiver) = mpsc::unbounded_channel();
        self.app.listen(event_name, move |event| {
            sender.send(event.payload().to_owned()).unwrap();
        });
        receiver
    }
}

impl<R: Runtime> SessionActor<R> {
    fn step_game(&mut self) -> Result<TickFrame, engine::session::StepFatal> {
        #[cfg(test)]
        let capture = if let Some((remaining, error, capture)) = &mut self.injected_step_failure {
            if *remaining == 0 {
                let mut observed = capture.lock().unwrap();
                observed.saved =
                    Some(serde_json::to_value(self.game.game().save().unwrap()).unwrap());
                observed.origin = Some(self.game.runtime_mirror_origin());
                observed.business = Some(self.game.business_state_hash().unwrap());
                return Err(error.clone());
            }
            *remaining -= 1;
            Some(Arc::clone(capture))
        } else {
            None
        };
        let result = self.game.step_frame();
        #[cfg(test)]
        if let (Some(capture), Ok(frame)) = (capture, &result) {
            capture.lock().unwrap().frames.push(frame.clone());
        }
        result
    }
    /// 调度和业务推进只服从共享 runner；LibOS 只等待命令或 deadline。
    async fn run(mut self) {
        loop {
            let plan = match self.pacing.cadence.wake_plan() {
                WakePlan::Idle => WaitPlan::Idle,
                WakePlan::At(deadline) => WaitPlan::At(deadline),
                WakePlan::Immediate => WaitPlan::Yield,
            };
            match native_async::wait(&mut self.cmd_rx, plan).await {
                Some(Wake::Command(command)) => {
                    let shutdown = matches!(command, SessionCommand::Shutdown { .. });
                    self.handle_command(command).await;
                    if shutdown {
                        break;
                    }
                }
                Some(Wake::Deadline) => {
                    if let Some(permit) = self.pacing.cadence.begin_turn(native_async::now()) {
                        self.drive_cycle(permit);
                    }
                    if self.cmd_rx.is_closed() {
                        break;
                    }
                }
                None => break,
            }
        }
        self.finish_archive_lane().await;
        self.delivery.finish().await;
    }

    async fn finish_archive_lane(&mut self) {
        if let Some(lane) = self.archive_lane.take() {
            if let Err(error) = lane.finish().await {
                eprintln!("桌面归档任务退出失败：{error}");
            }
        }
    }

    fn stage_day_end(&mut self, update: &CivilUpdate) -> Result<(), Box<HostFailure>> {
        let Some((writer, _)) = self.archive.as_ref() else {
            return Ok(());
        };
        let fail =
            |reason: &dyn std::fmt::Display| Box::new(HostFailure::archive(&reason.to_string()));
        let Some(lane) = self.archive_lane.as_ref() else {
            return Err(Box::new(HostFailure::archive("会话没有初始化存档任务")));
        };
        let checkpoint =
            CapturedCheckpoint::take(&self.game, update, self.generation, self.archive_epoch)
                .map_err(|error| fail(&error))?;
        let key = checkpoint.key;
        self.archive_order
            .can_stage(key)
            .map_err(|error| fail(&error))?;
        lane.stage(writer.clone(), checkpoint)
            .map_err(|error| fail(&error))?;
        self.archive_order
            .staged(key)
            .map_err(|error| fail(&error))?;
        Ok(())
    }

    fn finish_archive_write(&mut self, key: CheckpointKey, result: Result<(), String>) {
        if self.archive_order.accept_receipt(key) != ReceiptDisposition::Accepted {
            return;
        }
        if let Err(error) = result {
            let mut failure = HostFailure::archive(&error);
            failure.with_archive_key(key);
            self.stop_after_host_failure(failure);
        }
    }

    fn queue_quick_archive_read(
        &self,
    ) -> Result<oneshot::Receiver<Result<Option<SaveSlot>, StoreError>>, StoreError> {
        let (_, database) = self
            .archive
            .as_ref()
            .ok_or_else(|| StoreError::Invalid("桌面存档数据库未初始化".into()))?;
        let lane = self
            .archive_lane
            .as_ref()
            .ok_or_else(|| StoreError::Invalid("桌面归档任务未初始化".into()))?;
        let database = database.clone();
        lane.try_execute(move || load_selected_archive(&database))
    }

    fn drive_cycle(&mut self, permit: Permit) {
        let preferences = self.pause_preferences;
        let result = run_cycle(self, permit, preferences);
        match apply_cycle_pacing(
            &mut self.pacing,
            self.game.tick(),
            native_async::now(),
            result,
        ) {
            Ok(()) => {}
            Err(CycleError::Advance {
                source,
                unpublished,
            }) => {
                let mut failure = HostFailure::civil(source);
                if let Some(range) = unpublished {
                    range.describe(&mut failure.message);
                }
                self.stop_after_host_failure(failure);
            }
            Err(CycleError::Batch {
                source,
                unpublished,
            }) => {
                let mut failure = HostFailure::step(source);
                unpublished.describe(&mut failure.message);
                self.stop_after_host_failure(failure);
            }
            Err(CycleError::Civil { source, retryable }) => {
                let error = source.into_error();
                self.stop_after_host_failure(if retryable {
                    HostFailure::correction_prepare_rejected(&error)
                } else {
                    HostFailure::civil(error)
                });
            }
            Err(CycleError::Stage(failure)) => self.stop_after_host_failure(*failure),
        }
    }

    // 测试显式提供受控 permit，不另有一份调度算法。
    #[cfg(test)]
    fn run_cycle(&mut self, limit: usize) {
        self.drive_cycle(Permit::for_explicit_limit(limit));
    }

    #[cfg(test)]
    async fn tick_and_emit(&mut self) {
        self.drive_cycle(Permit::single_step());
    }

    #[cfg(test)]
    async fn run_fastest_batch(&mut self) {
        if let Some(permit) = self.pacing.cadence.begin_turn(native_async::now()) {
            self.drive_cycle(permit);
        }
    }

    fn emit_update(&mut self, update: EngineUpdate) {
        self.delivery.stage(Packet::Update(EngineEventPayload {
            session_id: self.session_id.clone(),
            timeline_id: self.timeline_id.clone(),
            update,
        }));
    }

    fn finish_company_write<T>(
        &mut self,
        result: Result<T, CompanyWriteHostError>,
    ) -> Result<T, CompanyWriteHostError> {
        // generation 拒绝先于领域命令，也不能因当前 Session 已 poison 而改写其结果。
        if matches!(&result, Err(CompanyWriteHostError::Session { .. })) {
            return result;
        }
        let fatal = self
            .game
            .require_healthy()
            .err()
            .or_else(|| match result.as_ref().err() {
                Some(CompanyWriteHostError::Command(error)) => error.step_fatal().cloned(),
                _ => None,
            });
        if let Some(fatal) = fatal {
            if let Err(error) = &result {
                eprintln!(
                    "[session {}] 公司写命令触发引擎致命错误：{error:?}",
                    self.session_id
                );
            }
            self.stop_after_step_failure(fatal.clone());
            return Err(CompanyWriteHostError::Fatal(fatal));
        }
        result
    }

    /// 处理单条命令。
    async fn handle_command(&mut self, cmd: SessionCommand) {
        if self.cmd_rx.is_closed() {
            return;
        }
        match cmd {
            SessionCommand::SubscribeRightsOffering {
                generation,
                event_id,
                requested_shares,
                reply,
            } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(|error| CompanyWriteHostError::Session {
                        message: error.to_string(),
                    })
                    .and_then(|_| {
                        self.game
                            .require_healthy()
                            .map_err(CompanyWriteHostError::Fatal)
                    })
                    .and_then(|_| {
                        self.game
                            .subscribe_rights_offering(AccountId(0), &event_id, requested_shares)
                            .map(|receipt| RightsSubscriptionResponse {
                                generation: self.generation.to_string(),
                                receipt,
                            })
                            .map_err(CompanyWriteHostError::Command)
                    });
                let result = self.finish_company_write(result);
                let _ = reply.send(result);
            }
            SessionCommand::ProposePlayerCompanyAction {
                generation,
                proposal,
                reply,
            } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(|error| CompanyWriteHostError::Session {
                        message: error.to_string(),
                    })
                    .and_then(|_| {
                        self.game
                            .propose_player_company_action(AccountId(0), proposal)
                            .map(|receipt| CompanyProposalResponse {
                                generation: self.generation.to_string(),
                                receipt,
                            })
                            .map_err(|error| CompanyWriteHostError::Command(error.into()))
                    });
                let result = self.finish_company_write(result);
                let _ = reply.send(result);
            }
            SessionCommand::EnqueueReportCorrection {
                generation,
                request,
                reply,
            } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(|error| CompanyWriteHostError::Session {
                        message: error.to_string(),
                    })
                    .and_then(|_| {
                        let operation_id = request.operation_id.clone();
                        let epoch = self.game.game().report_correction_epoch();
                        self.game
                            .enqueue_report_correction(&epoch, request)
                            .map(|_| ReportCorrectionResponse {
                                generation: self.generation.to_string(),
                                operation_id,
                            })
                            .map_err(|error| CompanyWriteHostError::Command(error.into()))
                    });
                let result = self.finish_company_write(result);
                let _ = reply.send(result);
            }
            SessionCommand::CancelReportCorrection {
                generation,
                operation_id,
                reply,
            } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(|error| CompanyWriteHostError::Session {
                        message: error.to_string(),
                    })
                    .and_then(|_| {
                        let epoch = self.game.game().report_correction_epoch();
                        self.game
                            .cancel_report_correction(&epoch, &operation_id)
                            .map(|_| ReportCorrectionResponse {
                                generation: self.generation.to_string(),
                                operation_id,
                            })
                            .map_err(|error| CompanyWriteHostError::Command(error.into()))
                    });
                let result = self.finish_company_write(result);
                let _ = reply.send(result);
            }
            SessionCommand::CompanyRead {
                generation,
                query,
                reply,
            } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(|error| CompanyReadHostError::Session {
                        message: error.to_string(),
                    })
                    .and_then(|_| {
                        let owner = AccountId(0);
                        self.game
                            .company_read_query(owner, query)
                            .map(|value| CompanyReadResponse {
                                generation: self.generation.to_string(),
                                owner,
                                value,
                            })
                            .map_err(CompanyReadHostError::Query)
                    });
                let _ = reply.send(result);
            }
            SessionCommand::Enqueue {
                player_id,
                intent,
                reply,
            } => {
                let res = self.game.enqueue_player_intent(player_id, intent);
                // reply 失败仅说明调用方已放弃等待——不属错误。
                let _ = reply.send(res);
            }
            SessionCommand::Snapshot { reply } => {
                let snap = self.game.snapshot();
                let _ = reply.send(snap);
            }
            SessionCommand::PlayerWorkingOrders { generation, reply } => {
                let orders =
                    serde_json::to_value(self.game.player_working_orders()).map_err(|_| {
                        // 1312：玩家在途委托序列化失败（边界日志承载细节）。
                        SessionError::InvalidSave(engine::error::ValidationFault::bare(1312))
                    });
                let result = self.generation_response(generation, orders);
                let _ = reply.send(result);
            }
            SessionCommand::HostHistoryQuery {
                generation,
                query,
                reply,
            } => {
                let result = (|| -> Result<GenerationResponse<serde_json::Value>, String> {
                    if generation != self.generation {
                        return Err("STALE_SESSION_GENERATION".into());
                    }
                    self.game.require_healthy().map_err(|e| e.to_string())?;
                    let value = match query {
                        HistoryQuery::MarketHistory(query) => serde_json::to_value(
                            self.game
                                .query_market_history_for(AccountId(0), &query)
                                .map_err(|e| e.to_string())?,
                        ),
                        HistoryQuery::CurrentMinuteHistory(query) => serde_json::to_value(
                            self.game
                                .query_current_minute_history_for(AccountId(0), &query)
                                .map_err(|e| e.to_string())?,
                        ),
                        HistoryQuery::PersonalTradeHistory(query) => serde_json::to_value(
                            self.game
                                .query_personal_trade_history(AccountId(0), query)
                                .map_err(|e| e.to_string())?,
                        ),
                        HistoryQuery::PersonalTradeConfirmations(query) => {
                            let cursor = query.before_receipt;
                            let cursor = cursor
                                .map(|s| {
                                    if s.is_empty()
                                        || (s.len() > 1 && s.starts_with('0'))
                                        || !s.bytes().all(|b| b.is_ascii_digit())
                                    {
                                        return Err("交割单游标无效".to_owned());
                                    }
                                    s.parse::<u64>().map_err(|e| e.to_string())
                                })
                                .transpose()?;
                            serde_json::to_value(
                                self.game
                                    .personal_trade_confirmations_page(AccountId(0), cursor),
                            )
                        }
                    }
                    .map_err(|e| e.to_string())?;
                    Ok(GenerationResponse {
                        generation: generation.to_string(),
                        value,
                    })
                })();
                let _ = reply.send(result);
            }
            SessionCommand::StockHistory {
                generation,
                code,
                reply,
            } => {
                let result = if generation != self.generation {
                    self.generation_response(
                        generation,
                        Err(SessionError::InvalidSave(
                            engine::error::ValidationFault::new(
                                1310,
                                vec![generation, self.generation],
                            ),
                        )),
                    )
                } else {
                    let queried = self.game.query_stock_history(AccountId(0), &code);
                    self.generation_response(generation, queried)
                };
                let _ = reply.send(result);
            }
            SessionCommand::InitialAllocation { generation, reply } => {
                let result = self.generation_response(generation, self.game.initial_allocation());
                let _ = reply.send(result);
            }
            SessionCommand::QueryBaseline { generation, reply } => {
                let result = self
                    .generation_response(generation, Ok(()))
                    .map_err(BaselineError::Session)
                    .and_then(|_| {
                        self.baseline_candidate("query_baseline", "desktop.actor.query_baseline")
                    });
                let _ = reply.send(result);
            }
            SessionCommand::ResumeObserver {
                generation,
                ordinal,
                reply,
            } => {
                if generation != self.generation {
                    let _ = reply.send(Err("STALE_SESSION_GENERATION".into()));
                } else {
                    self.delivery.resume_after(ordinal, reply);
                }
            }
            SessionCommand::RuntimeSnapshot { reply } => {
                let snap = self.game.runtime_snapshot();
                let _ = reply.send(snap);
            }
            SessionCommand::CivilDate { generation, reply } => {
                let _ =
                    reply.send(self.generation_response(generation, Ok(self.game.civil_date())));
            }
            SessionCommand::PublicReports {
                generation,
                query,
                reply,
            } => {
                let _ = reply.send(
                    self.generation_response(generation, self.game.query_public_reports(&query)),
                );
            }
            SessionCommand::PublicReportById {
                generation,
                id,
                reply,
            } => {
                let _ = reply
                    .send(self.generation_response(generation, self.game.public_report_by_id(id)));
            }
            SessionCommand::NpcDecisionDiagnostics {
                generation,
                account,
                reply,
            } => {
                let _ = reply.send(diagnostic_generation_response(
                    generation,
                    self.generation,
                    || self.game.npc_decision_diagnostics(account),
                ));
            }
            SessionCommand::SpeedMetrics { reply } => {
                let metrics = self
                    .pacing
                    .refresh_metrics(self.game.tick(), native_async::now());
                let _ = reply.send(metrics);
            }
            SessionCommand::Save {
                generation,
                candidate,
                reply,
            } => {
                let result = self.generation_response(generation, Ok(())).and_then(|_| {
                    if let Some(key) = candidate.as_ref() {
                        self.game.save_candidate(key)
                    } else {
                        self.game.save()
                    }
                });
                let _ = reply.send(result);
            }
            SessionCommand::Restore {
                generation,
                slot,
                reply,
            } => match self.restore(generation, slot).await {
                Ok(restored) => {
                    let _ = reply.send(Ok(restored));
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            #[cfg(feature = "host-parity")]
            SessionCommand::AdvanceCivilDay { generation, reply } => {
                use engine::session::runner::manual::{manual_civil, ManualCivilError};

                let result = self.generation_response(generation, Ok(())).and_then(|_| {
                    match manual_civil(&mut self.game, self.pause_preferences) {
                        Ok((update, paused)) => {
                            if paused {
                                self.pacing
                                    .pause_at_civil_boundary(self.game.tick(), native_async::now());
                            }
                            if let Err(failure) = self.stage_civil(update.clone(), paused) {
                                self.stop_after_host_failure(*failure);
                            }
                            Ok(update)
                        }
                        Err(ManualCivilError::Rejected(error)) => Err(error),
                        Err(ManualCivilError::Retryable(error)) => {
                            self.stop_after_host_failure(HostFailure::correction_prepare_rejected(
                                &error,
                            ));
                            Err(error)
                        }
                        Err(ManualCivilError::Fatal(error)) => {
                            self.stop_after_host_failure(HostFailure::civil_ref(&error));
                            Err(error)
                        }
                    }
                });
                let _ = reply.send(result);
            }
            #[cfg(feature = "host-parity")]
            SessionCommand::Step { generation, reply } => {
                use engine::session::runner::manual::{manual_tick, ManualStepError};

                let result = self.generation_response(generation, Ok(())).and_then(|_| {
                    manual_tick(&mut self.game, self.pacing.is_running())
                        .map(|batch| EngineUpdate::TickBatch(Box::new(batch)))
                        .map_err(ManualStepError::into_session_error)
                });
                if let Ok(update) = &result {
                    self.emit_update(update.clone());
                }
                if let Err(SessionError::Step(fatal)) = &result {
                    self.stop_after_step_failure(fatal.clone());
                }
                let _ = reply.send(result);
            }
            SessionCommand::SetSpeed { speed, reply } => {
                let result = self.apply_speed(speed);
                let _ = reply.send(result);
            }
            SessionCommand::SetRunning { running, reply } => {
                self.pacing
                    .set_running(running, self.game.tick(), native_async::now());
                let _ = reply.send(());
            }
            SessionCommand::SetPausePreferences { preferences, reply } => {
                self.pause_preferences = preferences;
                let _ = reply.send(());
            }
            SessionCommand::ArchiveWriteCompleted { key, result } => {
                self.finish_archive_write(key, result);
            }
            SessionCommand::ReadQuickArchive { reply } => match self.queue_quick_archive_read() {
                Ok(result) => {
                    native_async::spawn(async move {
                        let result = result.await.unwrap_or_else(|_| {
                            Err(StoreError::Invalid("归档任务没有返回读档结果".into()))
                        });
                        let _ = reply.send(result);
                    });
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            SessionCommand::Shutdown { reply } => {
                if let Err(error) = self.game.shared_ingress().close() {
                    self.stop_after_host_failure(failure::HostFailure::civil(error));
                }
                self.finish_archive_lane().await;
                let _ = reply.send(());
            }
        }
    }

    /// 应用新倍速：固定倍率使用精确 Duration；Fastest 切到 CPU 时间片 tight-loop。
    fn apply_speed(&mut self, speed: f64) -> Result<(), SessionError> {
        if self
            .pacing
            .apply_speed(speed, self.game.tick(), native_async::now())
            .is_err()
        {
            // 1311：speed 档位非法；ctx[0]=speed 位模式（f64）。
            return Err(SessionError::InvalidSave(
                engine::error::ValidationFault::new(1311, vec![speed.to_bits()]),
            ));
        }
        Ok(())
    }

    fn generation_response<T>(
        &self,
        generation: u64,
        value: Result<T, SessionError>,
    ) -> Result<GenerationResponse<T>, SessionError> {
        if generation != self.generation {
            // 1310：世代过期。
            return Err(SessionError::InvalidSave(
                engine::error::ValidationFault::new(1310, vec![generation, self.generation]),
            ));
        }
        value.map(|value| GenerationResponse {
            generation: generation.to_string(),
            value,
        })
    }

    async fn restore(
        &mut self,
        generation: u64,
        slot: Box<SaveSlot>,
    ) -> Result<RestoreResult, BaselineError> {
        self.generation_response(generation, Ok(()))?;
        let restored = ProtocolSession::restore(&slot)?;
        let next_generation = self.generation.checked_add(1).ok_or_else(|| {
            // 1313：世代计数耗尽。
            SessionError::InvalidSave(engine::error::ValidationFault::bare(1313))
        })?;
        let ingress = self.ingress.clone();
        let new_ingress = restored.shared_ingress();
        let install_ingress = move || -> Result<(), StoreError> {
            let mut ingress = ingress.write().map_err(|_| {
                // 1314：ingress 生命周期锁中毒。
                SessionError::ResourceLimit(engine::error::ValidationFault::bare(1314))
            })?;
            ingress.1.close()?;
            *ingress = (next_generation, new_ingress);
            Ok(())
        };
        let next_epoch = self
            .archive_epoch
            .checked_add(1)
            .ok_or_else(|| StoreError::Invalid("桌面归档 writer epoch 已耗尽".into()))?;
        if let Some((writer, _)) = self.archive.as_ref() {
            let writer = writer.clone();
            let lane = self
                .archive_lane
                .as_ref()
                .ok_or_else(|| StoreError::Invalid("桌面归档任务未初始化".into()))?;
            let renewed = lane
                .execute(move || writer.renew_after(None, install_ingress))
                .await?;
            if let Some((writer, _)) = self.archive.as_mut() {
                *writer = renewed;
            }
        } else {
            install_ingress()?;
        }
        self.archive_epoch = next_epoch;
        self.archive_order.install(next_generation, next_epoch);
        self.game = restored;
        self.game.prepare_public_baseline();
        self.timeline_id = uuid::Uuid::new_v4().to_string();
        self.generation = next_generation;
        self.delivery.replace(
            self.session_id.clone(),
            self.timeline_id.clone(),
            next_generation,
        );
        self.pacing
            .reset_after_restore(self.game.tick(), native_async::now());
        self.baseline_candidate("restore", "desktop.actor.restore")
    }

    fn baseline_candidate(
        &mut self,
        operation: &'static str,
        location: &'static str,
    ) -> Result<RestoreResult, BaselineError> {
        let catalog = self.game.company_catalog_view();
        self.finish_catalog_baseline(catalog, operation, location)
    }

    fn finish_catalog_baseline(
        &mut self,
        catalog: Result<
            Vec<engine::session::CompanyCatalogEntry>,
            engine::session::CompanyCatalogViewError,
        >,
        operation: &'static str,
        location: &'static str,
    ) -> Result<RestoreResult, BaselineError> {
        let company_catalog = match catalog {
            Ok(catalog) => catalog,
            Err(error) => {
                self.stop_after_host_failure(failure::HostFailure::catalog(
                    &error, operation, location,
                ));
                return Err(BaselineError::Catalog(error));
            }
        };
        // 观察者恢复不得重置其他消费者的公开 delta 基线或丢弃待写存档。
        let (snapshot, working_orders) = self.game.observer_baseline();
        let publication_ordinal = self.delivery.offer_baseline_fence().to_string();
        Ok(RestoreResult {
            snapshot,
            working_orders,
            publication_ordinal,
            latest_checkpoint: self.game.latest_day_end_key(),
            stock_catalog: self.game.stock_catalog_view(),
            company_catalog,
            civil_date: self.game.civil_date(),
            timeline_id: self.timeline_id.clone(),
            generation: self.generation.to_string(),
        })
    }
}

impl<R: Runtime> DrivePort for SessionActor<R> {
    type Error = Box<HostFailure>;

    fn session(&self) -> &ProtocolSession {
        &self.game
    }

    fn session_mut(&mut self) -> &mut ProtocolSession {
        &mut self.game
    }

    fn command_pending(&self) -> bool {
        self.cmd_rx.is_closed() || !self.cmd_rx.is_empty()
    }

    fn step_frame(&mut self) -> Result<TickFrame, engine::session::StepFatal> {
        self.step_game()
    }

    fn stage_tick_batch(&mut self, batch: TickBatch) -> Result<(), Self::Error> {
        self.emit_update(EngineUpdate::TickBatch(Box::new(batch)));
        Ok(())
    }

    fn stage_civil(
        &mut self,
        update: CivilUpdate,
        _barrier_paused: bool,
    ) -> Result<(), Self::Error> {
        let archive = self.stage_day_end(&update);
        self.emit_update(EngineUpdate::CivilUpdate(Box::new(update)));
        archive
    }

    fn now(&self) -> libos::clock::Instant {
        native_async::now()
    }

    #[cfg(test)]
    fn enforce_budget(&self) -> bool {
        self.injected_step_failure.is_none()
    }
}

#[cfg(test)]
fn take_publish_batch<T>(
    pending: &mut Vec<T>,
    incoming: Vec<T>,
    elapsed: Duration,
) -> Option<Vec<T>> {
    pending.extend(incoming);
    (elapsed >= UI_PUBLISH_INTERVAL && !pending.is_empty()).then(|| std::mem::take(pending))
}

#[cfg(test)]
fn compact_fastest_events(
    events: Vec<engine::Event>,
    ticks_per_day: u64,
    auction_ticks: u64,
) -> Vec<engine::Event> {
    if events.is_empty() {
        return events;
    }
    let mut keep = HashSet::new();
    let mut trade_indices = Vec::new();
    let mut active_minute_ticks: HashMap<(engine::StockId, u64), usize> = HashMap::new();
    let mut auction_slot_ticks: HashMap<(engine::StockId, u64), usize> = HashMap::new();
    let safe_ticks_per_day = ticks_per_day.max(1);

    for (index, event) in events.iter().enumerate() {
        match event {
            engine::Event::DayBoundary { .. } => {
                keep.insert(index);
                active_minute_ticks.clear();
                auction_slot_ticks.clear();
            }
            engine::Event::AuctionTick { tick, code, .. } => {
                let day_tick = (tick.saturating_sub(1)) % safe_ticks_per_day;
                auction_slot_ticks.insert((*code, day_tick / 6), index);
            }
            engine::Event::PriceTick { tick, code, .. } => {
                let day_tick = (tick.saturating_sub(1)) % safe_ticks_per_day;
                active_minute_ticks
                    .insert((*code, day_tick.saturating_sub(auction_ticks) / 60), index);
            }
            engine::Event::Trade { .. } => trade_indices.push(index),
            _ => {
                keep.insert(index);
            }
        }
    }
    for index in trade_indices.into_iter().rev().take(100) {
        keep.insert(index);
    }
    keep.extend(active_minute_ticks.into_values());
    keep.extend(auction_slot_ticks.into_values());

    events
        .into_iter()
        .enumerate()
        .filter_map(|(index, event)| keep.contains(&index).then_some(event))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        compact_fastest_events, diagnostic_generation_response, fixed_tick_interval,
        take_publish_batch, SessionManager,
    };
    use engine::{
        AccountId, DailyCandle, DailyTradeStats, Event, FloatAllocation, GameConfig, HotParams,
        InstParams, Money, NpcDecisionDiagnostics, NpcSetup, RetailParams, SecurityCategory,
        SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
    };
    use libos::native_async;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;

    pub(super) fn diagnostic_setup() -> SessionSetup {
        SessionSetup {
            company_system: None,
            dividend_tax_mode: None,
            flat_withholding_bp: None,
            rights_offering_enabled: false,
            issuer_repurchase_enabled: false,
            par_value_per_share: None,
            auto_corporate_foundation: false,
            report_frequency: None,
            company_operations: None,
            groups: Vec::new(),
            stocks: vec![StockSpec {
                code: StockCode("600101".to_owned()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 100_000,
                float_shares: 100_000,
            }],
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 1,
                hot_count: 0,
                retail_cash_median: Money::from_cents(10_000_000),
            },
            config: GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.8,
                    order_size_mean: 200,
                    chase_prob: 0.4,
                },
                inst: InstParams {
                    margin: 0.02,
                    order_size: 500,
                },
                hot: HotParams {
                    lookback: 3,
                    trend_threshold: 0.01,
                    order_size: 300,
                },
            },
            ticks_per_day: 30,
            auction_ticks: 0,
            closing_auction_ticks: 0,
            history_len: 20,
            t1_enabled: true,
            float_allocation: FloatAllocation::random(),
            start_date: engine::CivilDate::from_iso("2030-01-01").unwrap(),
            simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
            runtime_sync_policy: Default::default(),
        }
    }

    #[test]
    fn stale_diagnostics_generation_does_not_invoke_private_reader() {
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&reads);

        let result = diagnostic_generation_response(0, 1, move || {
            counter.fetch_add(1, Ordering::SeqCst);
            NpcDecisionDiagnostics::Unsupported
        });

        assert!(result.is_err());
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn desktop_committed_day_is_durable_without_frontend_observation() {
        let app = tauri::test::mock_app();
        let database = native_store::NativeDatabase::open_in_memory().unwrap();
        let manager = SessionManager::default();
        manager.initialize_archive(database.clone()).unwrap();
        let mut setup = diagnostic_setup();
        let day = engine::CivilDate::from_iso("2030-01-02").unwrap();
        setup.start_date = day;
        let id = manager
            .new_session(setup, 7, app.handle().clone())
            .await
            .unwrap();
        let desktop = manager.lookup(&id).await.unwrap();
        desktop
            .set_pause_preferences(engine::session::protocol::PausePreferences {
                pause_after_close: true,
                pause_before_open: false,
            })
            .await
            .unwrap();
        desktop.set_speed(f64::INFINITY).await.unwrap();
        desktop.set_running(true).await.unwrap();
        // 无事件监听者/前端 ACK，只从独立归档 FIFO 读取真正已提交的槽。
        let first = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(saved) = desktop.read_quick_archive().await.unwrap() {
                    break saved;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("桌面归档未提交");
        assert_eq!(first.civil_clock.settled_through, Some(day));
        assert!(first
            .snapshot
            .daily_candles
            .values()
            .flatten()
            .any(|candle| candle.trade_stats.is_some()));
        assert!(first.market_minute_bars.values().all(Vec::is_empty));

        assert_eq!(
            desktop.restore(1, first.clone()).await.unwrap().generation,
            "2"
        );
        manager.remove(&id).await.unwrap().shutdown().await.unwrap();
        let startup = manager.read_startup_archive().await.unwrap().unwrap();
        assert_eq!(startup.snapshot.seq, first.snapshot.seq);
        assert_eq!(startup.snapshot.daily_candles, first.snapshot.daily_candles);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn desktop_storage_read_does_not_hold_authority() {
        let app = tauri::test::mock_app();
        let database = native_store::NativeDatabase::open_in_memory().unwrap();
        let manager = SessionManager::default();
        manager.initialize_archive(database.clone()).unwrap();
        let id = manager
            .new_session(diagnostic_setup(), 7, app.handle().clone())
            .await
            .unwrap();
        let desktop = manager.lookup(&id).await.unwrap();
        let (entered, locked) = libos::native_async::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let busy = native_async::spawn_blocking(move || {
            database.with_connection(|_| {
                let _ = entered.send(());
                let _ = blocked.recv_timeout(Duration::from_secs(2));
                Ok(())
            })
        });
        locked.await.unwrap();
        // 先向同一个 authority FIFO 发读档，再验证后续命令无需等待 SQLite。
        let (reply, mut received) = libos::native_async::oneshot::channel();
        desktop
            .cmd_tx
            .send(super::SessionCommand::ReadQuickArchive { reply })
            .ok()
            .unwrap();
        tokio::time::timeout(Duration::from_millis(500), desktop.speed_metrics())
            .await
            .expect("SQLite 查询阻塞了市场任务")
            .unwrap();
        assert!(matches!(
            received.try_recv(),
            Err(libos::native_async::oneshot::error::TryRecvError::Empty)
        ));
        release.send(()).unwrap();
        busy.await.unwrap().unwrap();
        assert!(received.await.unwrap().unwrap().is_none());
        manager.remove(&id).await.unwrap().shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn diagnostics_reject_stale_generation_without_private_records() {
        let app = tauri::test::mock_app();
        let manager = SessionManager::default();
        let session_id = manager
            .new_session(diagnostic_setup(), 7, app.handle().clone())
            .await
            .unwrap();
        let handles = manager.lookup(&session_id).await.unwrap();

        let result = handles.npc_decision_diagnostics(0, AccountId(1)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn initial_allocation_queries_actual_npc_holdings_before_start() {
        let app = tauri::test::mock_app();
        let manager = SessionManager::default();
        let session_id = manager
            .new_session(diagnostic_setup(), 7, app.handle().clone())
            .await
            .unwrap();
        let handles = manager.lookup(&session_id).await.unwrap();
        assert!(handles.initial_allocation(0).await.is_err());
        let allocation = handles.initial_allocation(1).await.unwrap();
        assert_eq!(allocation.generation, "1");
        assert_eq!(allocation.value.stocks[0].categories[1].shares, 100_000);
        assert_eq!(allocation.value.stocks[0].categories[1].zero_holders, 0);
        assert_eq!(allocation.value.stocks[0].unallocated_shares, 0);
        assert_eq!(
            handles.initial_allocation(1).await.unwrap().value,
            allocation.value
        );
        handles.shutdown().await.unwrap();
        manager.remove(&session_id).await.unwrap();
    }

    #[tokio::test]
    #[cfg(not(feature = "simulation-diagnostics"))]
    async fn release_diagnostics_returns_unsupported_without_records() {
        let app = tauri::test::mock_app();
        let manager = SessionManager::default();
        let session_id = manager
            .new_session(diagnostic_setup(), 7, app.handle().clone())
            .await
            .unwrap();
        let handles = manager.lookup(&session_id).await.unwrap();

        let response = handles
            .npc_decision_diagnostics(1, AccountId(1))
            .await
            .unwrap();

        assert_eq!(response.value, NpcDecisionDiagnostics::Unsupported);
        assert!(serde_json::to_value(response.value)
            .unwrap()
            .get("records")
            .is_none());
    }

    #[tokio::test]
    #[cfg(feature = "simulation-diagnostics")]
    async fn feature_diagnostics_returns_supported_records_for_current_generation() {
        let app = tauri::test::mock_app();
        let manager = SessionManager::default();
        let session_id = manager
            .new_session(diagnostic_setup(), 7, app.handle().clone())
            .await
            .unwrap();
        let handles = manager.lookup(&session_id).await.unwrap();

        let response = handles
            .npc_decision_diagnostics(1, AccountId(1))
            .await
            .unwrap();

        assert!(
            matches!(response.value, NpcDecisionDiagnostics::Supported { records } if records.is_empty())
        );
    }

    #[test]
    fn desktop_wake_plan_uses_market_period_not_ipc_publish_interval() {
        use engine::session::runner::WakePlan;
        let mut pacing = super::DesktopPacing::new(1_000, 0, native_async::now());
        assert!(matches!(pacing.cadence.wake_plan(), WakePlan::Idle));
        pacing.apply_speed(180.0, 0, native_async::now()).unwrap();
        let before = libos::native_async::now();
        pacing.set_running(true, 0, native_async::now());
        let after = libos::native_async::now();
        let WakePlan::At(deadline) = pacing.cadence.wake_plan() else {
            panic!("fixed pace 须给出下次市场 deadline");
        };
        let period = fixed_tick_interval(1000, 180.0);
        assert!(deadline >= before + period && deadline <= after + period);
        pacing
            .apply_speed(f64::INFINITY, 0, native_async::now())
            .unwrap();
        assert!(matches!(pacing.cadence.wake_plan(), WakePlan::Immediate));
    }

    #[test]
    fn fixed_speed_intervals_keep_fractional_milliseconds() {
        assert_eq!(fixed_tick_interval(1000, 60.0).as_nanos(), 16_666_667);
        assert_eq!(fixed_tick_interval(1000, 180.0).as_nanos(), 5_555_556);
        assert_eq!(fixed_tick_interval(1000, 720.0).as_nanos(), 1_388_889);
    }

    #[test]
    fn fixed_high_speed_ticks_cross_ipc_as_one_sixteen_ms_batch() {
        let mut pending = Vec::new();
        for tick in 1..=11 {
            assert_eq!(
                take_publish_batch(&mut pending, vec![tick], Duration::from_millis(15)),
                None
            );
        }
        assert_eq!(
            take_publish_batch(&mut pending, vec![12], Duration::from_millis(16)),
            Some((1..=12).collect())
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn fastest_compaction_keeps_boundaries_and_latest_tick_per_minute() {
        let price_tick = |seq, tick| Event::PriceTick {
            seq,
            tick,
            code: engine::StockId(0),
            last_price: Money::from_cents(1_000_000_000),
            daily_candle: DailyCandle {
                time: 0,
                open: Money::from_cents(1_000_000_000),
                high: Money::from_cents(1_000_000_000),
                low: Money::from_cents(1_000_000_000),
                close: Money::from_cents(1_000_000_000),
                volume: 9_007_200,
                trade_stats: Some(DailyTradeStats {
                    turnover_cents: 9_007_200_000_000_000,
                    trade_count: 7,
                }),
            },
            bids: Vec::new(),
            asks: Vec::new(),
        };
        let boundary = Event::DayBoundary {
            seq: 3,
            day: 1,
            closed_daily_candles: Default::default(),
        };
        let compacted = compact_fastest_events(
            vec![
                price_tick(1, 1),
                price_tick(2, 59),
                boundary,
                price_tick(4, 61),
            ],
            14_400,
            0,
        );
        assert_eq!(compacted.len(), 2);
        assert!(matches!(compacted[0], Event::DayBoundary { seq: 3, .. }));
        let Event::PriceTick { daily_candle, .. } = &compacted[1] else {
            panic!("最后一个压缩事件应为 PriceTick");
        };
        let stats = daily_candle
            .trade_stats
            .as_ref()
            .expect("桌面 IPC 压缩必须保留累计成交统计");
        assert_eq!(stats.turnover_cents, 9_007_200_000_000_000);
        assert_eq!(stats.trade_count, 7);
        let json = serde_json::to_value(&compacted[1]).unwrap();
        assert_eq!(
            json["PriceTick"]["daily_candle"]["trade_stats"]["turnover_cents"],
            "9007200000000000"
        );
    }

    #[test]
    fn fastest_compaction_keeps_six_second_auction_slots_and_completion() {
        let auction_tick = |seq, tick| Event::AuctionTick {
            seq,
            tick,
            phase: engine::TradingPhase::CallAuction,
            code: engine::StockId(0),
            indicative_price: Some(Money::from_cents(100)),
            matched_volume: seq,
            imbalance: 0,
        };
        let completed = Event::AuctionCompleted {
            seq: 4,
            tick: 900,
            phase: engine::TradingPhase::CallAuction,
            code: engine::StockId(0),
            clearing_price: Some(Money::from_cents(100)),
            matched_volume: 3,
        };
        let compacted = compact_fastest_events(
            vec![
                auction_tick(1, 1),
                auction_tick(2, 6),
                auction_tick(3, 7),
                completed,
            ],
            15_300,
            900,
        );
        assert_eq!(compacted.len(), 3);
        assert!(matches!(compacted[0], Event::AuctionTick { seq: 2, .. }));
        assert!(matches!(compacted[1], Event::AuctionTick { seq: 3, .. }));
        assert!(matches!(
            compacted[2],
            Event::AuctionCompleted { seq: 4, .. }
        ));
    }
}
