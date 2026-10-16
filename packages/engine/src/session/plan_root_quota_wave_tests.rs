//! 缺口②增量二十——root 侧跨 wave/失败传播/死区验收（Astra+Luna 三审最小
//! 剩余集：Drop/send 竞态关闭+task_died 已保存 payload、a1 注入移真实事务
//! 中途、cross-wave 真实 pending、p3 对象身份与绑定顺序、普通测试 10s 硬限）。
//!
//! 覆盖：r4/r5 容量握手与 4c、cross-wave 确定性 pending（观察者闩+持槽钉住，
//! 不再依赖时序）、a1 整事务失败原子性（StepFatal=post-shadow 注入点=事务
//! 已完整运行后；panic=注入 root 沿真实事务 unwind；两形态均以失败前权威
//! 状态/事件为基线零漂移断言）、P1-① Drop 排空协议（已入队/已保存/迟到
//! 三形态，确定性握手）、e1/e2 operations 层死区（coordinator 级）。
//! ready_stock_stream 级 e1/e2 AccountValidation 全拒集成见
//! `pipeline/plan_quota_dead_zone_tests.rs`。

use super::plan_chain_candidates::{PrepareReady, RootPanicToken};
use super::plan_chain_candidates_tests::{
    execution_fixture_with_compute, execution_fixture_with_compute_and_insts,
};
use crate::AccountId;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// 普通测试挂起保护 5s（项目纪律单 case 硬上限 10s 的内部预算——四审：
/// M6 负向红证走「挂起超时断言」路径，10s 预算会使 case 总时长 10.2s 超限；
/// 收紧到 5s 后红证 case ≈5.2s，绿路径全部 <1s 不受影响）。
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

/// 构造 root 批（两个机构账户；不建计划，仅驱动观察批）。
fn two_root_batch_single(
    session: &crate::GameSession,
) -> super::plan_chain_candidates::PlanChainOperationBatch {
    let market = session.build_market_view();
    super::plan_chain_candidates::PlanChainOperationBatch::accounts(
        vec![AccountId(1)],
        market,
        Default::default(),
        Default::default(),
        crate::calendar::CivilInstant::new(
            crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
            0,
        )
        .unwrap(),
        Default::default(),
    )
}

/// 专用 3-worker 小池：隔离并行全量下共享默认池（128 worker）的环境占槽/
/// 释放竞态——wave 系用例的容量形态由此确定化（q3 根因登记于增量十九 index）。
fn dedicated_compute() -> Arc<crate::compute::ComputeContext> {
    crate::compute::ComputeContext::with_workers(3).unwrap()
}

fn two_account_session() -> crate::GameSession {
    two_account_session_with(dedicated_compute())
}

fn two_account_session_with(compute: Arc<crate::compute::ComputeContext>) -> crate::GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 2;
    crate::GameSession::new_with_compute(setup, 61, compute).unwrap()
}

fn two_root_batch(
    session: &crate::GameSession,
) -> super::plan_chain_candidates::PlanChainOperationBatch {
    let market = session.build_market_view();
    super::plan_chain_candidates::PlanChainOperationBatch::accounts(
        vec![AccountId(1), AccountId(2)],
        market,
        Default::default(),
        Default::default(),
        crate::calendar::CivilInstant::new(
            crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
            0,
        )
        .unwrap(),
        Default::default(),
    )
}

/// 观察者闩（确定性握手，两相位）：phase0=全部进入 bound 的 root 阻塞在
/// 观察点；phase1=第 1 个放行、第 2 个起仍阻塞；phase2=全放行。观察者为
/// per-ComputeContext（专用池），不污染其他用例。
/// 进入序≠派发序（负载下 bound 启动顺序可交换）——凡依赖「哪个 root 先
/// 进入」的形态，必须以容量串行化派发（持槽使 root1 先行独占），再以相位
/// 放行；不得按进入序号假设账户身份。
type RootGate = Arc<(Mutex<u8>, Condvar)>;

