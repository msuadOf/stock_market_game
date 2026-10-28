//! 实际 wasm adapter 的配置、相位与永久关停边界；不复用 std timeout 或虚拟时钟。
use super::protocol::{RuntimeError, WorkerId};
use super::watchdog::{self, Phase, SpinConfig, WatchdogConfig};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, TryRecvError};
use std::time::Duration;

/// 独立 memory 的初始化合同；所有非法值与重复初始化必须保持显式错误。
pub fn verify_wait_configuration() -> Result<String, String> {
    assert!(
        watchdog::wasm_wait_abort_layout().is_err(),
        "未初始化不能泄露可用 abort 布局"
    );
    let (_tx, rx) = channel::<u32>();
    let cfg = WatchdogConfig::default_config();
    assert!(
        matches!(watchdog::recv_deadline(&rx, Phase::Seal, &cfg), Err(RuntimeError::ProtocolInvariant { detail }) if detail.contains("尚未初始化"))
    );
    assert!(watchdog::init_wasm_wait(0, 0, 0, 1).is_err());
    assert!(watchdog::init_wasm_wait(0, 0, 1, 0).is_err());
    watchdog::init_wasm_wait(500, 2000, 250, 2000)?;
    assert!(watchdog::init_wasm_wait(500, 2000, 250, 2000).is_err());
    let layout = watchdog::wasm_wait_abort_layout()?;
    assert!(layout.address > 0);
    assert_eq!(
        layout.bytes, 1,
        "宿主必须使用 1B 原子 store，不覆盖相邻字段"
    );
    let spin = watchdog::spin_config();
    assert!(spin.enabled);
    assert_eq!(
        spin.window,
        Duration::from_micros(500),
        "group 的 spin_config 必须与显式 settings 同源"
    );
    Ok("显式 settings 未初始化/非法 slice/重复 init 均拒绝；spin 同源，abort 布局为 1B".to_owned())
}

/// 真时钟超时必须返回实际等待时长；quiet 预算不能让 coordinator 落裸 recv。
pub fn verify_wait_phase_limits() -> Result<String, String> {
    let (_tx, rx) = channel::<u32>();
    let cfg = WatchdogConfig {
        normal: Duration::from_millis(4),
        long: Duration::from_millis(6),
    };
    for phase in [Phase::TickConverge, Phase::Drain] {
        let started = crate::clock::Instant::now();
        let limit = cfg.limit_for(phase);
        match watchdog::recv_deadline(&rx, phase, &cfg) {
            Err(RuntimeError::Watchdog {
                phase: observed,
                waited,
                ..
            }) => {
                assert_eq!(observed, phase);
                assert!(waited >= limit, "实际时钟不能被分片次数冒充");
                assert!(started.elapsed() >= waited);
            }
            other => return Err(format!("真实 wasm 相位限错误：{other:?}")),
        }
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    }
    Ok("真实细片 TickConverge 与粗片 Drain 均在各自实际相位限返回 Watchdog".to_owned())
}

/// 两个 worker 入口均须在永久 shutdown 后拒绝交付已缓冲消息；它与 tick Abort 不共享标记。
pub fn verify_wait_permanent_shutdown() -> Result<String, String> {
    let (tx, rx) = channel();
    tx.send(41u32).expect("真实缓冲消息入队");
    let shutdown = AtomicBool::new(true);
    let cfg = WatchdogConfig::default_config();
    assert_eq!(
        watchdog::worker_recv(&rx, &cfg, &shutdown, WorkerId::Desk(0))
            .map_err(|error| error.to_string())?,
        None
    );
    assert_eq!(
        rx.try_recv(),
        Ok(41),
        "shutdown 前沿拒交付时不得先消费缓冲消息"
    );
    tx.send(73).expect("第二条真实缓冲消息入队");
    assert_eq!(
        watchdog::worker_recv_spin(
            &rx,
            &cfg,
            &shutdown,
            WorkerId::Group(0),
            SpinConfig {
                enabled: true,
                window: Duration::from_micros(500)
            }
        )
        .map_err(|error| error.to_string())?,
        None
    );
    assert_eq!(rx.try_recv(), Ok(73));
    let ordinary_shutdown = AtomicBool::new(false);
    tx.send(89).expect("普通收件消息入队");
    assert_eq!(
        watchdog::worker_recv(&rx, &cfg, &ordinary_shutdown, WorkerId::Desk(0))
            .map_err(|error| error.to_string())?,
        Some(89)
    );
    Ok("两个 worker 入口已验证永久 shutdown 前沿拒交付；独立普通收件仍交付消息".to_owned())
}

