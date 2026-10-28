//! 真实 wasm watchdog 前件验证：预缓冲消息仍须进入实际生产收件入口。
use super::protocol::{RuntimeError, WorkerId};
use super::watchdog::{
    recv_deadline, worker_recv, worker_recv_spin, Phase, SpinConfig, WatchdogConfig,
};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::channel;
use std::time::Duration;

/// 每个独立 memory 只拥有一个 probe；trap 后由宿主读取实际入口阶段。
#[repr(C)]
pub struct WatchdogProbe {
    words: [AtomicU32; 4],
}

impl WatchdogProbe {
    pub fn new(mode: u32) -> Result<Self, String> {
        if mode > 2 {
            return Err("watchdog 验证 mode 必须为 0..2".to_owned());
        }
        let probe = Self {
            words: [const { AtomicU32::new(0) }; 4],
        };
        probe.words[0].store(mode, Ordering::SeqCst);
        Ok(probe)
    }
    /// 保留 Arc 至 hook 退出，避免 trap 后宿主读取悬空观测字；只在验证 feature 设置。
    pub fn install_panic_probe(probe: &std::sync::Arc<Self>) {
        let probe = std::sync::Arc::clone(probe);
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let expected_location = info
                .location()
                .is_some_and(|location| location.file().ends_with("/sys/time/unsupported.rs"));
            let expected_message = info
                .to_string()
                .contains("time not implemented on this platform");
            probe.words[3].store(
                if expected_location && expected_message {
                    1
                } else {
                    2
                },
                Ordering::SeqCst,
            );
            previous(info);
        }));
    }
    pub fn address(&self) -> u32 {
        self.words.as_ptr() as usize as u32
    }
    fn stage(&self, value: u32) {
        self.words[1].store(value, Ordering::SeqCst);
    }
}

/// 0=coordinator，1=worker，2=禁用 spin 的实际 worker fallback；不冒充等待边界验证。
pub fn verify_watchdog_recv(mode: u32, probe: &WatchdogProbe) -> Result<String, String> {
    if probe.words[0].load(Ordering::SeqCst) != mode {
        return Err("watchdog probe 与 case 不一致".to_owned());
    }
    let (tx, rx) = channel();
    tx.send(17u32).expect("第一条真实消息入队");
    tx.send(29u32).expect("第二条真实消息入队");
    drop(tx);
    let cfg = WatchdogConfig {
        normal: Duration::from_millis(100),
        long: Duration::from_millis(100),
    };
    let shutdown = AtomicBool::new(false);
    let receive = || match mode {
        0 => recv_deadline(&rx, Phase::TickConverge, &cfg).map(Some),
        1 => worker_recv(&rx, &cfg, &shutdown, WorkerId::Group(0)),
        2 => worker_recv_spin(
            &rx,
            &cfg,
            &shutdown,
            WorkerId::Group(0),
            SpinConfig {
                enabled: false,
                window: Duration::ZERO,
            },
        ),
        _ => unreachable!("probe 已校验 mode"),
    };
    probe.stage(10);
    assert_eq!(receive().expect("第一条生产收件不得 trap 或错误"), Some(17));
    probe.words[2].store(1, Ordering::SeqCst);
    probe.stage(20);
    assert_eq!(receive().expect("第二条生产收件必须保持 FIFO"), Some(29));
    probe.words[2].store(2, Ordering::SeqCst);
    probe.stage(30);
    match (mode, receive()) {
        (0, Err(RuntimeError::Disconnected { .. })) | (1 | 2, Ok(None)) => {}
        (_, other) => panic!("真实缓冲排空后的断连语义错误：{other:?}"),
    }
    probe.stage(60);
    Ok(format!(
        "真实 watchdog 入口 mode={mode}：FIFO 两条及排空后断连均通过"
    ))
}
