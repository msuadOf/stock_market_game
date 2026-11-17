//! Tauri IPC 只是公共观察流的传输出口；市场 actor 不在这里编码或发送。
use super::failure::EngineFailurePayload;
use crate::{EngineEventPayload, ENGINE_EVENT_NAME};
use engine::session::publication::lifecycle::{ObserverLease, ObserverLifecycle, TerminalDelivery};
use engine::session::publication::{self, Observer, Producer, PumpRead, Read};
use libos::{native_async, wake_epoch::WakeEpoch};
use serde::Serialize;
use std::{
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock, RwLock,
    },
};
use tauri::{AppHandle, Emitter, Runtime};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

const CONTROL_CAPACITY: usize = 8;

pub(super) enum Packet {
    Update(EngineEventPayload),
    Failure(EngineFailurePayload),
}

struct Terminal {
    after: u64,
    failure: EngineFailurePayload,
}

#[derive(Clone, Serialize)]
struct GapNotice<'a> {
    session_id: &'a str,
    timeline_id: &'a str,
    gap: GapCursor,
    observer_lease: String,
}

#[derive(Clone, Serialize)]
struct GapCursor {
    through: String,
}

#[derive(Clone, Serialize)]
struct ObservedUpdate {
    observer_lease: String,
    #[serde(flatten)]
    payload: EngineEventPayload,
    publication_ordinal: String,
}

struct ResumeRequest {
    lease: u64,
    ordinal: u64,
    reply: oneshot::Sender<Result<(), String>>,
}

struct AckRequest {
    lease: u64,
    ordinal: u64,
    reply: oneshot::Sender<Result<(), String>>,
}