/// 宿主已在同 memory 用 Uint8Array Atomics.store(address,1) 置位；不接受虚拟 abort。
pub fn verify_wait_host_aborted() -> Result<String, String> {
    let (tx, rx) = channel();
    tx.send(97u32).expect("宿主 abort 前的真实消息入队");
    let cfg = WatchdogConfig::default_config();
    let shutdown = AtomicBool::new(false);
    assert!(matches!(
        watchdog::recv_deadline(&rx, Phase::Seal, &cfg),
        Err(RuntimeError::HostAborted {
            phase: Phase::Seal,
            who: None
        })
    ));
    assert!(matches!(
        watchdog::worker_recv(&rx, &cfg, &shutdown, WorkerId::Desk(0)),
        Err(RuntimeError::HostAborted {
            phase: Phase::WorkerIdle,
            who: Some(WorkerId::Desk(0))
        })
    ));
    assert!(matches!(
        watchdog::worker_recv_spin(
            &rx,
            &cfg,
            &shutdown,
            WorkerId::Group(0),
            SpinConfig {
                enabled: false,
                window: Duration::ZERO
            }
        ),
        Err(RuntimeError::HostAborted {
            phase: Phase::WorkerIdle,
            who: Some(WorkerId::Group(0))
        })
    ));
    assert_eq!(
        rx.try_recv(),
        Ok(97),
        "宿主 abort 优先于缓冲消息，不得交付成功"
    );
    Ok("同 memory 宿主永久 abort 被三入口明确报告，原消息仍在队列".to_owned())
}