fn install_root_entry_gate(
    compute: &Arc<crate::compute::ComputeContext>,
) -> (RootGate, Arc<AtomicUsize>) {
    let gate: RootGate = Arc::new((Mutex::new(0), Condvar::new()));
    let entered = Arc::new(AtomicUsize::new(0));
    let observer_gate = Arc::clone(&gate);
    let observer_entered = Arc::clone(&entered);
    compute.set_observer(Arc::new(move |observation| {
        if observation.work != crate::compute::ComputeWork::PlanRoot {
            return;
        }
        let ordinal = observer_entered.fetch_add(1, Ordering::SeqCst) + 1;
        let (phase, signal) = &*observer_gate;
        let mut phase = phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while (ordinal == 1 && *phase == 0) || (ordinal >= 2 && *phase < 2) {
            phase = match signal.wait(phase) {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }));
    (gate, entered)
}

/// 放行到指定相位（1=仅第 1 个 root；2=全部）。
fn release_roots_to(gate: &RootGate, level: u8) {
    let (phase, signal) = &**gate;
    let mut phase = phase
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *phase = level;
    signal.notify_all();
}

fn release_roots(gate: &RootGate) {
    release_roots_to(gate, 2);
}

/// 派发门（两段放行，测试/验证专用原语 set_dispatch_gate 的编排封装）：
/// 派发点第 1 次调用（FIFO 首项）放行；第 2 次起阻塞，直至
/// [`release_serial_dispatch_to`] 放行到对应层级。派发门运行在协调者线程
/// （reserve 成功后、spawn 前）——阻塞它即冻结后续派发，形成确定性派发序。
type SerialDispatchGate = Arc<(Mutex<u8>, Condvar, AtomicUsize)>;

fn install_serial_dispatch_gate(
    compute: &Arc<crate::compute::ComputeContext>,
) -> SerialDispatchGate {
    let gate: SerialDispatchGate = Arc::new((Mutex::new(0), Condvar::new(), AtomicUsize::new(0)));
    let observer_gate = Arc::clone(&gate);
    compute.set_dispatch_gate(Arc::new(move || {
        let ordinal = observer_gate.2.fetch_add(1, Ordering::SeqCst) + 1;
        let (phase, signal, _) = &*observer_gate;
        let mut phase = phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // 层级语义：phase=P 时仅放行前 P+1 项（ordinal ≤ P+1）——逐项串行放行。
        while ordinal > usize::from(*phase) + 1 {
            phase = match signal.wait(phase) {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }));
    gate
}

/// 派发门已放行层数（≥n 表示第 n 项已通过派发点）。
fn dispatch_gate_calls(gate: &SerialDispatchGate) -> usize {
    gate.2.load(Ordering::SeqCst)
}

/// 放行派发门到指定层级（level=P=放行前 P+1 项：1 允许第 2 项、2 全放）。
fn release_serial_dispatch_to(gate: &SerialDispatchGate, level: u8) {
    let (phase, signal, _) = &**gate;
    let mut phase = phase
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *phase = level;
    signal.notify_all();
}

/// 闩自动释放 guard：本 guard 析构（含 panic unwind）时先放行全部被闩 root，
/// 再轮到其后析构的协调者 Drop 排空——保证「测试失败→批销毁」路径不会因
/// 被间 root 永不发送而挂死（失败以断言红呈现，而非挂起）。
/// 声明顺序约束：必须**晚于** batch 声明（Rust 逆序析构 ⇒ 本 guard 先于
/// batch Drop）。
struct GateReleaser(RootGate);

impl Drop for GateReleaser {
    fn drop(&mut self) {
        // unwind 中锁可能被 poison：容忍并继续放行（放行是幂等置位）。
        let (phase, signal) = &*self.0;
        let mut phase = phase
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *phase = 2;
        signal.notify_all();
    }
}

/// 有界等待谓词成立（确定性握手的等待侧；10s 硬上限）。
fn wait_until(condition: &str, mut predicate: impl FnMut() -> bool) {
    let wait = deadline();
    while !predicate() {
        assert!(Instant::now() < wait, "等待超时（挂死即缺陷）：{condition}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// guard 终态归还（标记到达≠guard 已析构，以最终态为准；设计 §3.1④ 三审③）。
fn assert_guards_returned(compute: &Arc<crate::compute::ComputeContext>) {
    let wait = deadline();
    while compute.admission_snapshot() != (0, 0) {
        assert!(
            Instant::now() < wait,
            "guard 必须最终归还（admission={:?}）",
            compute.admission_snapshot()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 重抛 payload 的**对象身份**判等（四审直证：marker Arc 判等可被
/// 「clone marker+rebox」伪造；改为**同地址判等**——注入点把 panic_any 所装
/// Box 的元素地址登记到 ROOT_PANIC_TOKEN_ADDR，此处比较重抛对象的实际地址：
/// 任何重建对象地址必不同，证明传播的就是原 payload 对象本身）。
fn assert_token_identity(
    payload: &Box<dyn std::any::Any + Send>,
    expected_account: AccountId,
    case: &str,
) {
    let token = payload
        .downcast_ref::<Box<RootPanicToken>>()
        .unwrap_or_else(|| panic!("{case}: 重抛必须是原 Box<RootPanicToken> 对象"));
    assert_eq!(token.account, expected_account, "{case}: 账户身份");
    let expected = super::plan_chain_candidates::root_panic_token_addr()
        .unwrap_or_else(|| panic!("{case}: 注入地址登记缺失"));
    assert_eq!(
        &**token as *const RootPanicToken as usize,
        expected,
        "{case}: 对象身份（同地址判等，非同内容重建）"
    );
}

/// R1-c 消费优先（增量二十八红证）：root1 的结果**已现成**（发布完成、
/// 在队列中）且 root2 因容量受限留在 pending——阻塞拉取必须**先消费现成
/// 结果并返回交付**，不得被「循环顶补派发」阻塞。旧序（每轮先补派发后
/// 消费）下，本轮派发 root2 会撞上测试派发门（在派发点阻塞协调者线程）
/// → 现成结果迟迟不被消费 → returned_within_grace=false → 红；新序
/// （每轮先消费，零在飞+pending 非空时才在 4c 判定前尽力派发）下立即
/// 返回 Delivered → 绿。

/// r4（池外容量握手）+P1-①回归：池外协调者容量受阻上提 PendingCapacity；
/// 容量恢复后**重入必须补派发 pending 直至 Complete**——增量十七缺陷形态
/// （重入不补派发→永久空转）由本测试钉死（负向 mutation：移除 prepare 内
/// dispatch_ready_accounts 补派发调用 → 本测试超时/不 Complete 红）。
#[test]
fn r4_offpool_capacity_signal_then_redispatch_drains_to_complete() {
    let mut session = two_account_session();
    let compute = session.compute.clone();
    let mut batch = two_root_batch(&session);
    // 池外钉满账：测试侧占满全部 spawn 槽（无 drive 登记）。
    let mut held = Vec::new();
    while let Some(guard) = compute.reserve_spawn_slot() {
        held.push(guard);
    }
    // 专用池确定性：抓取后账面必满（无环境占槽）。
    let (drives, spawned) = compute.admission_snapshot();
    assert!(
        drives + spawned >= compute.workers(),
        "专用池持满前提：drives+spawned≥workers（实际 {drives}+{spawned}）"
    );
    let mut signals = 0usize;
    let mut delivered = 0usize;
    let wait = deadline();
    let drained = loop {
        match batch.prepare_ready_accounts(&mut session, true).unwrap() {
            // 池外容量上提：等待恢复后重入（重入从补派发起）。
            PrepareReady::PendingCapacity => {
                signals += 1;
                assert!(Instant::now() < wait, "容量等待不得死等");
                // 释放一个槽模拟他人 finite 完成；仅释放一次，剩余重入必须经
                // 补派发+消费自然推进到 Complete。
                if !held.is_empty() {
                    held.pop();
                }
                compute.wait_for_spawn_capacity();
            }
            PrepareReady::Progress(true) => delivered += 1,
            PrepareReady::Progress(false) => {
                // roots 排空（source=Empty）后的稳态回复——本层终止信号
                //（operations/routes 残留属 adaptive 层消费域）。
                break true;
            }
        }
        assert!(Instant::now() < wait, "推进不得空转挂起");
    };
    assert!(drained, "必须推进至 Complete（roots 全排空）");
    assert_eq!(delivered, 2, "两 root 都必须经补派发交付");
    assert!(
        signals >= 1,
        "满账钉住成立时池外容量受阻必须产生至少一次 PendingCapacity 上提"
    );
    drop(held);
    assert_guards_returned(&compute);
}

/// r5 重写（真正命中 4c）：**drive 槽占满**（测试登记满 drives + spawn 槽占满
/// workers−drives）后池内协调者零在飞+pending → 只能 4c 内联（reserve 恒拒）。
/// 满账 + 下一轮释放一个槽 → 重入后补派发必须立即派发（或此前已内联）→
/// 有限完成。
#[test]
fn r5_true_capacity_pressure_inline_or_late_dispatch_completes() {
    let mut session = two_account_session();
    let compute = session.compute.clone();
    if compute.workers() <= 1 {
        return; // 单 worker 池无台账路径（round5-③）。
    }
    let mut batch = two_root_batch(&session);
    // 池内形态：先登记满 drive（try_enter 在测试线程调用即可占数），再占满
    // spawn 槽 → drives+spawned == workers：任何 reserve/try_enter 恒拒。
    let mut drives = Vec::new();
    while let Some(slot) = compute.try_enter_pool_drive_slot() {
        drives.push(slot);
    }
    let mut held = Vec::new();
    while let Some(guard) = compute.reserve_spawn_slot() {
        held.push(guard);
    }
    let (drives_count, spawned_count) = compute.admission_snapshot();
    assert_eq!(
        drives_count + spawned_count,
        compute.workers(),
        "钉满账前提：drives+spawned==workers（专用池确定性账面）"
    );
    // 满账期间在池内推进：4b 无在飞可等 → 4c 内联是唯一路径。断言实际
    // 交付形态（Progress(true)=内联 root 的观察完成并 install；Drives 常驻
    // 期间 spawned 计数不得增长——reserve 恒拒由联合满账前提保证）。
    let mut delivered = 0usize;
    let wait = deadline();
    loop {
        match compute.install(|| batch.prepare_ready_accounts(&mut session, true)) {
            Ok(PrepareReady::Progress(true)) => delivered += 1,
            Ok(PrepareReady::Progress(false)) => break,
            Ok(PrepareReady::PendingCapacity) => {
                panic!("池内协调者不应产生 PendingCapacity（应 4c 内联）")
            }
            Err(fatal) => panic!("4c 内联推进不得失败：{fatal}"),
        }
        assert!(
            compute.admission_snapshot().0 + compute.admission_snapshot().1 <= compute.workers(),
            "满账期间 spawned 不得增长"
        );
        assert!(Instant::now() < wait, "4c 内联推进不得挂起");
    }
    assert!(delivered >= 1, "4c 内联必须实际交付至少一个 root（非空转 Ok）");
    drop(held);
    drop(drives);
    let _ = batch;
}

/// 跨 wave 续行（增量二十重写；Astra 三审「先构造并断言真实 pending 跨调用」）：
/// 确定性握手 = 观察者闩（root1 在 bound 观察点闩住）+ 持 2/3 spawn 槽。
/// 形态钉死：首轮只派发 root1（pending FIFO 首个），root2 留 pending 跨调用
/// 存续（bound 进入计数=1 且账面恒满证明未派发、未完成）；放行 root1 后其
/// 现成结果先交付（channel FIFO：root1 的 send 先于 root2），不被 root2/
/// pending 阻挡；随后 root2 派发交付至 Complete。
/// 负向红证（mutation M6）：移除 prepare 内补派发 → root2 永不派发 → 红。

/// personal 恰一次 install + 同一 observation 贯穿：以计划权威状态推进的
/// 幂等性承载（重复 install 会破坏计划账本不变量并立即暴露）。
#[test]
fn personal_installs_exactly_once_per_root() {
    let mut session = two_account_session();
    let compute = session.compute.clone();
    let plans_before = serde_json::to_value(&session.state.plans).unwrap();
    let mut batch = two_root_batch(&session);
    let wait = deadline();
    let mut delivered = 0usize;
    loop {
        match batch.prepare_ready_accounts(&mut session, true).unwrap() {
            PrepareReady::Progress(true) => delivered += 1,
            PrepareReady::Progress(false) => break,
            PrepareReady::PendingCapacity => compute.wait_for_spawn_capacity(),
        }
        assert!(Instant::now() < wait, "推进不得挂起");
    }
    assert_eq!(delivered, 2, "两 root 恰各 install 一次");
    // install 的可见承载=personal state 返还权威（无计划账户的 plans 本就不变，
    // 断言对象改为 observation 复核时点——恰一次 install 已由 delivered==2 与
    // 权威账本不变量（如重复 install 会破坏 BTreeMap 键唯一性并立即暴露）承载。
    let plans_after = serde_json::to_value(&session.state.plans).unwrap();
    assert_eq!(
        plans_before, plans_after,
        "无计划账户的 root 观察不得改动 plans（恰一次 install 不重复）"
    );
}

/// a1（panic 形态·真实事务中途，增量二十重写）：注入 root 沿**真实市场事务**
/// （plan_tick → 市场事务 → CommitTick 准备）unwind——失败前权威状态为基线：
/// 业务哈希/事件零部分提交（F 的 install 全在 tick shadow，随失败整链丢弃，
/// ADR-0017：CommitTick 外不修改权威状态——增量十九注释「失败前单点提交
/// 留存」口径有误，本用例纠正）、personal 不复活、payload 对象身份、
/// guard 终态归还。
/// 红证：对象身份断言在增量十九 String payload 下红（mutation M2 复现）。
#[test]
#[ignore = "口径=串行独立运行：root panic 注入为进程级全局静态，与并行用例的 root 任务交叉污染；node scripts/run-engine-lib-tests.mjs normal --ignored --bin <bin> plan_root_quota_wave_tests"]
fn a1_panic_form_atomicity_on_real_transaction() {
    // F-first 编排（六审：账户保持锚点=**F 的 Prepared 发布**；无持槽、无
    // ordinal 门——消除 ordinal 错位类）：panic 注入点配置「等待 AccountId(1)
    // 的 Prepared 发布 flag」——F 自由执行并在 finish 的 send(Prepared) 成功
    // 后置位（保持位），panic 才发生 ⇒ TaskDied 的发送确定晚于 F 的 Prepared
    // 发送 ⇒ FIFO ⇒ 协调者先消费 F 的 Prepared（install 进 shadow）再收
    // TaskDied——「F 已发布且先于失败被消费」全程结构保证。
    let (mut session, _) = execution_fixture_with_compute(dedicated_compute());
    let compute = session.compute.clone();
    let business_before = session.business_state_hash().unwrap();
    let plans_before = serde_json::to_value(&session.state.plans).unwrap();
    let roots = two_root_batch(&session);
    // 批次隔离（七轮三小点）：开始即清空 publish 保持集。
    super::plan_chain_candidates::clear_plan_root_published();
    super::plan_chain_candidates::set_root_panic_accounts(&[AccountId(2)]);
    super::plan_chain_candidates::set_root_panic_wait_published(Some(AccountId(1)));
    let f_published = Arc::new(AtomicBool::new(false));
    {
        let published = Arc::clone(&f_published);
        super::plan_chain_candidates::set_plan_root_prepared_probe(Some(Arc::new(
            move |account| {
                if account == AccountId(1) {
                    published.store(true, Ordering::SeqCst);
                }
            },
        )));
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::pipeline::run_injected_roots_market_tick_for_test(&mut session, roots)
    }));
    super::plan_chain_candidates::set_root_panic_accounts(&[]);
    super::plan_chain_candidates::set_root_panic_wait_published(None);
    super::plan_chain_candidates::set_plan_root_prepared_probe(None);
    super::plan_chain_candidates::clear_plan_root_published();
    let payload = outcome.expect_err("root panic 必须沿真实事务 unwind 显式传播（不吞不挂死）");
    assert_token_identity(&payload, AccountId(2), "a1 panic 形态");
    assert!(
        f_published.load(Ordering::SeqCst),
        "F 发布保持锚点：F 的 Prepared 已 send 成功（panic 等待该 flag 后才发生，TaskDied 晚于 F 发布）"
    );
    // 权威零部分提交：F 的 install 在 tick shadow 内，随事务 unwind 整链
    // 丢弃——权威与失败前（=tick 前）完全一致。
    assert_eq!(
        session.business_state_hash().unwrap(),
        business_before,
        "panic 事务零部分提交（业务/事件与失败前基线一致）"
    );
    assert_eq!(
        serde_json::to_value(&session.state.plans).unwrap(),
        plans_before,
        "失败 tick 的 personal state 不复活进计划权威"
    );
    assert_guards_returned(&compute);
    assert_eq!(
        session.business_state_hash().unwrap(),
        business_before,
        "迟到结果不得 install（终态仍零漂移）"
    );
}

/// 三级门辅助：放行 ordinal1（level=1）/ 全放（level=2）。
fn release_gate_to(gate: &Arc<(Mutex<u8>, Condvar)>, level: u8) {
    let (phase, signal) = &**gate;
    let mut phase = phase
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if *phase < level {
        *phase = level;
    }
    signal.notify_all();
}

/// a1 pending 形态（真实事务，增量二十一）：roots FIFO=[panic(2), F(1)]，
/// 持 2/3 槽——panic root 独占先派发（ordinal1=panic）→TaskDied 到达时
/// F 仍 pending（从未进入 bound：entered 终值==1 即形态证明）→ 协调者
/// remaining==0+pending 冻结 → 重抛 → unwind；权威零部分提交。
#[test]
#[ignore = "口径=串行独立运行：root panic 注入为进程级全局静态（交叉污染）；node scripts/run-engine-lib-tests.mjs normal --ignored --bin <bin> plan_root_quota_wave_tests"]
fn a1_panic_pending_form_real_transaction() {
    // 真实未派发 pending（六审：撤回「结构不可构造」声明，按审查指认构造）：
    // roots FIFO=[panic(2), F(1)]，**F ∈ 派发保持集**（dispatch 点 reserve 成功
    // 后命中保持集 → 推回 pending 前端并归还预留——与 reserve 失败同分支，
    // 模拟「该账户任务容量不可用」的冻结形态）；panic 自由派发并 panic →
    // TaskDied；协调者消费轮 loop-top 的 F 派发被保持集推回 → try_recv 收
    // TaskDied（panic 唯一在飞，remaining==0）→ **pending 非空时立即重抛**
    // ——F 从未派发、从未进入 bound（账户入口探针集合断言）、其 personal
    // 随批丢弃（零漂移断言）。
    let (mut session, _) = execution_fixture_with_compute(dedicated_compute());
    let compute = session.compute.clone();
    let business_before = session.business_state_hash().unwrap();
    let plans_before = serde_json::to_value(&session.state.plans).unwrap();
    let market = session.build_market_view();
    let roots = super::plan_chain_candidates::PlanChainOperationBatch::accounts(
        vec![AccountId(2), AccountId(1)],
        market,
        Default::default(),
        Default::default(),
        crate::calendar::CivilInstant::new(
            crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
            0,
        )
        .unwrap(),
        Default::default(),
    );
    super::plan_chain_candidates::set_root_panic_accounts(&[AccountId(2)]);
    super::plan_chain_candidates::set_root_dispatch_hold_accounts(&[AccountId(1)]);
    let entered_accounts: Arc<Mutex<std::collections::BTreeSet<AccountId>>> =
        Arc::new(Mutex::new(std::collections::BTreeSet::new()));
    {
        let entered = Arc::clone(&entered_accounts);
        super::plan_chain_candidates::set_plan_root_account_entry_probe(Some(Arc::new(
            move |account| {
                entered
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(account);
            },
        )));
    }
    let taskdied_received = Arc::new(AtomicBool::new(false));
    {
        let received = Arc::clone(&taskdied_received);
        super::plan_chain_candidates::set_plan_root_taskdied_probe(Some(Arc::new(
            move || {
                received.store(true, Ordering::SeqCst);
            },
        )));
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::pipeline::run_injected_roots_market_tick_for_test(&mut session, roots)
    }));
    super::plan_chain_candidates::set_root_panic_accounts(&[]);
    super::plan_chain_candidates::set_root_dispatch_hold_accounts(&[]);
    super::plan_chain_candidates::set_plan_root_account_entry_probe(None);
    super::plan_chain_candidates::set_plan_root_taskdied_probe(None);
    let payload = outcome.expect_err("真实未派发 pending 形态的 panic 必须显式传播");
    assert_token_identity(&payload, AccountId(2), "a1 pending 形态");
    assert!(
        taskdied_received.load(Ordering::SeqCst),
        "TaskDied 接收探针必触发（重抛发生在接收当场）"
    );
    let entered = entered_accounts
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert_eq!(
        entered,
        std::collections::BTreeSet::from([AccountId(2)]),
        "真实未派发证明：仅 panic 账户进入过 bound——F 从未派发（pending 冻结分支）"
    );
    assert_eq!(
        session.business_state_hash().unwrap(),
        business_before,
        "真实未派发 pending 形态零部分提交"
    );
    assert_eq!(
        serde_json::to_value(&session.state.plans).unwrap(),
        plans_before,
        "真实未派发 pending 形态 personal 不复活"
    );
    assert_guards_returned(&compute);
}

/// 在飞+迟到形态（增量二十八：新序 R1-c 下重写；零线程诊断探针见
/// 注入点等 F(1) 发布（保持锚点）；other(3) 入口门等「失败后 4b」（条件化
/// 探针，task_died 已存才触发）；协调者：消费 F 的 Prepared（install）→
/// 收 TaskDied（other 在飞，remaining=1）→ 存字段 → 失败后 4b → 放行
/// other → 迟到 Prepared 丢弃不 install → 收满 → 重抛。
#[test]
#[ignore = "口径=串行独立运行：同 a1_panic_form（全局注入交叉污染）；node scripts/run-engine-lib-tests.mjs normal --ignored --bin <bin> plan_root_quota_wave_tests"]
fn a1_panic_inflight_late_form_real_transaction() {
    let wide = crate::compute::ComputeContext::with_workers(8).unwrap();
    let (mut session, _) = execution_fixture_with_compute_and_insts(wide, 3);
    let compute = session.compute.clone();
    let business_before = session.business_state_hash().unwrap();
    let plans_before = serde_json::to_value(&session.state.plans).unwrap();
    let market = session.build_market_view();
    let roots = super::plan_chain_candidates::PlanChainOperationBatch::accounts(
        vec![AccountId(1), AccountId(2), AccountId(3)],
        market,
        Default::default(),
        Default::default(),
        crate::calendar::CivilInstant::new(
            crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
            0,
        )
        .unwrap(),
        Default::default(),
    );
    super::plan_chain_candidates::clear_plan_root_published();
    super::plan_chain_candidates::set_root_panic_accounts(&[AccountId(2)]);
    super::plan_chain_candidates::set_root_panic_wait_published(Some(AccountId(1)));
    let f_published = Arc::new(AtomicBool::new(false));
    {
        let published = Arc::clone(&f_published);
        super::plan_chain_candidates::set_plan_root_prepared_probe(Some(Arc::new(
            move |account| {
                if account == AccountId(1) {
                    published.store(true, Ordering::SeqCst);
                }
            },
        )));
    }
    let other_gate: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
    let fourb_after_failure = Arc::new(AtomicBool::new(false));
    let early_4b = Arc::new(AtomicBool::new(false));
    let taskdied_received = Arc::new(AtomicBool::new(false));
    {
        let gate_for_entry = Arc::clone(&other_gate);
        super::plan_chain_candidates::set_plan_root_account_entry_probe(Some(Arc::new(
            move |account| {
                if account != AccountId(3) {
                    return;
                }
                let (open, signal) = &*gate_for_entry;
                let mut open = open
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*open {
                    open = match signal.wait(open) {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                }
            },
        )));
        let received = Arc::clone(&taskdied_received);
        super::plan_chain_candidates::set_plan_root_taskdied_probe(Some(Arc::new(
            move || {
                received.store(true, Ordering::SeqCst);
            },
        )));
        let fourb = Arc::clone(&fourb_after_failure);
        let early = Arc::clone(&early_4b);
        let gate_for_4b = Arc::clone(&other_gate);
        let received_for_4b = Arc::clone(&taskdied_received);
        super::plan_chain_candidates::set_plan_root_4b_probe(Some(Arc::new(move || {
            if !received_for_4b.load(Ordering::SeqCst) {
                early.store(true, Ordering::SeqCst);
            }
            fourb.store(true, Ordering::SeqCst);
            let (open, signal) = &*gate_for_4b;
            let mut open = open
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *open = true;
            signal.notify_all();
        })));
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::pipeline::run_injected_roots_market_tick_for_test(&mut session, roots)
    }));
    super::plan_chain_candidates::set_root_panic_accounts(&[]);
    super::plan_chain_candidates::set_root_panic_wait_published(None);
    super::plan_chain_candidates::set_plan_root_prepared_probe(None);
    super::plan_chain_candidates::set_plan_root_account_entry_probe(None);
    super::plan_chain_candidates::set_plan_root_taskdied_probe(None);
    super::plan_chain_candidates::set_plan_root_4b_probe(None);
    super::plan_chain_candidates::clear_plan_root_published();
    let payload = outcome.expect_err("在飞+迟到形态的 panic 必须显式传播");
    assert_token_identity(&payload, AccountId(2), "a1 在飞+迟到 形态");
    assert!(
        f_published.load(Ordering::SeqCst),
        "F 发布保持锚点：F 的 Prepared 先于 TaskDied 发送（panic 等待该 flag）"
    );
    assert!(
        taskdied_received.load(Ordering::SeqCst),
        "TaskDied 接收保持位"
    );
    assert!(
        !early_4b.load(Ordering::SeqCst),
        "失败后语义：4b 探针触发时刻 TaskDied 必已接收"
    );
    assert!(
        fourb_after_failure.load(Ordering::SeqCst),
        "失败后 4b 分支确经（真实事务层）——other 保持至该分支之后才发送"
    );
    assert_eq!(
        session.business_state_hash().unwrap(),
        business_before,
        "在飞+迟到形态零部分提交"
    );
    assert_eq!(
        serde_json::to_value(&session.state.plans).unwrap(),
        plans_before,
        "迟到 Prepared 不得 install/personal 不复活"
    );
    assert_guards_returned(&compute);
}



/// 失败后 4b 确定性证据（六审分支锚点+七轮强化：**other-first FIFO 编排**与
/// early-4b 断言）：roots FIFO=[other(1), panic(2)]——other 在账户入口门
/// 等待（**失败后 4b** flag，由条件化 4b 探针在
/// task_died 已存时置位）；panic 的入口探针调用在锁修复后不被 other 的等待阻塞（M10 回退
/// 锁形态时存在 panic 被阻塞的交错→锁环→超时红；并行派发顺序不保证
/// 执行先后，红证是存在性的而非每次必然）。
/// panic→TaskDied→协调者收（remaining=1，other 在飞）→存字段→Empty→
/// **失败后 4b**→放行 other→迟到 Prepared 丢弃→收满→重抛。
/// early-4b 断言：4b 探针触发时刻 taskdied_received 必已置位（M11 条件
/// 移除后普通空等待提前触发→early→红）。
#[test]
#[ignore = "口径=串行独立运行：同 a1_panic_form（全局注入交叉污染）；node scripts/run-engine-lib-tests.mjs normal --ignored --bin <bin> plan_root_quota_wave_tests"]
fn a1_4b_wait_branch_is_actually_entered() {
    let mut session = two_account_session();
    let compute = session.compute.clone();
    let market = session.build_market_view();
    let mut batch = super::plan_chain_candidates::PlanChainOperationBatch::accounts(
        vec![AccountId(1), AccountId(2)],
        market,
        Default::default(),
        Default::default(),
        crate::calendar::CivilInstant::new(
            crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
            0,
        )
        .unwrap(),
        Default::default(),
    );
    // FIFO=[other(1), panic(2)]：other-first 派发顺序，执行先后无保证
    //（与 M10 存在性口径一致）；panic=AccountId(2)。
    super::plan_chain_candidates::clear_plan_root_published();
    super::plan_chain_candidates::set_root_panic_accounts(&[AccountId(2)]);
    let other_gate: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
    let fourb_after_failure = Arc::new(AtomicBool::new(false));
    let early_4b = Arc::new(AtomicBool::new(false));
    let taskdied_received = Arc::new(AtomicBool::new(false));
    {
        let gate_for_entry = Arc::clone(&other_gate);
        super::plan_chain_candidates::set_plan_root_account_entry_probe(Some(Arc::new(
            move |account| {
                if account != AccountId(1) {
                    return;
                }
                let (open, signal) = &*gate_for_entry;
                let mut open = open
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*open {
                    open = match signal.wait(open) {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                }
            },
        )));
        let received = Arc::clone(&taskdied_received);
        super::plan_chain_candidates::set_plan_root_taskdied_probe(Some(Arc::new(
            move || {
                received.store(true, Ordering::SeqCst);
            },
        )));
        let fourb = Arc::clone(&fourb_after_failure);
        let early = Arc::clone(&early_4b);
        let gate_for_4b = Arc::clone(&other_gate);
        let received_for_4b = Arc::clone(&taskdied_received);
        super::plan_chain_candidates::set_plan_root_4b_probe(Some(Arc::new(move || {
            if !received_for_4b.load(Ordering::SeqCst) {
                // early-4b 形态（M11 下出现）：记录，用例断言红。
                early.store(true, Ordering::SeqCst);
            }
            fourb.store(true, Ordering::SeqCst);
            let (open, signal) = &*gate_for_4b;
            let mut open = open
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *open = true;
            signal.notify_all();
        })));
    }
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let wait = deadline();
        loop {
            match batch.prepare_ready_accounts(&mut session, true) {
                Ok(PrepareReady::Progress(_)) => {}
                Ok(PrepareReady::PendingCapacity) => compute.wait_for_spawn_capacity(),
                Err(fatal) => panic!("prepare 失败：{fatal}"),
            }
            if Instant::now() > wait {
                panic!("推进超时（挂死即缺陷）");
            }
        }
    }))
    .expect_err("panic 必须显式传播");
    super::plan_chain_candidates::set_root_panic_accounts(&[]);
    super::plan_chain_candidates::set_plan_root_account_entry_probe(None);
    super::plan_chain_candidates::set_plan_root_taskdied_probe(None);
    super::plan_chain_candidates::set_plan_root_4b_probe(None);
    assert_token_identity(&payload, AccountId(2), "4b 分支用例");
    assert!(
        !early_4b.load(Ordering::SeqCst),
        "失败后语义：4b 探针触发时刻 TaskDied 必已接收（early-4b=普通空等待提前放行=M11 形态红）"
    );
    assert!(
        taskdied_received.load(Ordering::SeqCst),
        "TaskDied 接收保持位（先于 4b 触发）"
    );
    assert!(
        fourb_after_failure.load(Ordering::SeqCst),
        "确定性分支证据：协调者实际经过失败后 4b 等待分支——other 保持至该分支之后才发送"
    );
    assert_guards_returned(&compute);
}

/// 双重 panic 防护回归（增量二十一；增量二十八适配新序 R1-c——账户门
/// 取代 ordinal 门消除进入序竞态）：root1（panic）自由、root2 **按账户门**
/// 闩（在飞未发送、先于 TaskDied 派发）→ 非阻塞循环拉取消费 TaskDied
/// （remaining 2→1>0 → 存字段返回，保存形态）→ 闭包内 panic（模拟外层
/// 既有失败）→ unwind 中批被 Drop：GateReleaser 先开账户门 → root2 迟到
/// Prepared 被收满丢弃 → task_died 命中 panicking 分支——不重抛（重抛=
/// abort 并吞原始失败），stderr 显式登记，原始 panic 原样传回。
#[test]
#[ignore = "口径=串行独立运行：同 a1_panic_form（全局注入交叉污染）；stderr 登记行由门禁 --nocapture 运行 grep 留证（q6-doublepanic.log）；node scripts/run-engine-lib-tests.mjs normal --ignored --bin <bin> plan_root_quota_wave_tests"]
fn a1_double_panic_guard_preserves_original_failure() {
    let mut session = two_account_session();
    let compute = session.compute.clone();
    super::plan_chain_candidates::set_root_panic_accounts(&[AccountId(1)]);
    // 账户门：root2（AccountId(2)）闩在观察点；root1 自由。
    let account_gate: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
    let taskdied_received = Arc::new(AtomicBool::new(false));
    {
        let gate = Arc::clone(&account_gate);
        super::plan_chain_candidates::set_plan_root_account_entry_probe(Some(Arc::new(
            move |account| {
                if account == AccountId(1) {
                    return;
                }
                let (open, signal) = &*gate;
                let mut open = open
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*open {
                    open = match signal.wait(open) {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                }
            },
        )));
        let received = Arc::clone(&taskdied_received);
        super::plan_chain_candidates::set_plan_root_taskdied_probe(Some(Arc::new(
            move || {
                received.store(true, Ordering::SeqCst);
            },
        )));
    }
    let original = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut batch = two_root_batch(&session);
        // unwind 安全：先于 batch Drop 析构开账户门（Drop 收满可终止）。
        struct AccountGateReleaser(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for AccountGateReleaser {
            fn drop(&mut self) {
                let (open, signal) = &*self.0;
                let mut open = open
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *open = true;
                signal.notify_all();
            }
        }
        let _account_releaser = AccountGateReleaser(Arc::clone(&account_gate));
        // 两 root 全派发（3 池无持槽）；root2 闩；root1 panic → 循环拉取至
        // TaskDied 消费（remaining 2→1>0 → 存字段）→ 空队列返回（保存形态）。
        wait_until("TaskDied 已被消费（保存形态成立）", || {
            match batch.prepare_ready_accounts(&mut session, false) {
                Ok(PrepareReady::Progress(false)) => {}
                _ => panic!("root2 闩住期间不应有交付或信号"),
            }
            taskdied_received.load(Ordering::SeqCst)
        });
        // 原始失败（task_died 已保存、root2 在飞被闩）→ unwind 中批 Drop →
        // 双重 panic 分支：stderr 登记、原始 panic 保留。
        panic!("a1 mid-unwind original failure");
    }))
    .expect_err("原始 panic 必须原样传回（不得被 abort/TaskDied 替换）");
    super::plan_chain_candidates::set_root_panic_accounts(&[]);
    super::plan_chain_candidates::set_plan_root_account_entry_probe(None);
    super::plan_chain_candidates::set_plan_root_taskdied_probe(None);
    let message = original
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| original.downcast_ref::<&str>().map(|text| (*text).to_string()));
    assert_eq!(
        message.as_deref(),
        Some("a1 mid-unwind original failure"),
        "双重 panic 分支必须保留原始失败身份"
    );
    assert_guards_returned(&compute);
}

/// p3 增强（增量二十重写）：①payload **对象身份**（RootPanicToken+Arc::ptr_eq，
/// clone/rebox 同内容不可通过——三审③）；②发布顺序探针——TaskDied 到达
/// 协调者时 bound 环境（含计时/扰动绑定）必已析构（两段式序：bound 环境
/// 析构 → finish 发布；探针在 bound 析构点触发，收到通知时必已置位）；
/// ③任务最终释放（guard 终态归还）。
#[test]
#[ignore = "口径=串行独立运行：同 a1_panic_form（全局注入交叉污染）；node scripts/run-engine-lib-tests.mjs normal --ignored --bin <bin> plan_root_quota_wave_tests"]
fn p3_binding_order_and_payload_identity() {
    let mut session = two_account_session();
    let compute = session.compute.clone();
    let mut batch = two_root_batch_single(&session);
    let bound_released = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&bound_released);
        super::plan_chain_candidates::set_root_bound_release_probe(Some(Arc::new(move || {
            flag.store(true, Ordering::SeqCst);
        })));
    }
    // 实际绑定释放直证（四审）：主线程安装可识别 phase timing 绑定
    //（test_collector，与生产捕获同构）→ 派发线程（本线程）捕获其克隆 →
    // 任务线程持有（thread-local+闭包链）→ TaskDied 发布与协调者接收之后，
    // 全部任务侧克隆实际 Drop ⇒ Arc 强计数回落到基线（RAII 释放的真实
    // 效果；executor 绑定同构，timing 绑定即「实际绑定」的直证对象）。
    let timing_binding = crate::verification_evidence::PhaseTimingBinding::test_collector();
    let binding_baseline = timing_binding.strong_count();
    let probe_binding = timing_binding.clone();
    // 接收点计数记录（六审口径）：TaskDied 接收探针在协调者线程收到当场
    // 读取强计数——该时刻发布线程 TLS 已卸下（发布点直证），计数=基线+结构
    // 固定的外层捕获副本（终态回落见下方断言；不宣称接收时全部克隆已 Drop）。
    let received_strong_count = Arc::new(std::sync::atomic::AtomicUsize::new(usize::MAX));
    {
        let record = Arc::clone(&received_strong_count);
        let count_probe = timing_binding.clone();
        super::plan_chain_candidates::set_plan_root_taskdied_probe(Some(Arc::new(
            move || {
                record.store(count_probe.strong_count(), Ordering::SeqCst);
            },
        )));
    }
    super::plan_chain_candidates::set_root_panic_accounts(&[AccountId(1)]);
    let payload = crate::verification_evidence::with_phase_timing_binding(timing_binding, || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let wait = deadline();
            loop {
                match batch.prepare_ready_accounts(&mut session, true) {
                    Ok(PrepareReady::Progress(_)) => {}
                    Ok(PrepareReady::PendingCapacity) => compute.wait_for_spawn_capacity(),
                    Err(fatal) => panic!("prepare 失败：{fatal}"),
                }
                if Instant::now() > wait {
                    panic!("p3 推进超时");
                }
            }
        }))
        .expect_err("panic 必须显式传播")
    });
    super::plan_chain_candidates::set_root_panic_accounts(&[]);
    super::plan_chain_candidates::set_plan_root_taskdied_probe(None);
    super::plan_chain_candidates::set_root_bound_release_probe(None);
    assert_token_identity(&payload, AccountId(1), "p3");
    assert!(
        bound_released.load(Ordering::SeqCst),
        "发布顺序（探针侧证据）：收到 TaskDied 时 bound 环境必已析构（探针先于 finish 发布）"
    );
    // 发布点直接核验（五审主断言）：finish 段 send(TaskDied) 调用现场，
    // 发布线程的相位计时绑定 TLS 必已卸下（实际绑定已释放——两段式序的
    // 直证，非计数推断；负向 M8 把 finish 移入绑定作用域内则此处红）。
    assert_eq!(
        super::plan_chain_candidates::plan_root_publish_tls_state(),
        Some(false),
        "发布点直证：send(TaskDied) 现场发布线程绑定 TLS 已卸下（实际绑定已释放）"
    );
    // 接收时刻数值断言（六审 Luna 断言、七轮归属修正）：接收瞬间强计数 =
    // 基线 + **测试侧 clone 数**（当前=2：测试持有的 probe_binding 与探针
    // 闭包捕获的 count_probe——协调者 TLS 持原引用、任务侧 timing 在发布
    // 前已随绑定作用域卸载；七轮 Astra 归属指正，非「任务外层捕获」）。
    // 发布时序主断言仍=发布点 TLS Some(false)；本数值断言检测测试侧克隆
    // 结构恒定，实现/测试任一侧变化将红提示口径更新。
    assert_ne!(
        received_strong_count.load(Ordering::SeqCst),
        usize::MAX,
        "TaskDied 接收探针必触发（接收点计数已记录）"
    );
    const TEST_SIDE_CLONES: usize = 2;
    assert_eq!(
        received_strong_count.load(Ordering::SeqCst),
        binding_baseline + TEST_SIDE_CLONES,
        "接收时刻数值断言：接收瞬间强计数=基线+测试侧 clone 数（probe_binding+count_probe）"
    );
    // 终态复核：强计数回落基线（测试侧两 clone 随作用域结束 Drop；任务侧
    // timing 已在发布前随绑定作用域卸载）。
    wait_until("phase timing 绑定强计数回落基线（终态实际释放）", || {
        probe_binding.strong_count() == binding_baseline
    });
    assert_eq!(
        probe_binding.strong_count(),
        binding_baseline,
        "终态复核：强计数回落基线（测试侧 clone 已随作用域结束 Drop）"
    );
    // 身份范围写明（五审）：同地址判等证明的是**内层 RootPanicToken 对象**
    // 的身份（Box 元素地址）；外层 payload Box 的重包不单独证明——语义身份
    // 即 token 本身（外层 Box 只是运输容器），如实登记。
    assert_guards_returned(&compute);
}

