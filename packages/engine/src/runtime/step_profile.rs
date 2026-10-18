//! P0 分段画像探针（视图内生战役）：driver 线程一个 step 各段 wall 计时
//! 累积器。目的：把"六项可删"（Echo 链/双写/镜像安装/重建链）在本树上的
//! 实测占比从数据里量出来（Sol 终裁拒绝拍胸脯——要数据）。
//!
//! 设计约束：
//! - **零默认开销**：全局 `AtomicBool` 开关（Relaxed load）+ thread-local
//!   账本；未启用时 [`scope`] 返回 `None`，调用点 `let _g = ...` 零成本。
//! - 账本只活在**驱动器线程**（drive.rs 与 failure.rs 的段都在该线程被
//!   调用——thread-local 即驱动器账本）。
//! - 段名 `&'static str` 只用于诊断输出（engine 无字符串纪律的边界豁免
//!   ——不进业务/存档/hash）。
//! - 计时开销 ~2×Instant::now/段 ≈ 80ns；18 段/tick ≈ 1.5μs，B 口径
//!   ~3.2ms/tick 占比 0.05%（启用时）。

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// 分段清单（枚举序=代码执行序；详细语义见各插桩点注释）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepSegment {
    /// 裸引擎本体：world.run_ticks(1)（收束+安装，D 系列之外=对照基线）。
    WorldRun,
    /// 终轮成交 VWAP 计算（新持仓经历入场参考）。
    VwapCalc,
    /// 账户回声收集：dirty 标记 + 稀疏/全量账户快照上行（D1 EchoAccounts）。
    AcctEchoCollect,
    /// 账户镜像回写循环（session.positions 合并+现金写回）。
    AcctWriteback,
    /// 同拍成交公开投影（D3 apply_same_tick_fill_projection）。
    FillProjection,
    /// 事件组装（trade/lifecycle/auction/rejection 投影）。
    EventAssembly,
    /// 个人态回声：collect_group_belief + session 整态安装（D1 EchoBelief）。
    BeliefEcho,
    /// 首买经历 init 执行。
    HoldInits,
    /// 计划簿回声：collect_plans + 同拍执行腿补写（D1 EchoPlans + D3）。
    PlansEcho,
    /// 成交触达账户的个人记忆裁剪（D3）。
    MemoryPrune,
    /// 行情安装：市场态/价格历史/分钟线（D4 install_market_views 主体）。
    MarketInstall,
    /// attention 稀疏收集 + heap 全名册镜像重建（D6；install_market_views 尾段）。
    AttentionHeap,
    /// K 线安装（D4 install_candles）。
    CandleInstall,
    /// 连续拍 PriceTick 事件生成。
    PriceTickEmit,
    /// retail 拒收诊断/决策 trace 收集（D5）。
    RetailDiag,
    /// 信封账本：resting 收集 + 增量/全量重建（D6 EnvelopeLedger）。
    Envelope,
    /// desk 簿镜像 + lifecycle 投影清空（D6 DeskBookMirror）。
    DeskMirror,
    /// 日结段（ends_day：日界安装/world 日结/镜像重刷/补装）。
    DayEnd,
    /// 每拍安装基线快照（runtime_install_baseline：markets/价格历史/
    /// 分钟线/K 线历史+活动五域 clone——D5 安装-回滚链的快照面）。
    BaselineSnapshot,
    /// step 入口外围（锁取放/锁步校验/ingress freeze/意图路由等
    /// runtime_step_with_install_inner 之外的驱动器线程工作）。
    StepOuter,
    /// step_frame_equivalent 总段（=drive.rs step_with_capture 全部
    /// 子段 + 外围相位/写回；内嵌全部 drive 子段——二分定位用）。
    DriverStep,
    /// step 入口检查（check_step_start：require_healthy +
    /// has_missing_npc_strategy 的页级重扫面）。
    StepEntry,
    /// 驱动器锁+锁步校验+ingress freeze+意图路由（body 之前的共用段）。
    LockRoute,
    /// 刀③（协调者刀单③）：with_routed_runtime_driver 的 body 总段
    ///（=runtime_step_with_install 全部=driver_step+安装链+尾部）——
    /// 与 step_outer/lock_route 的差额=step() 内 body 外前置，二分
    /// 定位"未段化 ~106μs"归属（内联胶水 vs 前置路由）。
    StepViaBody,
    /// undo 槽清理（入口清+成功尾清；五域 ids/Vec clear）。
    UndoClear,
    /// belief 线程往返收集（collect_group_belief——嵌套在 belief_echo 内；
    /// 差额=安装循环）。
    BeliefCollect,
    /// resting 事实收集往返（collect_resting_facts——嵌套在 envelope 段内；
    /// 差额=差分+增量应用）。
    EnvelopeCollect,
}

