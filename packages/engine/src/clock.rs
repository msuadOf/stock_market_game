//! 时钟门面（wasm 线程抽象层 R4a 阶段 1）。
//!
//! 设计锚：`.tmp/wasm-thread-abstraction-design-input.md` §6.1.3-2（时钟层
//! 窄门面）/ §7.3.2（env.engine_clock_now 白名单）/ §7.3.3 + §8.3（跨
//! Worker timeOrigin 映射，启动时差内含）。
//!
//! - **native**：`pub use std::time::Instant`——编译期同义 re-export，零
//!   运行期间接（§0 硬约束：native 逐指令等价，asm 对照留证于
//!   agents/wasm-thread-r4a/）。
//! - **wasm32**：std `Instant::now` 在 wasm32-unknown-unknown 必 panic
//!   （std `sys/time/unsupported`，§6.1.1 F1/N1——两工具链同构）。本门面
//!   经唯一时间 import `env.engine_clock_now`（f64 ms，JS bootstrap =
//!   performance.timeOrigin + performance.now()）换算到**协调者共享时间线**。
//!
//! wasm 语义（§8.3 公式）：`engine_clock_now(w) = A_w − timeOrigin_c`，其中
//! `A_w = timeOrigin_w + now_w`（JS 侧已合成，Rust 只见绝对 ms）。协调者
//! 首次取时锚定 `ORIGIN`（CAS 一次，子 Worker 永不覆写——读到非零即沿用
//! 协调者时间线；启动时差已内含于 timeOrigin_w，无成对读数固化偏差）。
//! 每 Worker 内单调性由 performance.now 单调性保证；跨 Worker 对齐为
//! **诊断级精度**（µs~ms 量级，不作协议判据——协议等待全用时长累计）。

// ---------------------------------------------------------------------------
// native：编译期同义 re-export（§0——零运行期间接；asm 对照留证）。
// ---------------------------------------------------------------------------
#[cfg(not(target_family = "wasm"))]
pub use std::time::Instant;

// ---------------------------------------------------------------------------
// wasm32：env.engine_clock_now（f64 绝对 ms）→ 协调者共享时间线 Instant。
// ---------------------------------------------------------------------------
#[cfg(target_family = "wasm")]
mod wasm_impl {
    use std::ops::{Add, Sub};
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    use std::time::Duration;

    /// 唯一时间 import（§7.3.2 白名单符号，glue 由 vite alias 'env' →
    /// apps/web/src/host/engine-clock-env.ts 提供）。返回
    /// `performance.timeOrigin + performance.now()`（绝对 ms，§8.3 的 A_w）。
    #[link(wasm_import_module = "env")]
    extern "C" {
        fn engine_clock_now() -> f64;
    }

    /// 协调者 timeOrigin（绝对 ms 的 f64 位型）。0=未锚定。
    /// 共享线性内存 static：阶段 2 子 Worker 与协调者同内存直读（读到非零
    /// 即协调者时间线；协调者 CAS 首锚，子 Worker 永不覆写——§8.3 公式）。
    /// 0 哨兵安全：timeOrigin 为 epoch 级毫秒（~1.7e12），真值恒非 0。
    static COORDINATOR_ORIGIN_BITS: AtomicU64 = AtomicU64::new(0);

    /// 共享时间线纳秒（自协调者 timeOrigin 起）。负值/溢出=不变量破坏
    /// （同一单调时间线上 later-or-equal 恒成立），显式 panic 不静默夹逼。
    fn shared_line_nanos() -> u64 {
        let raw = unsafe { engine_clock_now() };
        let raw_bits = raw.to_bits();
        let mut current = COORDINATOR_ORIGIN_BITS.load(AtomicOrdering::Acquire);
        if current == 0 {
            match COORDINATOR_ORIGIN_BITS.compare_exchange(
                0,
                raw_bits,
                AtomicOrdering::AcqRel,
                AtomicOrdering::Acquire,
            ) {
                Ok(_) => current = raw_bits,
                // CAS 失败=他者已锚定（竞争窗口内），读其值即可。
                Err(actual) => current = actual,
            }
        }
        let delta_ms = raw - f64::from_bits(current);
        if !(delta_ms >= 0.0) {
            panic!(
                "engine::clock 时间线倒退：raw={raw}ms origin={}ms——\
                 performance.now 单调性被破坏，拒绝静默夹逼（铁律二）",
                f64::from_bits(current)
            );
        }
        let delta_ns = delta_ms * 1_000_000.0;
        if delta_ns >= u64::MAX as f64 {
            panic!(
                "engine::clock 时间线纳秒溢出：{delta_ns}——协调者 timeOrigin 未锚定即跨实例取时"
            );
        }
        delta_ns as u64
    }

