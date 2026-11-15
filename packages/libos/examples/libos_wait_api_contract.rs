//! 测试专用 Browser 探针，真实调用 LibOS；用等待观察点和宿主时钟 import 注入确定的交错。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[path = "../tests/support/wait_api.rs"]
mod shared;

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[path = "../tests/support/construction_cleanup.rs"]
mod construction_cleanup;

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
mod browser {
    use libos::{clock::Instant, wait::{self, WaitFailure, WaitObserver}};
    use std::{cell::RefCell, sync::mpsc::{self, Sender}, time::Duration};

    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        fn wait_contract_before_sleep();
    }

    struct Probe;
    impl WaitObserver for Probe {
        fn note_sleep(&self) {
            // 测试宿主只安排事件与时钟样本；实际 sleep 仍由生产等待 adapter 执行。
            unsafe { wait_contract_before_sleep() };
        }
        fn note_spin(&self) {
            panic!("协调者不应进入 worker 自旋分支");
        }
        fn note_raw_recv_selected(&self) {
            panic!("协调者不应用裸 recv 绕过 deadline");
        }
    }

    thread_local! {
        static SENDER: RefCell<Option<Sender<u32>>> = const { RefCell::new(None) };
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn wait_contract_init() {
        wait::init_wasm_wait(0, 0, 250, 1_000).unwrap();
        let _ = Instant::now(); // 写入共享时钟原点；故障测例确保样本仍晚于该原点。
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn wait_contract_initialization() {
        use std::sync::atomic::AtomicBool;
        let (tx, rx) = mpsc::channel::<u32>();
        tx.send(61).unwrap();
        let stop = AtomicBool::new(false);

        fn assert_uninitialized(rx: &mpsc::Receiver<u32>, stop: &AtomicBool) {
            assert!(wait::require_wait_initialized().is_err());
            assert!(wait::wasm_wait_abort_layout().is_err());
            assert!(wait::wasm_wait_death_layout().is_err());
            assert!(matches!(
                wait::recv_deadline(rx, Duration::ZERO, false),
                Err(WaitFailure::Uninitialized(_))
            ));
            assert!(matches!(wait::worker_recv(rx, stop), Err(WaitFailure::Uninitialized(_))));
            assert!(matches!(
                wait::worker_recv_spin(
                    rx, stop, wait::SpinConfig { enabled: false, window: Duration::ZERO },
                ),
                Err(WaitFailure::Uninitialized(_))
            ));
        }

        assert_uninitialized(&rx, &stop);
        for (fine, coarse) in [(0, 1000), (250, 0)] {
            assert!(wait::init_wasm_wait(40, 0, fine, coarse).is_err());
            assert_uninitialized(&rx, &stop);
        }
        wait::init_wasm_wait(40, 0, 250, 1000).expect("非法配置不得占用初始化状态");
        assert!(wait::require_wait_initialized().is_ok());
        let abort = wait::wasm_wait_abort_layout().unwrap();
        let death = wait::wasm_wait_death_layout().unwrap();
        assert_ne!(abort.address, 0);
        assert_ne!(death.address, 0);
        assert_eq!((abort.bytes, death.bytes), (1, 4));
        assert_eq!(death.address % death.bytes, 0);
        assert!(abort.address + abort.bytes <= death.address || death.address + death.bytes <= abort.address);

        assert!(wait::init_wasm_wait(60, 1, 500, 1500).is_err());
        assert!(wait::spin_config(false).enabled);
        assert_eq!(wait::spin_config(false).window, Duration::from_micros(40));
        let abort_after = wait::wasm_wait_abort_layout().unwrap();
        let death_after = wait::wasm_wait_death_layout().unwrap();
        assert_eq!((abort_after.address, abort_after.bytes), (abort.address, abort.bytes));
        assert_eq!((death_after.address, death_after.bytes), (death.address, death.bytes));
        assert_eq!(wait::recv_deadline(&rx, Duration::ZERO, false), Ok(61), "失败或重复初始化不得吞掉缓冲");
        drop(tx);
        assert_eq!(wait::worker_recv(&rx, &stop), Ok(None));
    }

    // 测试宿主同线程回调可在等待片首尾发布，不持有收件过程中的 RefCell borrow。
    #[unsafe(no_mangle)]
    pub extern "C" fn wait_contract_publish(kind: u32) {
        SENDER.with_borrow_mut(|cell| {
            let tx = cell.take().expect("每次注入只使用一个有效 Sender");
            match kind {
                0 => { tx.send(42).unwrap(); *cell = Some(tx); }
                1 => drop(tx),
                _ => panic!("未知测试事件"),
            }
        });
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn wait_contract_shared() {
        super::shared::clock_arithmetic();
        super::shared::coordinator_queue();
        super::shared::worker_queue();
        super::shared::shutdown_wake();
        super::construction_cleanup::abort_without_registration();
        super::construction_cleanup::abort_after_partial_registration();
        super::construction_cleanup::untransferred_ownership();
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn wait_contract_case(case: u32, coarse: u32) {
        assert!(coarse <= 1);
        let (tx, rx) = mpsc::channel();
        SENDER.with_borrow_mut(|cell| assert!(cell.replace(tx).is_none()));
        let result = wait::verify_coordinator_recv(&rx, Duration::from_millis(1), coarse != 0, &Probe);
        match case {
            0 => assert_eq!(result, Ok(42), "在超时判定点可观察到数据，优先交付"),
            1 => assert_eq!(result, Err(WaitFailure::Disconnected), "在超时判定点已断开，不报告空队列超时"),
            2 => {
                assert!(matches!(result, Err(WaitFailure::Deadline { .. })));
                wait_contract_publish(0);
                assert_eq!(rx.try_recv(), Ok(42), "判定后的消息留给后续接收");
            }
            // 不要求暴露绝对时钟原点，只要求正常锚定后退回样本以类型化错误退出。
            3 | 4 => {
                match result {
                    Err(WaitFailure::ClockRegressed { previous, observed }) => assert!(previous > observed),
                    other => panic!("回退时须返回 ClockRegressed，实际：{other:?}"),
                }
            }
            _ => panic!("未知测试 case"),
        }
        SENDER.with_borrow_mut(|cell| drop(cell.take()));
    }
}

fn main() {}