impl StepSegment {
    pub const ALL: [Self; 27] = [
        Self::WorldRun,
        Self::VwapCalc,
        Self::AcctEchoCollect,
        Self::AcctWriteback,
        Self::FillProjection,
        Self::EventAssembly,
        Self::BeliefEcho,
        Self::HoldInits,
        Self::PlansEcho,
        Self::MemoryPrune,
        Self::MarketInstall,
        Self::AttentionHeap,
        Self::CandleInstall,
        Self::PriceTickEmit,
        Self::RetailDiag,
        Self::Envelope,
        Self::DeskMirror,
        Self::DayEnd,
        Self::BaselineSnapshot,
        Self::StepOuter,
        Self::DriverStep,
        Self::StepEntry,
        Self::LockRoute,
        Self::StepViaBody,
        Self::UndoClear,
        Self::BeliefCollect,
        Self::EnvelopeCollect,
    ];

    /// 段名（诊断输出用；ASCII 稳定键）。
    pub const fn name(self) -> &'static str {
        match self {
            Self::WorldRun => "world_run",
            Self::VwapCalc => "vwap_calc",
            Self::AcctEchoCollect => "acct_echo_collect",
            Self::AcctWriteback => "acct_writeback",
            Self::FillProjection => "fill_projection",
            Self::EventAssembly => "event_assembly",
            Self::BeliefEcho => "belief_echo",
            Self::HoldInits => "hold_inits",
            Self::PlansEcho => "plans_echo",
            Self::MemoryPrune => "memory_prune",
            Self::MarketInstall => "market_install",
            Self::AttentionHeap => "attention_heap",
            Self::CandleInstall => "candle_install",
            Self::PriceTickEmit => "price_tick_emit",
            Self::RetailDiag => "retail_diag",
            Self::Envelope => "envelope",
            Self::DeskMirror => "desk_mirror",
            Self::DayEnd => "day_end",
            Self::BaselineSnapshot => "baseline_snapshot",
            Self::StepOuter => "step_outer",
            Self::DriverStep => "driver_step",
            Self::StepEntry => "step_entry",
            Self::LockRoute => "lock_route",
            Self::StepViaBody => "step_via_body",
            Self::UndoClear => "undo_clear",
            Self::BeliefCollect => "belief_collect",
            Self::EnvelopeCollect => "envelope_collect",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// 全局开关（探针入口 enable；生产恒 false）。
static ENABLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// 各段累计 ns + 进入次数（下标=StepSegment::index）。
    static LEDGER: RefCell<Option<([u128; 27], [u64; 27])>> = const { RefCell::new(None) };
}

/// 在当前线程启用分段画像（重复调用=账本重置）。
pub fn enable_on_this_thread() {
    LEDGER.with_borrow_mut(|slot| *slot = Some(([0; 27], [0; 27])));
    ENABLED.store(true, Ordering::Relaxed);
}

/// 关闭并取走账本（段名、累计 ns、进入次数；未启用/无数据=None——
/// 显式返回不静默）。
pub fn take_ledger() -> Option<Vec<(&'static str, u128, u64)>> {
    ENABLED.store(false, Ordering::Relaxed);
    LEDGER.with_borrow_mut(|slot| {
        slot.take().map(|(nanos, counts)| {
            StepSegment::ALL
                .into_iter()
                .enumerate()
                .map(|(i, seg)| (seg.name(), nanos[i], counts[i]))
                .collect()
        })
    })
}

/// 计时段 guard：构造记起点，Drop 累计。未启用返回 None（零成本）。
pub struct SegmentGuard {
    segment: StepSegment,
    started: Instant,
}

impl Drop for SegmentGuard {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed().as_nanos();
        let seg = self.segment;
        LEDGER.with_borrow_mut(|slot| {
            if let Some((nanos, counts)) = slot.as_mut() {
                nanos[seg.index()] += elapsed;
                counts[seg.index()] += 1;
            }
        });
    }
}

/// 进入一个计时段（未启用时 None）。用法：`let _g = scope(SEG); …;`
pub fn scope(segment: StepSegment) -> Option<SegmentGuard> {
    if !ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    // 开关开而账本缺（别的线程 enable）：本线程未启用——不计时。
    let active = LEDGER.with_borrow(|slot| slot.is_some());
    if !active {
        return None;
    }
    Some(SegmentGuard {
        segment,
        started: Instant::now(),
    })
}