    /// 协调者共享时间线上的时刻（结构等价 native Instant 的 opaque 语义：
    /// Copy/Ord/算术——引擎调用面子集，见 clock_tests）。
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Instant {
        nanos: u64,
    }

    impl Instant {
        pub fn now() -> Instant {
            Instant {
                nanos: shared_line_nanos(),
            }
        }

        pub fn elapsed(&self) -> Duration {
            // std 同式：now() − self（自锚点起经过的时长）；时钟倒退显式 panic。
            Instant::now().duration_since(*self)
        }

        pub fn duration_since(&self, earlier: Instant) -> Duration {
            self.checked_duration_since(earlier).unwrap_or_else(|| {
                panic!(
                    "engine::clock duration_since 时间倒退（{} < {}）",
                    self.nanos, earlier.nanos
                )
            })
        }

        pub fn saturating_duration_since(&self, earlier: Instant) -> Duration {
            self.checked_duration_since(earlier)
                .unwrap_or(Duration::ZERO)
        }

        pub fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
            if self.nanos < earlier.nanos {
                return None;
            }
            Some(Duration::from_nanos(self.nanos - earlier.nanos))
        }

        pub fn checked_add(&self, duration: Duration) -> Option<Instant> {
            let extra = u64::try_from(duration.as_nanos()).ok()?;
            self.nanos.checked_add(extra).map(|nanos| Instant { nanos })
        }

        pub fn checked_sub(&self, duration: Duration) -> Option<Instant> {
            let sub = u64::try_from(duration.as_nanos()).ok()?;
            self.nanos.checked_sub(sub).map(|nanos| Instant { nanos })
        }
    }

    impl Add<Duration> for Instant {
        type Output = Instant;
        fn add(self, rhs: Duration) -> Instant {
            self.checked_add(rhs)
                .expect("engine::clock Instant + Duration 溢出——deadline 构造面不应溢出（铁律二：不静默回绕）")
        }
    }

    impl Sub<Duration> for Instant {
        type Output = Instant;
        fn sub(self, rhs: Duration) -> Instant {
            self.checked_sub(rhs)
                .expect("engine::clock Instant - Duration 下溢——调用面须先保证不减穿（铁律二）")
        }
    }

    impl Sub for Instant {
        type Output = Duration;
        fn sub(self, rhs: Instant) -> Duration {
            self.duration_since(rhs)
        }
    }
}

#[cfg(target_family = "wasm")]
pub use wasm_impl::Instant;

#[cfg(test)]
mod tests {
    use super::*;

    /// §0 合同：native 门面就是 std::time::Instant（同类型、非包装）。
    /// type_name 相等证明 re-export 未引入新类型/中间层。
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn native_门面即std_instant本体() {
        assert_eq!(
            std::any::type_name::<Instant>(),
            std::any::type_name::<std::time::Instant>()
        );
    }

    /// 引擎调用面使用的 API 子集（elapsed / 加 Duration / 比较 /
    /// saturating_duration_since，§6.1.2 A/B 档逐点勘定）在门面上等价可用。
    #[test]
    fn 门面覆盖引擎调用面的instant_api子集() {
        let started = Instant::now();
        std::thread::sleep(std::time::Duration::from_micros(50));
        assert!(started.elapsed() >= std::time::Duration::from_micros(1));
        assert!(Instant::now() >= started);
        let deadline = started + std::time::Duration::from_secs(1);
        assert!(
            deadline.saturating_duration_since(Instant::now()) <= std::time::Duration::from_secs(1)
        );
        assert_eq!(
            deadline.saturating_duration_since(deadline),
            std::time::Duration::ZERO
        );
    }

    /// 单调性：连续取时非递减（native 由 std 保证；wasm 由 performance.now
    /// 单调 + 常量 ORIGIN 保证——本测试在 wasm 无 runner，native 侧代言）。
    #[test]
    fn 连续取时单调非递减() {
        let mut previous = Instant::now();
        for _ in 0..64 {
            let current = Instant::now();
            assert!(current >= previous);
            previous = current;
        }
    }
}