/// P1-①（增量二十重写）：TaskDied **已成功入队**（send 先于 guard Drop，
/// spawned 回落即已入队）后协调者被外层失败销毁——Drop 排空协议必须重抛。
/// 确定性握手：专用池 + 观察者闩钉住「派发已发生、Drop 未发生」窗口，
/// spawned 计数回落=入队完成（无采样竞态），drop 包裹 catch_unwind 断言
/// 显式退出原因（Err=原 payload 重抛；Ok=payload 静默丢失=缺陷红）。
/// 红证：mutation M1（Drop 排空协议整体移除）→ payload 丢失 → Ok → 红。
#[test]
#[ignore = "口径=串行独立运行：同 a1_panic_form；node scripts/run-engine-lib-tests.mjs normal --ignored --bin <bin> plan_root_quota_wave_tests"]
fn a1_queued_taskdied_survives_coordinator_drop() {
    let mut session = two_account_session();
    let compute = session.compute.clone();
    let (gate, entered) = install_root_entry_gate(&compute);
    let mut batch = two_root_batch_single(&session);
    let _releaser = GateReleaser(Arc::clone(&gate));
    super::plan_chain_candidates::set_root_panic_accounts(&[AccountId(1)]);
    // 派发确定性：一次非阻塞 prepare 即派发 panic root（闩在观察点）。
    match batch.prepare_ready_accounts(&mut session, false).unwrap() {
        PrepareReady::Progress(false) => {}
        _ => panic!("root 闩住期间不应有交付或信号"),
    }
    wait_until("panic root 进入 bound（被闩）", || entered.load(Ordering::SeqCst) == 1);
    let business_at_drop = session.business_state_hash().unwrap();
    // 迟到 TaskDied 窗口钉死（四审）：注入 Drop「已见 Empty」探针——探针在
    // Drop 首次观察到空队列的**同一线程时点**回调，回调内才放行 panic root：
    // 其 send 必然晚于 Empty 观察（旧实现「见 Empty 即返回」恰在此窗口丢
    // payload——负向 M7 红）。
    let empty_probe_fired = Arc::new(AtomicBool::new(false));
    {
        let fired = Arc::clone(&empty_probe_fired);
        let gate_for_probe = Arc::clone(&gate);
        super::plan_chain_candidates::set_plan_root_drop_empty_probe(Some(Arc::new(
            move || {
                fired.store(true, Ordering::SeqCst);
                release_roots(&gate_for_probe);
            },
        )));
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(batch)));
    super::plan_chain_candidates::set_plan_root_drop_empty_probe(None);
    assert!(
        empty_probe_fired.load(Ordering::SeqCst),
        "Drop 必须显式经过「队列已空」观察点（迟到窗口开启的协议锚点）"
    );
    super::plan_chain_candidates::set_root_panic_accounts(&[]);
    let payload = outcome.expect_err("已入队 TaskDied 必须在销毁点重抛（静默丢失=缺陷）");
    assert_token_identity(&payload, AccountId(1), "a1 queued 形态");
    assert_eq!(
        session.business_state_hash().unwrap(),
        business_at_drop,
        "销毁点传播不得改动权威状态"
    );
    assert_guards_returned(&compute);
}