enum Control {
    #[cfg(any(test, feature = "host-parity"))]
    Inspect(oneshot::Sender<publication::lifecycle::ObserverEvidence>),
    Resume(ResumeRequest),
    Ack(AckRequest),
    Hold {
        lease: u64,
        held: bool,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

struct AckEndpoint {
    lease: Arc<AtomicU64>,
    generation: u64,
    sender: mpsc::Sender<Control>,
    signal: Arc<WakeEpoch>,
}

/// 观察 ACK 走独立端口，不经过市场命令队列。
#[derive(Clone, Default)]
pub(super) struct AckRoute(Arc<RwLock<Option<AckEndpoint>>>);

impl AckRoute {
    pub(super) async fn acknowledge(
        &self,
        generation: u64,
        lease: u64,
        ordinal: u64,
    ) -> Result<(), String> {
        self.request(generation, |reply| {
            Control::Ack(AckRequest {
                lease,
                ordinal,
                reply,
            })
        })
        .await
    }

    pub(super) async fn set_held(
        &self,
        generation: u64,
        lease: u64,
        held: bool,
    ) -> Result<(), String> {
        self.request(generation, |reply| Control::Hold { lease, held, reply })
            .await
    }

    async fn request(
        &self,
        generation: u64,
        control: impl FnOnce(oneshot::Sender<Result<(), String>>) -> Control,
    ) -> Result<(), String> {
        let (reply, response) = oneshot::channel();
        {
            let inner = self.0.read().expect("观察者地址锁已中毒");
            let endpoint = inner.as_ref().ok_or("OBSERVER_CLOSED")?;
            if endpoint.generation != generation {
                return Err("STALE_SESSION_GENERATION".to_owned());
            }
            endpoint
                .sender
                .try_send(control(reply))
                .map_err(|_| "OBSERVER_CONTROL_UNAVAILABLE".to_owned())?;
            endpoint.signal.notify();
        }
        response.await.map_err(|_| "OBSERVER_CLOSED".to_owned())?
    }
}

enum DeliveryAction<T> {
    Item { ordinal: u64, value: T },
    Gap(u64),
    Terminal(EngineFailurePayload),
}

async fn consume<T, F>(
    observer: Observer<T>,
    generation: u64,
    lease_counter: Arc<AtomicU64>,
    final_evidence: Arc<std::sync::Mutex<Option<publication::lifecycle::ObserverEvidence>>>,
    mut send: F,
    signal: Arc<WakeEpoch>,
    mut controlled: mpsc::Receiver<Control>,
    window: NonZeroUsize,
    terminal_record: Arc<OnceLock<Terminal>>,
) where
    F: FnMut(DeliveryAction<T>, u64) -> bool,
{
    let mut announced_gap = false;
    let mut lifecycle = ObserverLifecycle::default();
    let initial = lifecycle.hold(generation);
    lease_counter.store(initial.lease, Ordering::Release);
    lifecycle.fence(initial, 0);
    lifecycle.release(initial);
    let mut sent_through = 0;
    let mut terminal_delivery = TerminalDelivery::default();
    let mut pump = publication::ObserverPump::new(observer, window);
    let mut received_control = None;
    loop {
        let observed = signal.load();
        for _ in 0..CONTROL_CAPACITY {
            let Some(control) = received_control
                .take()
                .or_else(|| controlled.try_recv().ok())
            else {
                break;
            };
            match control {
                #[cfg(any(test, feature = "host-parity"))]
                Control::Inspect(reply) => {
                    let _ = reply.send(lifecycle.snapshot());
                }
                Control::Resume(request) => {
                    let lease = ObserverLease {
                        generation,
                        lease: request.lease,
                    };
                    if !lifecycle.is_current(lease) {
                        let _ = request.reply.send(Err("STALE_OBSERVER_LEASE".into()));
                        continue;
                    }
                    let result = pump
                        .resume_after(request.ordinal)
                        .map_err(|error| match error {
                            publication::ResumeError::Stale { through } => {
                                format!("OBSERVER_BASELINE_STALE: {through}")
                            }
                            publication::ResumeError::Future { issued } => {
                                format!("OBSERVER_BASELINE_FUTURE: {issued}")
                            }
                        });
                    if result.is_ok() {
                        lifecycle.fence(lease, request.ordinal);
                        sent_through = request.ordinal;
                        announced_gap = false;
                    }
                    let _ = request.reply.send(result);
                }
                Control::Hold {
                    lease,
                    held: next,
                    reply,
                } => {
                    let expected = ObserverLease { generation, lease };
                    let accepted = if next {
                        lifecycle
                            .current()
                            .is_some_and(|old| old.lease.checked_add(1) == Some(lease))
                            && lifecycle.hold(generation) == expected
                    } else {
                        lifecycle.release(expected)
                    };
                    if accepted {
                        lease_counter.store(lease, Ordering::Release);
                    }
                    let _ = reply.send(if accepted {
                        Ok(())
                    } else {
                        Err("STALE_OBSERVER_LEASE".into())
                    });
                }
                Control::Ack(request) => {
                    let lease = ObserverLease {
                        generation,
                        lease: request.lease,
                    };
                    if !lifecycle.is_current(lease) {
                        let _ = request.reply.send(Err("STALE_OBSERVER_LEASE".into()));
                        continue;
                    }
                    let result = pump
                        .acknowledge(request.ordinal)
                        .map(|_| {
                            lifecycle.accepted(lease, request.ordinal);
                        })
                        .map_err(|error| format!("OBSERVER_ACK_INVALID: {error:?}"));
                    let _ = request.reply.send(result);
                }
            }
        }
        let producer_closed = signal.is_closed();
        let terminal = terminal_record.get();
        let emit_terminal =
            |send: &mut F, terminal: &Terminal, missing: Option<(u64, u64)>, lease: u64| {
                let mut failure = terminal.failure.clone();
                if let Some((from, through)) = missing {
                    failure.note_missing_publications(from, through);
                }
                // The sideband is never an acknowledgement-bearing mailbox packet.
                send(DeliveryAction::Terminal(failure), lease);
            };
        if let Some(terminal) = terminal {
            if let Some(missing) = terminal_delivery.take(
                pump.terminal_readiness(terminal.after),
                lifecycle.is_held(),
                sent_through,
                terminal.after,
            ) {
                emit_terminal(
                    &mut send,
                    terminal,
                    missing,
                    lifecycle.current().expect("terminal 需要 lease").lease,
                );
                break;
            }
        }
        if lifecycle.is_held() {
            if producer_closed {
                break;
            }
            signal.wait_for_change(observed).await;
            continue;
        }
        match pump.poll() {
            PumpRead::Ready(Read::Item { ordinal, value }) => {
                announced_gap = false;
                pump.record_sent(ordinal)
                    .expect("观察者只发送匹配当前窗口的连续帧");
                if !send(
                    DeliveryAction::Item { ordinal, value },
                    lifecycle.current().expect("投递需要 lease").lease,
                ) {
                    break;
                }
                let lease = lifecycle.current().expect("活跃观察者必须有 lease");
                lifecycle.sent(lease, ordinal);
                lifecycle.flushed(lease, ordinal);
                sent_through = ordinal;
            }
            PumpRead::Ready(Read::NeedsBaseline { through }) => {
                if !announced_gap {
                    announced_gap = true;
                    if !send(
                        DeliveryAction::Gap(through),
                        lifecycle.current().expect("缺口需要 lease").lease,
                    ) {
                        break;
                    }
                }
                if signal.is_closed() {
                    if terminal.is_none() && terminal_record.get().is_some() {
                        continue;
                    }
                    break;
                }
                signal.wait_for_change(observed).await;
            }
            PumpRead::CreditBlocked if producer_closed => {
                if let Some(terminal) = terminal {
                    emit_terminal(
                        &mut send,
                        terminal,
                        Some((sent_through + 1, terminal.after)),
                        lifecycle.current().expect("terminal 需要 lease").lease,
                    );
                    break;
                }
                // 关闭信号不再等待 epoch 变化；只由仍然有效的回执路由
                // 释放发送窗口，路由撤下则直接取消本代观察者。
                received_control = controlled.recv().await;
                if received_control.is_none() {
                    break;
                }
            }
            PumpRead::Ready(Read::Empty) | PumpRead::CreditBlocked => {
                signal.wait_for_change(observed).await;
            }
            PumpRead::Ready(Read::Closed) => {
                if let Some(terminal) = terminal_record.get() {
                    let missing =
                        (sent_through < terminal.after).then(|| (sent_through + 1, terminal.after));
                    emit_terminal(
                        &mut send,
                        terminal,
                        missing,
                        lifecycle.current().expect("terminal 需要 lease").lease,
                    );
                }
                break;
            }
        }
    }
    *final_evidence.lock().expect("观察终态锁已中毒") = Some(lifecycle.snapshot());
}

fn emit<R: Runtime>(
    app: &AppHandle<R>,
    packet: Packet,
    ordinal: Option<u64>,
    lease: Option<u64>,
) -> tauri::Result<()> {
    match packet {
        Packet::Update(payload) => match ordinal {
            Some(ordinal) => app.emit(
                ENGINE_EVENT_NAME,
                ObservedUpdate {
                    observer_lease: lease.expect("有序投递必须有 lease").to_string(),
                    payload,
                    publication_ordinal: ordinal.to_string(),
                },
            ),
            None => app.emit(ENGINE_EVENT_NAME, payload),
        },
        Packet::Failure(mut payload) => {
            payload.publication_ordinal = ordinal.map(|ordinal| ordinal.to_string());
            #[derive(Clone, Serialize)]
            struct ObservedFailure {
                #[serde(flatten)]
                payload: EngineFailurePayload,
                #[serde(skip_serializing_if = "Option::is_none")]
                observer_lease: Option<String>,
            }
            app.emit(
                "engine-failure",
                ObservedFailure {
                    payload,
                    observer_lease: lease.map(|value| value.to_string()),
                },
            )
        }
    }
}

pub(super) struct Delivery<R: Runtime> {
    app: AppHandle<R>,
    producer: Option<Producer<Packet>>,
    terminal: Option<Arc<OnceLock<Terminal>>>,
    lane: Option<JoinHandle<()>>,
    resume: Option<mpsc::Sender<Control>>,
    ack_route: AckRoute,
    offered_fence: Option<u64>,
    final_evidence: Arc<std::sync::Mutex<Option<publication::lifecycle::ObserverEvidence>>>,
    retired: Vec<JoinHandle<()>>,
    #[cfg(test)]
    inline_test: bool,
}

impl<R: Runtime> Delivery<R> {
    pub(super) fn start(
        app: AppHandle<R>,
        session_id: String,
        timeline_id: String,
        ack_route: AckRoute,
    ) -> Self {
        let mut delivery = Self {
            app,
            producer: None,
            terminal: None,
            lane: None,
            resume: None,
            ack_route,
            offered_fence: None,
            final_evidence: Arc::new(std::sync::Mutex::new(None)),
            retired: Vec::new(),
            #[cfg(test)]
            inline_test: false,
        };
        delivery.replace(session_id, timeline_id, 1);
        delivery
    }

    #[cfg(test)]
    pub(super) fn inline_test(app: AppHandle<R>) -> Self {
        Self {
            app,
            producer: None,
            terminal: None,
            lane: None,
            resume: None,
            ack_route: AckRoute::default(),
            offered_fence: None,
            final_evidence: Arc::new(std::sync::Mutex::new(None)),
            retired: Vec::new(),
            inline_test: true,
        }
    }

    pub(super) fn replace(&mut self, session_id: String, timeline_id: String, generation: u64) {
        #[cfg(test)]
        if self.inline_test {
            return;
        }
        self.close();
        // 旧 epoch 的 IPC 结果仍携带旧 timeline_id；前端拒绝晚到的结果。
        // 不等旧发送者退出，避免它把恢复或推进拖住。
        if let Some(old) = self.lane.take() {
            self.retired.retain(|lane| !lane.is_finished());
            self.retired.push(old);
        }
        let (producer, observer) = publication::channel(publication::OBSERVER_PACKET_LIMIT);
        let signal = Arc::clone(observer.signal());
        let terminal = Arc::new(OnceLock::new());
        self.terminal = Some(Arc::clone(&terminal));
        let app = self.app.clone();
        let (resume, requests) = mpsc::channel(CONTROL_CAPACITY);
        self.final_evidence = Arc::new(std::sync::Mutex::new(None));
        let lease_counter = Arc::new(AtomicU64::new(1));
        *self.ack_route.0.write().expect("观察者地址锁已中毒") = Some(AckEndpoint {
            lease: Arc::clone(&lease_counter),
            generation,
            sender: resume.clone(),
            signal: Arc::clone(&signal),
        });
        self.resume = Some(resume);
        self.lane = Some(native_async::spawn_io_lane(consume(
            observer,
            generation,
            lease_counter,
            Arc::clone(&self.final_evidence),
            move |action, lease| {
                let result = match action {
                    DeliveryAction::Item { ordinal, value } => {
                        emit(&app, value, Some(ordinal), Some(lease))
                    }
                    DeliveryAction::Terminal(failure) => {
                        emit(&app, Packet::Failure(failure), None, Some(lease))
                    }
                    DeliveryAction::Gap(through) => app.emit(
                        ENGINE_EVENT_NAME,
                        GapNotice {
                            observer_lease: lease.to_string(),
                            session_id: &session_id,
                            timeline_id: &timeline_id,
                            gap: GapCursor {
                                through: through.to_string(),
                            },
                        },
                    ),
                };
                if let Err(error) = result {
                    eprintln!("[session {session_id}] Tauri 观察通道发送失败：{error}");
                    return false;
                }
                true
            },
            signal,
            requests,
            publication::OBSERVER_IN_FLIGHT_LIMIT,
            terminal,
        )));
        self.producer = Some(producer);
    }

    pub(super) fn stage(&mut self, packet: Packet) {
        #[cfg(test)]
        if self.inline_test {
            emit(&self.app, packet, None, None).expect("测试用 inline sink 必须成功");
            return;
        }
        // 满载由公共观察流登记和通知。市场不能等待前端读取或确认。
        let weight = match &packet {
            Packet::Update(payload) => publication::protocol_weight(&payload.update),
            Packet::Failure(_) => NonZeroUsize::MIN,
        };
        let producer = self.producer.as_mut().expect("关闭后不能再交接观察输出");
        let _ = producer.publish_weighted(packet, weight);
    }

    pub(super) fn stage_terminal(&self, failure: EngineFailurePayload) {
        #[cfg(test)]
        if self.inline_test {
            emit(&self.app, Packet::Failure(failure), None, None)
                .expect("测试用 inline sink 必须成功");
            return;
        }
        let producer = self.producer.as_ref().expect("终止时必须有观察生产者");
        let record = self.terminal.as_ref().expect("终止时必须有首错记录");
        if record
            .set(Terminal {
                after: producer.published_ordinal(),
                failure,
            })
            .is_ok()
        {
            producer.signal().notify();
        }
    }

    #[cfg(any(test, feature = "host-parity"))]
    pub(super) async fn inspect_observer(
        &self,
    ) -> Result<publication::lifecycle::ObserverEvidence, String> {
        let completed = || {
            self.final_evidence
                .lock()
                .expect("观察终态锁已中毒")
                .as_ref()
                .copied()
                .ok_or_else(|| "OBSERVER_CLOSED".to_owned())
        };
        let (reply, result) = oneshot::channel();
        let Some(sender) = self.resume.as_ref() else {
            return completed();
        };
        if sender.try_send(Control::Inspect(reply)).is_err() {
            return completed();
        }
        self.producer
            .as_ref()
            .ok_or("OBSERVER_CLOSED")?
            .signal()
            .notify();
        match result.await {
            Ok(evidence) => Ok(evidence),
            Err(_) => completed(),
        }
    }

    pub(super) fn observer_lease(&self) -> u64 {
        self.ack_route
            .0
            .read()
            .expect("观察者地址锁已中毒")
            .as_ref()
            .map_or(1, |endpoint| endpoint.lease.load(Ordering::Acquire))
    }

    pub(super) fn offer_baseline_fence(&mut self) -> u64 {
        let ordinal = self
            .producer
            .as_ref()
            .map(Producer::published_ordinal)
            .unwrap_or(0);
        self.offered_fence = Some(ordinal);
        ordinal
    }

    pub(super) fn resume_after(
        &mut self,
        lease: u64,
        ordinal: u64,
        reply: oneshot::Sender<Result<(), String>>,
    ) {
        if self.offered_fence != Some(ordinal) {
            let _ = reply.send(Err("OBSERVER_BASELINE_STALE: 未签发该基线游标".into()));
            return;
        }
        let (Some(resume), Some(producer)) = (&self.resume, &self.producer) else {
            let _ = reply.send(Err("OBSERVER_CLOSED: 观察流已关闭".into()));
            return;
        };
        if let Err(error) = resume.try_send(Control::Resume(ResumeRequest {
            lease,
            ordinal,
            reply,
        })) {
            let Control::Resume(request) = error.into_inner() else {
                unreachable!()
            };
            let _ = request
                .reply
                .send(Err("OBSERVER_RESUME_UNAVAILABLE: 恢复请求未入队".into()));
            return;
        }
        self.offered_fence = None;
        producer.signal().notify();
    }

    fn close(&mut self) {
        *self.ack_route.0.write().expect("观察者地址锁已中毒") = None;
        self.resume = None;
        self.offered_fence = None;
        self.terminal = None;
        if let Some(producer) = self.producer.take() {
            let signal = Arc::clone(producer.signal());
            drop(producer);
            signal.close();
        }
    }

    pub(super) async fn finish(&mut self) -> Result<(), String> {
        self.close();
        let mut errors = Vec::new();
        for lane in self.retired.drain(..).chain(self.lane.take()) {
            if let Err(error) = lane.await {
                errors.push(format!("Tauri 观察通道执行任务退出异常：{error}"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }
}

impl<R: Runtime> Drop for Delivery<R> {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests;
