//! 相位感知看门狗（Sol 终审修订项）。
//!
//! 固定单值超时（原型 5s）在长相位（日结公司域事务、持久化窗口）会误报——worker
//! 长时空闲等待是协议设计的一部分。处置：相位分档放宽阈值，超时上报必须携带相位
//! 上下文（不静默放大全局阈值）。正常相位保持紧阈值以尽快暴露挂死。

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use super::protocol::{RuntimeError, WorkerId};

/// 等待相位：决定看门狗阈值档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// worker 收件循环空闲等待（正常档）。
    WorkerIdle,
    /// 协调者 tick 收束（等待 G GroupReport + N DeskReport，正常档）。
    TickConverge,
    /// Q2 Seal 收尾确认收集（等待 G SealAck，正常档——一组一轮小消息）。
    Seal,
    /// 日终 Drain→DeskDrained 收集（长相位档：清簿回流可重）。
    Drain,
    /// 关停 Final 收集（长相位档：期末上交可重）。
    FinalCollection,
}

/// 看门狗阈值配置。正常相位与长相位分档；测试注入小值。
#[derive(Clone, Copy, Debug)]
pub struct WatchdogConfig {
    /// 正常相位上限（tick 收束/worker 空闲）。
    pub normal: Duration,
    /// 长相位上限（Drain/FinalCollection：日结事务与持久化窗口）。
    pub long: Duration,
}

impl WatchdogConfig {
    /// 缺省：正常 5s（对齐原型 RECV_TIMEOUT），长相位 60s。
    pub const fn default_config() -> Self {
        WatchdogConfig {
            normal: Duration::from_secs(5),
            long: Duration::from_secs(60),
        }
    }