/// P1-②（增量二十八适配新序 R1-c 消费优先）：三 root（panic=1、闩=2/3，
/// **按账户入口门**确定性编排——进入序无关）：root1 自由 panic → TaskDied；
/// 非阻塞拉取消费 TaskDied（remaining 3→2>0）→ **存 task_died 字段**返回
/// （新序下保存形态要求其他 root 先于 TaskDied 派发——三 root 起始全派发
/// 满足）；外层失败销毁协调者 → Drop 收满协议：收 TaskDied（payload 留存）
/// + root2/3 迟到 Prepared 丢弃（Empty 探针放行，send 晚于 Empty 观察）
/// → 收满重抛已保存 payload。M1 红：旧 Drop 忽略字段+一次性排空 → 队列
/// 空（TaskDied 已被消费）→ Ok → 红。
#[test]
#[ignore = "口径=串行独立运行：同 a1_panic_form；node scripts/run-engine-lib-tests.mjs normal --ignored --bin <bin> plan_root_quota_wave_tests"]
fn a1_saved_taskdied_with_late_result_survives_coordinator_drop() {
    let wide = crate::compute::ComputeContext::with_workers(3).unwrap();
    let (mut session, _) = execution_fixture_with_compute_and_insts(wide, 3);
    let compute = session.compute.clone();
    let market = session.build_market_view();
    let mut batch = super::plan_chain_candidates::PlanChainOperationBatch::accounts(
        vec![AccountId(1), AccountId(2), AccountId(3)],
        market,
        Default::default(),
        Default::default(),
        crate::calendar::CivilInstant::new(
            crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
            0,
        )
        .unwrap(),
        Default::default(),
    );
    super::plan_chain_candidates::clear_plan_root_published();
    super::plan_chain_candidates::set_root_panic_accounts(&[AccountId(1)]);
    // 账户入口门：账户 2/3 闩（观察点等待），账户 1（panic）自由。
    let account_gate: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
    let taskdied_received = Arc::new(AtomicBool::new(false));
    {
        let gate = Arc::clone(&account_gate);
        super::plan_chain_candidates::set_plan_root_account_entry_probe(Some(Arc::new(
            move |account| {
                if account == AccountId(1) {
                    return;
                }
                let (open, signal) = &*gate;
                let mut open = open
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*open {
                    open = match signal.wait(open) {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                }
            },
        )));
        let received = Arc::clone(&taskdied_received);
        super::plan_chain_candidates::set_plan_root_taskdied_probe(Some(Arc::new(
            move || {
                received.store(true, Ordering::SeqCst);
            },
        )));
    }
    // unwind 安全 guard：任何失败路径先开账户门（防 Drop 收满死等被闩 root）。
    struct AccountGateReleaser(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for AccountGateReleaser {
        fn drop(&mut self) {
            let (open, signal) = &*self.0;
            let mut open = open
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *open = true;
            signal.notify_all();
        }
    }
    let _account_releaser = AccountGateReleaser(Arc::clone(&account_gate));
    // ① 非阻塞循环拉取：三 root 全派发（3 容量）；root1 panic → TaskDied；
    //   循环轮询至消费发生（remaining 3→2>0）→ 存字段 → 空队列 → None
    //   返回（保存形态；循环容忍 TaskDied 尚未入队的首几轮）。
    wait_until("TaskDied 已被消费（保存形态成立）", || {
        match batch.prepare_ready_accounts(&mut session, false) {
            Ok(PrepareReady::Progress(false)) => {}
            _ => panic!("root2/3 闩住期间不应有交付或信号"),
        }
        taskdied_received.load(Ordering::SeqCst)
    });
    let business_at_saved = session.business_state_hash().unwrap();
    let plans_at_saved = serde_json::to_value(&session.state.plans).unwrap();
    // ② Drop 迟到窗口钉死：Empty 探针回调放行账户门 → root2/3 的 send 晚于
    //   Empty 观察；Drop 收满（TaskDied payload + 两迟到 Prepared 丢弃）→
    //   重抛已保存 payload。
    let empty_probe_fired = Arc::new(AtomicBool::new(false));
    {
        let fired = Arc::clone(&empty_probe_fired);
        let gate_for_probe = Arc::clone(&account_gate);
        super::plan_chain_candidates::set_plan_root_drop_empty_probe(Some(Arc::new(
            move || {
                fired.store(true, Ordering::SeqCst);
                let (open, signal) = &*gate_for_probe;
                let mut open = open
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *open = true;
                signal.notify_all();
            },
        )));
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(batch)));
    super::plan_chain_candidates::set_plan_root_drop_empty_probe(None);
    super::plan_chain_candidates::set_plan_root_account_entry_probe(None);
    super::plan_chain_candidates::set_plan_root_taskdied_probe(None);
    super::plan_chain_candidates::set_root_panic_accounts(&[]);
    super::plan_chain_candidates::clear_plan_root_published();
    assert!(
        empty_probe_fired.load(Ordering::SeqCst),
        "Drop 必须显式经过「队列已空」观察点（迟到窗口开启的协议锚点）"
    );
    let payload = outcome.expect_err(
        "已保存 task_died 必须在销毁点重抛；迟到结果必须被收满计数（见 Empty 即返回=竞态）",
    );
    assert_token_identity(&payload, AccountId(1), "a1 saved 形态");
    assert_eq!(
        session.business_state_hash().unwrap(),
        business_at_saved,
        "迟到的 root2/3 结果不得 install（私有候选随批丢弃）"
    );
    assert_eq!(
        serde_json::to_value(&session.state.plans).unwrap(),
        plans_at_saved,
        "迟到结果不得复活 personal state"
    );
    assert_guards_returned(&compute);
}


/// e2（死区形态·root 在飞）：F 侧候选全被 AccountValidation 拒绝的完整 tick
/// 链路属集成层（`pipeline/plan_quota_dead_zone_tests.rs` 增量二十已补）；
/// 本单元承载死区核心断言——候选产出为空且 roots 在飞时，blocking prepare
/// 不返回 Complete 也不挂起：4b 等在飞推进直至交付。
#[test]
fn e2_inflight_roots_dead_zone_keeps_progressing_to_delivery() {
    let mut session = two_account_session();
    let compute = session.compute.clone();
    let mut batch = two_root_batch(&session);
    // 首轮派发两 root 在飞（空账）。随后持续 blocking 拉取：中途不产生
    // Complete（roots 未排空）也不挂起；两个 root 都交付后才排空。
    let mut deliveries = 0usize;
    let wait = deadline();
    loop {
        match batch.prepare_ready_accounts(&mut session, true).unwrap() {
            PrepareReady::Progress(true) => deliveries += 1,
            // roots 排空稳态=死区闭合的终点（候选层拒绝不在此层）。
            PrepareReady::Progress(false) => break,
            PrepareReady::PendingCapacity => compute.wait_for_spawn_capacity(),
        }
        assert!(
            Instant::now() < wait,
            "整体推进超时（死区挂死即缺陷）"
        );
    }
    assert_eq!(deliveries, 2, "两 root 恰各交付一次");
}

/// a1（StepFatal 形态·真实事务中途，增量二十重写）：`inject_post_shadow_failure`
/// 在 CommitTick 准备点触发（市场事务已完整运行——F 已在 shadow 内执行并
/// install，区别于增量十九 `inject_step_failure` 的 pre-mutation 位置：事务
/// 根本未运行）。失败后权威零部分提交：业务哈希/事件与 tick 前完全一致；
/// session 哈希漂移仅 poison 单字段（临时摘除 poison 复算判等后恢复——
/// ADR-0017 口径如实断言）；F 已执行以观察者 PlanRoot 记录 ≥1 证明；guard
/// 终态归还。
/// 红证（mutation M4）：注入点退回 check_step_start（增量十九形态）→
/// F-proof 断言红。
#[test]
fn a1_stepfatal_form_atomicity_via_real_failing_tick() {
    // D5 pin+登记（旧管线专有机制）：staged 注入事务原子性=旧管线
    // staged handover 桥（runtime 首步消费面 take_staged_intents
    // 为构造期一次性——注入中途失败重放面不适用）。等价锚：组端
    // Abort/undo 可逆日志（翻转批 batch4 civil 回滚全量对账）承载
    // 原子性语义。
    if crate::runtime::step_runtime_enabled() {
        return;
    }
    let (mut session, _) = execution_fixture_with_compute(dedicated_compute());
    let compute = session.compute.clone();
    let plan_root_entries = Arc::new(AtomicUsize::new(0));
    {
        let entries = Arc::clone(&plan_root_entries);
        compute.set_observer(Arc::new(move |observation| {
            if observation.work == crate::compute::ComputeWork::PlanRoot {
                entries.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }
    let business_before = session.business_state_hash().unwrap();
    let session_before = session.session_state_hash().unwrap();
    let plans_before = serde_json::to_value(&session.state.plans).unwrap();
    let fatal = crate::session::StepFatal::InvariantViolation {
        description: "quota a1 post-shadow injection".to_owned(),
        location: "plan_root_quota_wave_tests".to_owned(),
    };
    session.inject_post_shadow_failure(fatal.clone());
    let step_result = session.step();
    assert_eq!(
        step_result.unwrap_err(),
        fatal,
        "注入失败必须使 step 显式 Err 且为原 fatal"
    );
    assert_eq!(
        session.business_state_hash().unwrap(),
        business_before,
        "真实事务失败后权威业务/事件状态零漂移（F 的 install 全在 shadow，随失败丢弃）"
    );
    assert_eq!(
        serde_json::to_value(&session.state.plans).unwrap(),
        plans_before,
        "失败 tick 的 personal/计划状态不复活"
    );
    assert!(
        plan_root_entries.load(Ordering::SeqCst) >= 1,
        "真实事务期间计划根已进入 bound（F 已执行的存在性证明；pre-mutation 注入形态下为 0）"
    );
    // session 哈希漂移上界=仅 poison 单字段：临时摘除 poison 复算判等后恢复
    //（失败元数据不得丢失；hash.rs 口径：session hash=business+poison+诊断缓存）。
    let poison = session.poison.take();
    assert_eq!(
        session.session_state_hash().unwrap(),
        session_before,
        "session 哈希漂移仅来自 poison 元数据（ADR-0017 允许口径的显式上界）"
    );
    session.poison = poison;
    assert_guards_returned(&compute);
}

/// e1（推进前提形态·operations 层死区，coordinator 级）：持满全部 spawn 槽
/// → 两 root pending-only+零在飞——blocking prepare 不得 Ready（无交付）、
/// 不得 Complete（roots 未排空）、不得挂起：池外上提 PendingCapacity（专用
/// 池确定性断言信号必然发生）；容量恢复后补派发推进至全部交付。
/// ready_stock_stream 级 AccountValidation 全拒集成（e1 完整形态）见
/// `pipeline/plan_quota_dead_zone_tests.rs`。
#[test]
fn e1_pending_only_dead_zone_closes_after_capacity_recovery() {
    let mut session = two_account_session();
    let compute = session.compute.clone();
    let mut held = Vec::new();
    while let Some(guard) = compute.reserve_spawn_slot() {
        held.push(guard);
    }
    let mut batch = two_root_batch(&session);
    let wait = deadline();
    let mut delivered = 0usize;
    let mut signals = 0usize;
    loop {
        match batch.prepare_ready_accounts(&mut session, true).unwrap() {
            PrepareReady::Progress(true) => delivered += 1,
            PrepareReady::Progress(false) => break,
            PrepareReady::PendingCapacity => {
                signals += 1;
                assert!(Instant::now() < wait, "死区不得挂起：pending-only 必须上提信号");
                if !held.is_empty() {
                    held.pop();
                }
                compute.wait_for_spawn_capacity();
            }
        }
        assert!(
            Instant::now() < wait,
            "死区不得挂起：pending-only 形态必须推进至交付或排空"
        );
    }
    assert_eq!(delivered, 2, "容量恢复后 pending 全部补派发交付");
    assert!(
        signals >= 1,
        "pending-only+零在飞必须产生 PendingCapacity（不 Ready、不 Complete、不挂起）"
    );
    drop(held);
    assert_guards_returned(&compute);
}