/// 真实等待的选路与 owned handle 观测；rawSelected 只证明裸 recv 分支被选择，不能证明 futex park。
#[repr(C)]
pub struct WaitProbe {
    words: [std::sync::atomic::AtomicU32; 16],
}
impl WaitProbe {
    pub fn new(mode: u32) -> Result<Self, String> {
        if mode > 11 {
            return Err("wait boundary mode 必须为 0..11".to_owned());
        }
        let probe = Self {
            words: [const { std::sync::atomic::AtomicU32::new(0) }; 16],
        };
        probe.words[0].store(mode, std::sync::atomic::Ordering::SeqCst);
        Ok(probe)
    }
    pub fn address(&self) -> u32 {
        self.words.as_ptr() as usize as u32
    }
    fn load(&self, index: usize) -> u32 {
        self.words[index].load(std::sync::atomic::Ordering::SeqCst)
    }
    fn stage(&self, value: u32) {
        self.words[1].store(value, std::sync::atomic::Ordering::SeqCst);
    }
    pub(crate) fn note_spin(&self) {
        self.words[12].fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    pub(crate) fn note_sleep(&self) {
        self.words[13].fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    pub(crate) fn note_raw_recv_selected(&self) {
        self.words[14].fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

fn wait_for(probe: &WaitProbe, index: usize) {
    let started = crate::clock::Instant::now();
    while probe.load(index) == 0 {
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "真实等待未到指定选路观测 index={index}"
        );
        std::thread::sleep(Duration::from_micros(250));
    }
}

/// 4=raw 消息，5=raw 后 shutdown 过滤，6=raw 后 host abort 过滤；
/// 7/8=coordinator 细/粗片中宿主 abort，9=quiet shutdown，10=Hot shutdown，11=显式 disabled spin。
pub fn verify_wait_boundary(mode: u32, probe: std::sync::Arc<WaitProbe>) -> Result<String, String> {
    if probe.load(0) != mode {
        return Err("wait probe 与 case 不一致".to_owned());
    }
    probe.stage(10);
    let simple = match mode {
        0 => Some(verify_wait_configuration()),
        1 => Some(verify_wait_phase_limits()),
        2 => Some(verify_wait_permanent_shutdown()),
        3 => Some(verify_wait_host_aborted()),
        _ => None,
    };
    if let Some(result) = simple {
        let result = result?;
        probe.stage(60);
        return Ok(result);
    }
    if mode == 7 || mode == 8 {
        let (_tx, rx) = channel::<u32>();
        let phase = if mode == 7 {
            Phase::TickConverge
        } else {
            Phase::Drain
        };
        let cfg = WatchdogConfig {
            normal: Duration::from_secs(2),
            long: Duration::from_secs(2),
        };
        assert!(
            matches!(watchdog::verify_coordinator_recv(&rx, phase, &cfg, std::sync::Arc::clone(&probe)),
            Err(RuntimeError::HostAborted { phase: actual, who: None }) if actual == phase)
        );
        assert!(
            probe.load(13) > 0 && probe.load(14) == 0,
            "coordinator 必须留在真实 sleep 分片，不能落裸 recv"
        );
        probe.stage(60);
        return Ok(format!(
            "coordinator {phase:?} 真实分片观测到同 memory 宿主 abort"
        ));
    }
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));
    let (tx, rx) = channel::<super::protocol::GroupIn>();
    let child_shutdown = std::sync::Arc::clone(&shutdown);
    let child_probe = std::sync::Arc::clone(&probe);
    let spin = if mode == 11 {
        SpinConfig {
            enabled: false,
            window: Duration::ZERO,
        }
    } else {
        watchdog::spin_config()
    };
    let handle = crate::thread_facade::Builder::new()
        .name("wait-boundary-worker".to_owned())
        .spawn(move || {
            let cfg = WatchdogConfig {
                normal: Duration::from_nanos(1),
                long: Duration::from_nanos(1),
            };
            let result = watchdog::verify_worker_recv(
                &rx,
                &cfg,
                &child_shutdown,
                WorkerId::Group(0),
                spin,
                std::sync::Arc::clone(&child_probe),
            );
            child_probe.words[15].store(1, std::sync::atomic::Ordering::SeqCst);
            result
        })
        .map_err(|error| format!("真实等待 worker spawn 失败：{error}"))?;
    probe.words[4].store(handle.join_index(), std::sync::atomic::Ordering::SeqCst);
    probe.words[8].store(0, std::sync::atomic::Ordering::SeqCst);
    probe.words[3].store(1, std::sync::atomic::Ordering::SeqCst);
    let watched = match mode {
        9 => 13,
        10 => 12,
        _ => 14,
    };
    wait_for(&probe, watched);
    if mode == 6 {
        // 由真实宿主读取 stage50 后，用同 memory 的 1B 原子 store 置位；普通 message 不能中断阻塞 Rust。
        probe.stage(50);
        let started = crate::clock::Instant::now();
        while !watchdog::verify_host_aborted() {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "宿主未置位真实共享 abort 字"
            );
            std::thread::sleep(Duration::from_micros(250));
        }
        assert_eq!(
            probe.load(15),
            0,
            "GLOBAL_ABORT 不得冒充裸 mpsc recv 的唤醒源"
        );
    }
    if matches!(mode, 5 | 9 | 10) {
        shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    if !matches!(mode, 9 | 10) {
        assert_eq!(
            probe.load(15),
            0,
            "选中裸 recv 路径后无消息/断连/永久哨兵不能提前返回"
        );
        tx.send(super::protocol::GroupIn::Shutdown)
            .map_err(|error| format!("真实选路后的唤醒消息失败：{error}"))?;
    }
    let result = handle
        .join()
        .map_err(|error| format!("真实等待 worker join 失败：{error}"))?;
    match mode {
        4 | 11 => assert!(matches!(
            result.map_err(|error| error.to_string())?,
            Some(super::protocol::GroupIn::Shutdown)
        )),
        5 | 9 | 10 => assert!(result.map_err(|error| error.to_string())?.is_none()),
        6 => assert!(matches!(
            result,
            Err(RuntimeError::HostAborted {
                phase: Phase::WorkerIdle,
                who: Some(WorkerId::Group(0))
            })
        )),
        _ => return Err("未知真实等待边界 mode".to_owned()),
    }
    assert_eq!(probe.load(15), 1);
    match mode {
        9 => assert!(probe.load(13) > 0 && probe.load(14) == 0),
        10 => assert!(probe.load(12) > 0 && probe.load(13) == 0 && probe.load(14) == 0),
        11 => assert!(probe.load(12) == 0 && probe.load(14) == 1),
        // 调度停顿可能在首个 sample 前耗尽短预算；raw case 不承诺至少一次 spin/sleep。
        // Hot/quiet 的实际调用由各自长预算握手 case 10/9 单独覆盖。
        _ => assert_eq!(probe.load(14), 1),
    }
    probe.stage(60);
    Ok(format!(
        "实际 wait 边界 mode={mode} 完成，owned join 已消费；未宣称已观测 futex park"
    ))
}