    /// 相位对应阈值：Drain/FinalCollection 走长档，其余走正常档。
    pub fn limit_for(&self, phase: Phase) -> Duration {
        match phase {
            Phase::Drain | Phase::FinalCollection => self.long,
            Phase::WorkerIdle | Phase::TickConverge | Phase::Seal => self.normal,
        }
    }
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

/// 带相位的收件：超时→携带相位上下文的显式 `RuntimeError::Watchdog`；
/// 断连→显式 `RuntimeError::Disconnected`。绝不静默返回 None。
pub fn recv_deadline<T>(
    rx: &Receiver<T>,
    phase: Phase,
    cfg: &WatchdogConfig,
) -> Result<T, RuntimeError> {
    match rx.recv_timeout(cfg.limit_for(phase)) {
        Ok(msg) => Ok(msg),
        Err(RecvTimeoutError::Timeout) => Err(RuntimeError::Watchdog {
            phase,
            waited: cfg.limit_for(phase),
            detail: format!(
                "相位 {phase:?} 等待超时（正常档={}s，长相位档={}s）",
                cfg.normal.as_secs(),
                cfg.long.as_secs()
            ),
        }),
        Err(RecvTimeoutError::Disconnected) => Err(RuntimeError::Disconnected {
            who: None,
            stage: format!("相位 {phase:?} 上行通道断连"),
        }),
    }
}

/// worker 侧收件变体：等待中响应**永久关停标记**（shutdown——sever/
/// Drop/显式 shutdown 置位）⇒ 分片窗口内干净退出；超时且无关停 ⇒ 显式
/// 失败上报；断连 ⇒ 干净退出（协调者已释放通道，关停协议的一部分）。
/// Sol 终审三审终阻断：退出判定**不复用 abort 标记**——abort_tick 置
/// 位后仅成功 install_tick 才清，可重试 tick 的 Abort（回滚存活契约）
/// 窗口内 worker 不得退出；此前 50ms 分片轮询读到 abort 即退出，使
/// "Abort 后稍等再重试"撞通道断连（worker 已亡）。永久关停与可重试
/// Abort 是两个正交信号：shutdown=进程级回收（sever/Drop）；
/// abort=tick 级回滚语义（组内 GroupIn::Abort 消息驱动回滚）。
/// 分片轮询（50ms 步长）保持：sever 后组↔部互持对方通道 clone 形成
/// 断连互等（Disconnected 互不可达），shutdown 在 ≤50ms 内被空闲
/// worker 观察——回收即时；正常路径消息到达即时唤醒无轮询税。
pub fn worker_recv<T>(
    rx: &Receiver<T>,
    cfg: &WatchdogConfig,
    shutdown: &std::sync::atomic::AtomicBool,
    who: WorkerId,
) -> Result<Option<T>, RuntimeError> {
    use std::sync::atomic::Ordering;
    let limit = cfg.limit_for(Phase::WorkerIdle);
    let step = std::cmp::min(Duration::from_millis(50), limit);
    let mut waited = Duration::ZERO;
    loop {
        match rx.recv_timeout(step) {
            Ok(msg) => return Ok(Some(msg)),
            Err(RecvTimeoutError::Disconnected) => return Ok(None),
            Err(RecvTimeoutError::Timeout) => {
                waited += step;
                if shutdown.load(Ordering::SeqCst) {
                    return Ok(None);
                }
                if waited >= limit {
                    return Err(RuntimeError::Watchdog {
                        phase: Phase::WorkerIdle,
                        waited: limit,
                        detail: format!("worker {who} 空闲超时且无永久关停标记"),
                    });
                }
            }
        }
    }
}

/// B-1' 组线程等待自旋配置（协调者批 2026-10-19——seal-spread-wakeup
/// -design.md）：消息密集期（tick 内 Start/Continue/Batch/Seal 接连
/// 到达）组线程热态自旋，免深 C-state futex 唤醒延迟（静置窗实测
/// spread 94μs 无归属段的归因）。自旋窗超时回落现行 50ms 步进睡眠。
#[derive(Clone, Copy, Debug)]
pub struct SpinConfig {
    /// 是否启用自旋（ENGINE_SPIN_DISABLE=1 或 wasm/test 环境恒 false）。
    pub enabled: bool,
    /// 热态自旋窗（默认 500μs——验证批 250/500/1000 三档配对选优，
    /// 选中值依据实测 spread 分布登记；协调者裁决"小常数须带数据来源"）。
    pub window: Duration,
}

/// 进程级自旋配置（env 读一次）。默认开（单局吞吐优先的部署取向——
/// 协调者裁决②）；`ENGINE_SPIN_DISABLE=1` 逃生；`ENGINE_SPIN_US`
/// 调档（微秒）。**cfg(test) 恒关**：全量回归不因自旋空转拖慢——自旋
/// 分支等价性由 watchdog_spin_tests 直构 SpinConfig 覆盖。
/// **wasm 恒关**：串行路径无 OS 线程，自旋无意义（协调者裁决②）。
pub fn spin_config() -> SpinConfig {
    #[cfg(any(test, target_family = "wasm"))]
    {
        SpinConfig {
            enabled: false,
            window: Duration::ZERO,
        }
    }
    #[cfg(not(any(test, target_family = "wasm")))]
    {
        static SPIN: std::sync::OnceLock<SpinConfig> = std::sync::OnceLock::new();
        *SPIN.get_or_init(|| {
            let disabled = std::env::var("ENGINE_SPIN_DISABLE").ok().as_deref() == Some("1");
            // 默认 500μs：覆盖静置窗实测 spread 主体（124-185μs）且不过度
            // 空转——验证批三档（250/500/1000）配对复核后如改值须同步本注释。
            let window_us = match std::env::var("ENGINE_SPIN_US") {
                Err(_) => 500,
                // 复核建议②收口：显式设置的部署参数非法值=显式失败，
                /// 不静默回落默认（铁律二"不用默认值掩盖异常"）。
                Ok(raw) => spin_us_from_str(&raw),
            };
            SpinConfig {
                enabled: !disabled,
                window: Duration::from_micros(window_us),
            }
        })
    }
}

/// `ENGINE_SPIN_US` 显式值解析：非法（非数字/溢出）→ panic（含值与
/// 原因——部署意图明确表达时静默回落会掩盖配置错误）。
/// 非 wasm 门控（cfg(test) 可见——panic 合同由 watchdog_spin_tests 覆盖；
/// wasm 无 OS 线程，spin_config 恒关不走本函数）。
#[cfg(not(target_family = "wasm"))]
pub(crate) fn spin_us_from_str(raw: &str) -> u64 {
    raw.parse::<u64>().unwrap_or_else(|e| {
        panic!("ENGINE_SPIN_US 非法值 {raw:?}：{e}——显式部署意图不静默回落默认 500μs（铁律二）")
    })
}

/// worker 侧收件的组线程变体（B-1' 分级自旋——先热态自旋窗再回落
/// `worker_recv` 睡眠路径）。语义等价论证（seal-spread-wakeup-design
/// §3）：消息序 FIFO 不变（try_recv/recv_timeout 同队）；断连→Ok(None)、
/// 关停→Ok(None)、watchdog 判定（自旋段计入 waited）全部保持；自旋
/// 耗尽后立即观察 shutdown（响应 ≤T_spin+ε，快于睡眠路径的 50ms 步进）。
pub fn worker_recv_spin<T>(
    rx: &Receiver<T>,
    cfg: &WatchdogConfig,
    shutdown: &std::sync::atomic::AtomicBool,
    who: WorkerId,
    spin: SpinConfig,
) -> Result<Option<T>, RuntimeError> {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::TryRecvError;
    let limit = cfg.limit_for(Phase::WorkerIdle);
    let mut waited = Duration::ZERO;
    // 热态自旋窗：worker_recv_spin 被调用时=刚处理完一条消息（核热）。
    // 窗内 try_recv 轮询（spin_loop 让出流水线端口——降功耗与 SMT 干扰）；
    // 断连即时退出；窗耗尽立即观察 shutdown（响应 ≤窗+ε）并回落睡眠。
    if spin.enabled && spin.window > Duration::ZERO {
        let spin_started = std::time::Instant::now();
        loop {
            match rx.try_recv() {
                Ok(msg) => return Ok(Some(msg)),
                Err(TryRecvError::Disconnected) => return Ok(None),
                Err(TryRecvError::Empty) => {}
            }
            let elapsed = spin_started.elapsed();
            if elapsed >= spin.window || elapsed >= limit {
                // 自旋段计入空闲等待（watchdog 判定口径不变）。
                waited = elapsed.min(limit);
                break;
            }
            std::hint::spin_loop();
        }
        if shutdown.load(Ordering::SeqCst) {
            return Ok(None);
        }
        if waited >= limit {
            return Err(RuntimeError::Watchdog {
                phase: Phase::WorkerIdle,
                waited: limit,
                detail: format!("worker {who} 空闲超时且无永久关停标记"),
            });
        }
    }
    // 回落睡眠路径（与 worker_recv 同式——waited 续算）。
    let step = std::cmp::min(Duration::from_millis(50), limit);
    loop {
        match rx.recv_timeout(step) {
            Ok(msg) => return Ok(Some(msg)),
            Err(RecvTimeoutError::Disconnected) => return Ok(None),
            Err(RecvTimeoutError::Timeout) => {
                waited += step;
                if shutdown.load(Ordering::SeqCst) {
                    return Ok(None);
                }
                if waited >= limit {
                    return Err(RuntimeError::Watchdog {
                        phase: Phase::WorkerIdle,
                        waited: limit,
                        detail: format!("worker {who} 空闲超时且无永久关停标记"),
                    });
                }
            }
        }
    }
}
