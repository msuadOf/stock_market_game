//! 计算后端抽象（ADR-0008）：批 7 起 rayon 全面退役——本文件是**串行垫片**
//! （公共 API 保持，池/派发语义以串行立即执行实现；并行由 `runtime/` 组线程/
//! 交易部所有权模型接管，GameSession 接线前的过渡形态）。
//!
//! 设计原则：
//! - 只抽象「并行可独立」的工作（NPC decide）。
//! - 顺序依赖部分（撮合、结算、路由）留在 session.rs，不经 ComputeBackend。
//! - CPU 实现 = rayon 并行（零行为变化）。
//! - GPU 实现 = wgpu compute shader（整数定点，跨厂商确定性）。
//! - 动态切换：SessionSetup.compute 控制。
//!
//! 共同 V（隐藏公允价）已删除：后端只接收**纯批量输入**（同一份公共市场
//! 视图 + 每人自身视图 + 每人种子），按输入顺序返回稳定顺序输出，不按
//! 账户身份偷授予任何信息；个体信念/计划的会话侧编排不经本接缝。

use crate::compute::parallel::*;
use crate::config::GameConfig;
use crate::session::SplitMix64;
use crate::strategy::{Intent, MarketView, SelfView, StrategyData};

/// 会话计算资源的唯一执行上下文；宿主可共享同一个 Arc，shadow 不另建池。
/// 不承载交易权威状态、Tokio I/O 或旧状态析构。
pub struct ComputeContext {
    /// 配置 worker 数（串行垫片：仅作宽度记账与部署合同读数，不建线程）。
    workers: usize,
    /// 池准入台账：占池协调者数（drives）与已派发在飞预留数（spawned）共用
    /// 一把锁，使联合不变式「drives + spawned ≤ workers」在**入口登记与
    /// 派发预留两点**都原子成立、持续不破（round3 反例 b：旧实现先读计数再
    /// spawn，窗口内他人占槽；round4 ①-必修a：旧入口无条件增 drives，他人
    /// 预留后、spawn 入队前的窗口内进入者登记可致 drives+spawned>workers）。
    /// 该不变式只对**多 worker 池（workers > 1）**声明（round5-③ 限定）：
    /// 单 worker 池入口恒准入且 spawned 不记账，drives 可在协作让位下的
    /// 嵌套入场中超过 workers——单 worker 池的活性不靠台账，由
    /// `stock_stream` 单 worker 分支的 `try_recv + yield_work` 协作循环
    /// 自足保证（论证见 `.tmp/spine/round5-fix/liveness-contract.md` 第三节）。
    /// 入口被拒的协调者不进入池内执行路径（不登记、不派发预留，只在自己
    /// 线程内联推进自有工作）；派发预留被拒的任务留在 pending 由协调者内联
    /// 或池外等待容量。只做准入登记与容量唤醒，不含任何调度行为。
    /// WASM 单线程恒为空转。
    admission: std::sync::Mutex<PoolAdmission>,
    /// 容量变化广播：占槽退出/派发预留释放时唤醒等待容量的池外协调者。
    capacity_changed: std::sync::Condvar,
    /// 方向B·宽度封顶（宽度税治理）：实际参与宽度——池宽（`workers()`）
    /// 是部署合同上限（读法 A：池=上限、活跃任务=取用），本值是粒度成形
    /// （parallel_grain/scatter-gather 派发）所对齐的**实际**参与宽度，
    /// 恒在 [1, workers] 内。tick 入口按本 tick 真实并行需求收缩：无可见
    /// 工作的线程走 rayon 空闲协议快速入睡，消灭宽池空扫描与 join 等待
    /// 自旋（128w 实测税底=醒线程扫描驻留，不随任务数缩减——见
    /// .tmp/width-diagnosis/ 与 .tmp/width-fix-A/ 归因）。多会话共享同一
    /// 上下文时后写者生效（与准入台账同域，登记为已知形状）。
    active_width: std::sync::atomic::AtomicUsize,
    #[cfg(any(test, feature = "verification-harness"))]
    observer: std::sync::RwLock<Option<std::sync::Arc<dyn Fn(ComputeObservation) + Send + Sync>>>,
    /// 派发门（测试/验证专用）：派发点成功预留之后、spawn 入队之前调用，
    /// 用于把「预留返回与 spawn 之间」的计数窗口钉成确定交错（round4
    /// ①-必修a 的红测试）。仅本上下文显式设置时生效，互不干扰。
    #[cfg(any(test, feature = "verification-harness"))]
    dispatch_gate: std::sync::RwLock<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// 容量等待探针（测试/验证专用）：`wait_for_spawn_capacity` 在台账锁内
    /// 确认容量满（`drives + spawned ≥ workers`）之后、`wait_while` 原子
    /// 进入等待之前调用（round5-② 锁内化：探针持锁发出，测试方收到信号
    /// 即可断定等待者已持锁确认满账且即将原子入等——取代 round4 ② 的
    /// 取锁前发信号）。回调在持台账锁状态下执行，不得再取台账锁
    /// （reserve/release/enter/admission_snapshot 等都会取锁，须避免）。
    #[cfg(any(test, feature = "verification-harness"))]
    capacity_wait_probe: std::sync::RwLock<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
    /// T1 验收探针（ADR-0042 §T1；cfg(test)，**按上下文隔离**避免并行测试
    /// 互相污染）：install 往返窗口计数、账户任务流工作量计数（spawned/
    /// payloads/inline）、queue 链阶段计数（attention 判定/个人观察/个人
    /// 分析/decide/quote+plan 投影/共享捕获次数）。见 T1Probes 与
    /// npc_decisions/npc_tick_preparation 的递增点。
    #[cfg(test)]
    pub(crate) t1_probes: T1Probes,
}

/// T1 验收探针组（cfg(test)）：全部按 ComputeContext 隔离。
#[cfg(test)]
#[derive(Default)]
pub(crate) struct T1Probes {
    pub(crate) install_roundtrip_probe_active: std::sync::atomic::AtomicBool,
    pub(crate) install_roundtrip_count: std::sync::atomic::AtomicU64,
    pub(crate) install_roundtrip_last: std::sync::atomic::AtomicU64,
    pub(crate) stream_spawned: std::sync::atomic::AtomicU64,
    pub(crate) stream_payloads: std::sync::atomic::AtomicU64,
    pub(crate) stream_inline: std::sync::atomic::AtomicU64,
    /// 方向B·scatter-gather 派发的 chunk 任务计数（cfg(test)，按上下文隔离）。
    pub(crate) parallel_gather_chunks: std::sync::atomic::AtomicU64,
    /// 方向B·scatter-gather 协调线程内联 chunk 计数（cfg(test)）。
    pub(crate) parallel_gather_inline: std::sync::atomic::AtomicU64,
    pub(crate) queue_stage_attention: std::sync::atomic::AtomicU64,
    pub(crate) queue_stage_observation: std::sync::atomic::AtomicU64,
    pub(crate) queue_stage_analysis: std::sync::atomic::AtomicU64,
    pub(crate) queue_stage_decide: std::sync::atomic::AtomicU64,
    pub(crate) queue_stage_projection: std::sync::atomic::AtomicU64,
    pub(crate) queue_stage_captures: std::sync::atomic::AtomicU64,
    /// 归并到达序记录（cfg(test)）：账户任务结果到达消费方（归并回调）
    /// 的真实顺序——ADR-0032 的 receipt 事实基准，供顺序语义断言对照。
    pub(crate) queue_arrival_order: std::sync::Mutex<Vec<crate::AccountId>>,
    /// T3·epoch 对齐调用见证（cfg(test)，Astra 四发现裁定·发现 1）：逐
    /// attempt（queue 生产调用，序号由 `queue_attempt_seq` 分配）/epoch
    /// （tick）/account 记录 attention evaluate 的**实际调用**与 evaluate
    /// 边界前后的 rng_state。恰一次评估以本记录为证据；next_candidate 终值
    /// 与「RNG 确有推进」仅辅助。
    pub(crate) queue_attention_trace: std::sync::Mutex<Vec<AttentionCallRecord>>,
    /// queue 生产调用序号（cfg(test)）：`queue_npc_on` 入口 fetch_add 分配，
    /// 单调递增；跨测试上下文隔离（同一 ComputeContext 内全局唯一）。
    pub(crate) queue_attempt_seq: std::sync::atomic::AtomicU64,
    /// T3 收口（Astra 三轮·组 1）root 生产交接见证（cfg(test)）：真实 root
    /// 完成时点（capture_roots 返回后）与首批交付时点（first_ready_batch
    /// 进入时）共用同一全局单调计数——「首批交付等待 root 完毕」的顺序证据。
    pub(crate) root_handover_seq: std::sync::atomic::AtomicU64,
    /// 交接记录：真实 root 完成时点（tick/attempt/roots/序号/捕获序数）。
    pub(crate) root_handover_trace: std::sync::Mutex<Vec<RootHandoverRecord>>,
    /// 首批交付记录（tick/序号/attempt）——与完成记录同计数域，逐 tick
    /// 配对证据（六轮 Astra·组 1）。
    pub(crate) first_delivery_trace: std::sync::Mutex<Vec<FirstDeliveryRecord>>,
    /// T5（Sol 终裁·完成边界见证）：root personal patch 真实 install 计数
    ///（prepare 接收点内联 install 时递增）——capture 返回当场可断言。
    pub(crate) root_personal_installs: std::sync::atomic::AtomicU64,
    /// T5（同上）：capture 期排空的容量受阻分支进入计数。
    pub(crate) root_drain_capacity_waits: std::sync::atomic::AtomicU64,
}

/// T3 收口·首批交付单条记录（cfg(test)）：一次真实交付时点证据。
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct FirstDeliveryRecord {
    /// 交付 tick。
    pub(crate) tick: u64,
    /// 交付序号（与 handover_seq 同计数域：完成=奇数位、交付=紧邻偶数位）。
    pub(crate) delivery_seq: u64,
    /// 交付时的生产 attempt 快照（同源标识——与完成记录的 attempt 对应）。
    pub(crate) attempt: u64,
}

/// T3 收口·root 交接单条记录（cfg(test)）：一次真实 root 完成时点证据。
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct RootHandoverRecord {
    /// 交接发生的消费 tick（post-expiry 执行相位）。
    pub(crate) tick: u64,
    /// 本次交接对应的生产 attempt（queue_attempt_seq 快照——完成→交付按
    /// session/attempt/tick 三键配对的边界证据；五轮 Astra·组 2）。
    pub(crate) attempt: u64,
    /// 本次交接的 root 批非空标记（operations/routes/root 源任一非空）。
    pub(crate) roots: usize,
    /// 全局单调交接序号（与首批交付序号同一计数域比较）。
    pub(crate) handover_seq: u64,
    /// 同 tick 内的捕获序数（1=首次；≥2=重捕获——与「同一生产结果」判定
    /// 联用：同 tick 同 attempt 的第二次捕获=重捕获证据）。
    pub(crate) capture_ordinal: u32,
    /// 重捕获现场（同 tick 第二次捕获时抓一次调用栈——六轮 Astra·组 1
    /// 的实际调用路径证据；仅首例抓取避免膨胀）。
    pub(crate) backtrace: Option<String>,
}

/// T3·调用见证单条记录（cfg(test)）：一次 attention evaluate 的实际调用证据。
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct AttentionCallRecord {
    /// queue 生产调用序号（同一 tick attempt 内的调用分组键）。
    pub(crate) attempt: u64,
    /// 评估 epoch（=该 attempt 的 pop 视界 tick）。
    pub(crate) tick: u64,
    pub(crate) account: crate::AccountId,
    /// evaluate 的受理判定结果（生产自身的 accept 决策）。
    pub(crate) accepted: bool,
    /// evaluate 前 rng_state（该账户个人 RNG 流位置）。
    pub(crate) rng_before: u64,
    /// evaluate 后 rng_state（判定抽样 1–2 次后的流位置；个人分析续抽在其后）。
    pub(crate) rng_after: u64,
}

/// 派发准入台账（`drives + spawned ≤ workers` 的两个分量）。
#[derive(Default)]
struct PoolAdmission {
    drives: usize,
    spawned: usize,
}

/// 协调者占用本池 worker 槽期间的登记守卫：Drop 时递减计数并广播容量变化。
/// 不调度、不持有任务；仅让派发准入能看到“本池还有几个槽真正可用”。
pub(crate) struct PoolDriveSlot {
    context: std::sync::Arc<ComputeContext>,
}

impl Drop for PoolDriveSlot {
    fn drop(&mut self) {
        {
            let mut admission = self
                .context
                .admission
                .lock()
                .expect("池准入台账锁被 poison");
            admission.drives = admission.drives.saturating_sub(1);
        }
        self.context.capacity_changed.notify_all();
    }
}

/// 派发预留守卫：Drop 归还一个派发槽并广播容量变化。
/// 单 worker 池不记账（惰性守卫，`context` 为 None）。
pub(crate) struct SpawnSlotGuard {
    context: Option<std::sync::Arc<ComputeContext>>,
}

impl Drop for SpawnSlotGuard {
    fn drop(&mut self) {
        if let Some(context) = self.context.take() {
            context.release_spawn_slot();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, thiserror::Error)]
pub enum ComputeInitError {
    #[error("计算 worker 数必须大于零")]
    InvalidWorkers,
    #[error("无法读取 OS 可用计算资源，OS code={os_code:?}")]
    AvailableResources { os_code: Option<i32> },
    #[error("无法创建计算执行上下文，workers={workers}，OS code={os_code:?}")]
    PoolBuild {
        workers: usize,
        os_code: Option<i32>,
    },
    #[error("计算资源配置超出执行库表示范围，requested={requested}，maximum={maximum}")]
    UnsupportedWorkerCount { requested: usize, maximum: usize },
    #[error("此 WASM 宿主没有线程能力，requested={workers}")]
    ThreadsUnavailable { workers: usize },
}

/// 验证观察点记录真实执行库状态；不参与交易顺序或存档。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum ComputeWork {
    Observe,
    DayEndDeliver,
    LedgerRebase,
    Analyse,
    Decide,
    Project,
    StockPrepare,
    StockMatch,
    StockFinish,
    SettlementAccount,
    ReceiptProjectionAccount,
    ReadOnlyPrepare,
    CandidatePrepare,
    PlanRoot,
    /// ReceiptAggregation 大账本完整校验（≥512 行的并行 row 审计）。
    ReceiptAggregationAudit,
}

#[cfg(any(test, feature = "verification-harness"))]
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct ComputeObservation {
    pub work: ComputeWork,
    pub configured_workers: usize,
    pub executing_workers: usize,
    /// ThreadPool::current_thread_index 检查实际 registry，不是自行分配的 tag。
    pub owner_worker: Option<usize>,
}



#[cfg(test)]
mod context_tests {
    use super::*;

    #[test]
    fn compute_context_rejects_zero_without_initializing_a_pool() {
        assert!(matches!(
            ComputeContext::with_workers(0),
            Err(ComputeInitError::InvalidWorkers)
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn compute_context_default_uses_available_os_resources() {
        assert_eq!(
            ComputeContext::shared_default().unwrap().workers(),
            std::thread::available_parallelism().unwrap().get()
        );
    }

    /// 方向B·宽度封顶（scatter-gather 原语）：按 active_width 打包的阻塞
    /// 归并并行 map——结果按索引序、首错按索引序（不按完成序）；chunk 任务
    /// 数 = min(len, active_width)；active_width 可低于池宽（128=上限语义，
    /// 实际参与宽度按需收缩，空闲线程无可见工作即入睡，消灭 join 等待自旋
    /// 与全池空扫描）。
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn parallel_map_chunks_gathers_in_order_with_active_width_cap() {
        let compute = ComputeContext::with_workers(4).unwrap();
        compute.set_active_width(2);
        let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = std::sync::Arc::clone(&collected);
        let out: Vec<u64> = compute
            .parallel_map_chunks(7, move |i| {
                seen.lock().unwrap().push(i);
                Ok::<u64, ()>(i as u64 + 100)
            })
            .expect("全 Ok 输入不得出错");
        assert_eq!(out, vec![100, 101, 102, 103, 104, 105, 106], "结果按索引序");
        let mut ran = collected.lock().unwrap().clone();
        ran.sort_unstable();
        assert_eq!(ran, (0..7).collect::<Vec<_>>(), "每索引恰一次");
        assert_eq!(
            compute.t1_probes.parallel_gather_chunks.swap(0, std::sync::atomic::Ordering::SeqCst),
            2,
            "chunk 任务数必须=min(len,active_width)=2"
        );
        let err: Result<Vec<u64>, usize> =
            compute.parallel_map_chunks(7, |i| if i == 3 || i == 5 { Err(i) } else { Ok(i as u64) });
        assert_eq!(err, Err(3), "首错必须按索引序而非完成序");
    }

    /// 方向B·宽度封顶：active_width 钳制在 [1, 池宽]；超界设置取边界。
    #[test]
    fn active_width_is_clamped_to_pool_bounds() {
        let compute = ComputeContext::with_workers(4).unwrap();
        compute.set_active_width(0);
        assert_eq!(compute.active_width(), 1, "下界=1");
        compute.set_active_width(999);
        assert_eq!(compute.active_width(), 4, "上界=池宽（128=上限语义）");
        compute.set_active_width(3);
        assert_eq!(compute.active_width(), 3);
    }

    /// 方向A·任务粗化：chunk 粒度公式——目标 chunk 数=min(len,workers)，
    /// chunk_size=ceil(len/chunks)；len≤workers 时退化为 1（与旧逐项派发
    /// 同形，宽池/due 小时不改变行为）；无经验魔数。
    #[test]
    fn parallel_grain_targets_worker_aligned_chunk_count() {
        use super::parallel_grain;
        assert_eq!(parallel_grain(7, 2), 4, "ceil(7/min(7,2))=4 ⇒ 2 个 chunk");
        assert_eq!(parallel_grain(4, 128), 1, "due≤workers ⇒ 单账户 chunk（旧行为）");
        assert_eq!(parallel_grain(20000, 128), 157, "ceil(20000/128)=157 ⇒ 128 个 chunk");
        assert_eq!(parallel_grain(627, 16), 40, "ceil(627/16)=40 ⇒ 16 个 chunk");
        assert_eq!(parallel_grain(0, 8), 1, "空集粒度无意义但保持正数");
        assert_eq!(parallel_grain(9, 1), 9, "单 worker ⇒ 单 chunk 全串行");
        // Luna 快检（方向A）补证：目标数是近似+上界，非恰等——9/4 目标 4，
        // 实际 chunk_size=3 ⇒ 3 个 chunk（ceil(9/3)=3 ≤ 4）。
        assert_eq!(parallel_grain(9, 4), 3);
        assert!(9usize.div_ceil(parallel_grain(9, 4)) <= 9.min(4));
    }

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn compute_context_wasm_is_explicitly_sequential() {
        assert!(matches!(
            ComputeContext::with_workers(2),
            Err(ComputeInitError::ThreadsUnavailable { workers: 2 })
        ));
        assert_eq!(ComputeContext::with_workers(1).unwrap().install(|| 7), 7);
    }
}

impl ComputeContext {
    /// T1 验收探针（cfg(test)）：`install` 往返计数——窗口由被测协调链
    /// 开启/关闭（queue 链账户任务流的消费窗口），窗口内每次本上下文的
    /// `install`（含 `parallel` 别名）调用计 1。目标形态下协调线程纯归并、
    /// 账户任务纯计算，消费窗口内计数恒 0；旧形态（逐结果 install 投影）
    /// 计数 >0 可红。按上下文隔离，避免并行测试互相污染。
    #[cfg(test)]
    pub(crate) fn begin_install_roundtrip_probe(&self) {
        self.t1_probes.install_roundtrip_count.store(0, std::sync::atomic::Ordering::SeqCst);
        self.t1_probes
            .install_roundtrip_probe_active
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// 关闭探针窗口并把窗口计数存档（测试用）。
    #[cfg(test)]
    pub(crate) fn end_install_roundtrip_probe(&self) {
        self.t1_probes
            .install_roundtrip_probe_active
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.t1_probes.install_roundtrip_last.store(
            self.t1_probes
                .install_roundtrip_count
                .load(std::sync::atomic::Ordering::SeqCst),
            std::sync::atomic::Ordering::SeqCst,
        );
    }

    /// 读取并清零最近一次窗口的 install 往返计数（测试用）。
    #[cfg(test)]
    pub(crate) fn take_last_install_roundtrip_probe_count(&self) -> u64 {
        self.t1_probes
            .install_roundtrip_last
            .swap(0, std::sync::atomic::Ordering::SeqCst)
    }

    /// 读取并清零账户任务流工作量计数（spawned, payloads, inline）（测试用）。
    #[cfg(test)]
    pub(crate) fn take_stream_workload_counts(&self) -> (u64, u64, u64) {
        (
            self.t1_probes.stream_spawned.swap(0, std::sync::atomic::Ordering::SeqCst),
            self.t1_probes.stream_payloads.swap(0, std::sync::atomic::Ordering::SeqCst),
            self.t1_probes.stream_inline.swap(0, std::sync::atomic::Ordering::SeqCst),
        )
    }

    /// 读取并清零 queue 链阶段计数（attention, observation, analysis,
    /// decide, projection, captures）（测试用）。
    #[cfg(test)]
    pub(crate) fn take_queue_stage_counts(&self) -> (u64, u64, u64, u64, u64, u64) {
        (
            self.t1_probes.queue_stage_attention.swap(0, std::sync::atomic::Ordering::SeqCst),
            self.t1_probes.queue_stage_observation.swap(0, std::sync::atomic::Ordering::SeqCst),
            self.t1_probes.queue_stage_analysis.swap(0, std::sync::atomic::Ordering::SeqCst),
            self.t1_probes.queue_stage_decide.swap(0, std::sync::atomic::Ordering::SeqCst),
            self.t1_probes.queue_stage_projection.swap(0, std::sync::atomic::Ordering::SeqCst),
            self.t1_probes.queue_stage_captures.swap(0, std::sync::atomic::Ordering::SeqCst),
        )
    }

    /// 读取并清空 attention 调用见证记录（T3·Astra 四发现裁定·发现 1，测试用）。
    /// 恰一次评估的**实际调用证据**从这里取证。
    #[cfg(test)]
    pub(crate) fn take_queue_attention_trace(
        &self,
    ) -> Vec<crate::compute::AttentionCallRecord> {
        std::mem::take(
            &mut *self
                .t1_probes
                .queue_attention_trace
                .lock()
                .expect("attention 调用见证锁被 poison"),
        )
    }

    pub fn with_workers(workers: usize) -> Result<std::sync::Arc<Self>, ComputeInitError> {
        if workers == 0 {
            return Err(ComputeInitError::InvalidWorkers);
        }
        Ok(std::sync::Arc::new(Self {
            workers,
            active_width: std::sync::atomic::AtomicUsize::new(workers),
            admission: std::sync::Mutex::new(PoolAdmission::default()),
            capacity_changed: std::sync::Condvar::new(),
            #[cfg(any(test, feature = "verification-harness"))]
            observer: std::sync::RwLock::new(None),
            #[cfg(any(test, feature = "verification-harness"))]
            dispatch_gate: std::sync::RwLock::new(None),
            #[cfg(any(test, feature = "verification-harness"))]
            capacity_wait_probe: std::sync::RwLock::new(None),
            #[cfg(test)]
            t1_probes: T1Probes::default(),
        }))
    }

    /// 池等价构造（串行垫片）：按 worker 数直接构造（原 `from_pool` 的
    /// 测试迁移面——rayon 池已退役）。
    pub fn from_workers(workers: usize) -> std::sync::Arc<Self> {
        Self::with_workers(workers).expect("from_workers：workers 必须为正")
    }

    /// 默认资源来自 OS 及进程亲和限制，不读取 caller 的 Rayon registry。
    pub fn available() -> Result<std::sync::Arc<Self>, ComputeInitError> {
        #[cfg(not(target_arch = "wasm32"))]
        let workers = std::thread::available_parallelism()
            .map_err(|error| ComputeInitError::AvailableResources {
                os_code: error.raw_os_error(),
            })?
            .get();
        #[cfg(target_arch = "wasm32")]
        let workers = 1;
        Self::with_workers(workers)
    }

    /// direct-engine 默认入口显式持有并共享此上下文，不创建 Rayon global pool。
    pub fn shared_default() -> Result<std::sync::Arc<Self>, ComputeInitError> {
        static DEFAULT: std::sync::OnceLock<
            Result<std::sync::Arc<ComputeContext>, ComputeInitError>,
        > = std::sync::OnceLock::new();
        DEFAULT.get_or_init(Self::available).clone()
    }



    /// 尝试登记一个正在占用本池 worker 槽的协调循环；必须由已在本池内的
    /// 线程调用（`is_worker()` 为真），守卫存活期间协调者始终在该槽上。
    /// 入口即条件准入（round4 ①-必修a）：台账锁内联合校验
    /// `drives + spawned < workers`，成功才 `drives += 1` 并返回守卫——
    /// 联合不变式因此在**入口与派发两点**都持续成立（workers>1 的池，
    /// round5-③ 限定），不再只在派发预留点
    /// 成立（旧实现无条件增 drives：他人预留后、spawn 入队前的窗口内，
    /// 新进入者登记可使 drives+spawned 超过 workers，不变式被打破）。
    /// 准入被拒返回 None：install 已把闭包放到 worker 上、等待即自锁，
    /// 该协调者不得进入池内执行路径——不登记、不做派发预留（未登记实体
    /// 若再预留会重新打破不变式），只在自己线程内联推进自有待派发工作
    /// （物理上仍是池内 worker，资源域不变），其内联完成或退出即释放
    /// worker，超订只延迟不锁死。单 worker 池恒准入（workers≤1 分支）：
    /// 台账不变式只对 workers>1 声明——单 worker 池 spawned 不记账
    /// （`reserve_spawn_slot` 惰性守卫），drives 可在协作让位下的嵌套入场
    /// 超过 workers，活性由 `stock_stream` 单 worker 分支的
    /// `try_recv + yield_work` 协作循环自足保证（契约文档第三节单 worker
    /// 特例，round5-③ 表述限定）。
    pub(crate) fn try_enter_pool_drive_slot(self: &std::sync::Arc<Self>) -> Option<PoolDriveSlot> {
        let mut admission = self.admission.lock().expect("池准入台账锁被 poison");
        let admitted =
            self.workers() <= 1 || admission.drives + admission.spawned < self.workers();
        if !admitted {
            return None;
        }
        admission.drives += 1;
        drop(admission);
        Some(PoolDriveSlot {
            context: self.clone(),
        })
    }

    /// 派发点原子预留：多 worker 池在锁内联合校验 `drives + spawned < workers`，
    /// 成功则 `spawned += 1` 并返回守卫（Drop 归还并广播容量变化）；
    /// 失败返回 None——任务必须留在 pending，由池内协调者内联或池外协调者
    /// 等待容量推进。单 worker 池直接放行（树 A 协作 yield 语义）。
    pub(crate) fn reserve_spawn_slot(self: &std::sync::Arc<Self>) -> Option<SpawnSlotGuard> {
        if self.workers() <= 1 {
            return Some(SpawnSlotGuard { context: None });
        }
        let workers = self.workers();
        let mut admission = self.admission.lock().expect("池准入台账锁被 poison");
        (admission.drives + admission.spawned < workers).then(|| {
            admission.spawned += 1;
            SpawnSlotGuard {
                context: Some(self.clone()),
            }
        })
    }

    /// 归还一个派发预留并广播容量变化（仅由 `SpawnSlotGuard` 调用）。
    fn release_spawn_slot(&self) {
        {
            let mut admission = self.admission.lock().expect("池准入台账锁被 poison");
            debug_assert!(
                admission.spawned > 0,
                "派发预留释放次数多于预留次数（台账失衡）"
            );
            admission.spawned = admission.spawned.saturating_sub(1);
        }
        self.capacity_changed.notify_all();
    }

    /// 等待派发容量恢复：真实同步原语（Condvar），不轮询、不自旋。
    /// 池外协调者在准入为 0 且仍有未派发工作时调用；占槽退出与派发预留
    /// 释放都会唤醒。单 worker 池立即返回（树 A 协作语义覆盖）。
    /// 容量等待探针（round5-② 锁内化）：在台账锁内确认
    /// `drives + spawned ≥ workers` 之后、`wait_while` 原子释放锁进入等待
    /// 之前发出。测试方看到探针时，本线程已持锁确认满账，而任何容量释放
    /// （`release_spawn_slot` / `PoolDriveSlot::drop`）都必须先取同一把锁：
    /// 释放要么在探针读取满账之前已完成（不可能——那时读到的就不是满账），
    /// 要么必然排在本线程进入等待之后，唤醒不会丢失——「信号已发、等待
    /// 未入」的窗口不存在（旧实现探针在取锁前发信号，测试方无法区分
    /// 等待者是已入等还是尚未取锁，Astra round5 阻断②）。探针回调在持
    /// 台账锁状态下执行，不得再取台账锁。
    pub(crate) fn wait_for_spawn_capacity(&self) {
        if self.workers() <= 1 {
            return;
        }
        let workers = self.workers();
        let admission = self.admission.lock().expect("池准入台账锁被 poison");
        #[cfg(any(test, feature = "verification-harness"))]
        if admission.drives + admission.spawned >= workers {
            self.run_capacity_wait_probe();
        }
        let admission = self
            .capacity_changed
            .wait_while(admission, |state| state.drives + state.spawned >= workers)
            .expect("池准入台账锁被 poison");
        drop(admission);
    }

    /// 当前是否仍有派发容量（唤醒后复查，避免虚假唤醒后空转返回）。
    pub(crate) fn has_spawn_capacity(&self) -> bool {
        if self.workers() <= 1 {
            return true;
        }
        let workers = self.workers();
        let admission = self.admission.lock().expect("池准入台账锁被 poison");
        admission.drives + admission.spawned < workers
    }

    /// fire-and-forget 任务进入本上下文池；WASM 顺序立即执行。
    /// 两段式任务：`bound` 在继承的计时/扰动绑定内执行并返回载荷；
    /// `finish`（发送等收尾动作）在绑定释放之后执行——协调者可见的完成信号
    /// 因此严格晚于最后一个绑定克隆的析构，收尾 try_unwrap 无需等待即独占。
    pub(crate) fn spawn_two_stage<R: Send + 'static>(
        &self,
        bound: impl FnOnce() -> R + Send + 'static,
        finish: impl FnOnce(R) + Send + 'static,
    ) {
        // 串行垫片：任务立即执行（绑定包装保持——计时/扰动继承语义不变）。
        let timing = crate::verification_evidence::current_phase_timing_binding();
        #[cfg(any(test, feature = "verification-harness"))]
        let executor = crate::session::pipeline::current_executor_binding();
        let payload = crate::verification_evidence::with_phase_timing_binding(timing, move || {
            #[cfg(any(test, feature = "verification-harness"))]
            {
                crate::session::pipeline::with_executor_binding(executor, bound)
            }
            #[cfg(not(any(test, feature = "verification-harness")))]
            {
                bound()
            }
        });
        finish(payload);
    }

    pub(crate) fn spawn(&self, work: impl FnOnce() + Send + 'static) {
        // 串行垫片：立即执行（绑定继承语义保持）。
        let timing = crate::verification_evidence::current_phase_timing_binding();
        #[cfg(any(test, feature = "verification-harness"))]
        let executor = crate::session::pipeline::current_executor_binding();
        crate::verification_evidence::with_phase_timing_binding(timing, move || {
            #[cfg(any(test, feature = "verification-harness"))]
            {
                crate::session::pipeline::with_executor_binding(executor, work)
            }
            #[cfg(not(any(test, feature = "verification-harness")))]
            {
                work()
            }
        });
    }

    /// 并行分支 burst：两个分支在本上下文池执行，协调逻辑留在调用线程。
    pub(crate) fn join<A: Send, B: Send>(
        &self,
        left: impl FnOnce() -> A + Send,
        right: impl FnOnce() -> B + Send,
    ) -> (A, B) {
        (left(), right())
    }

    /// scope burst：scope 体在本上下文池的一个 worker 上执行（含其 spawn）。
    pub(crate) fn scope<'scope, R: Send>(
        &self,
        body: impl FnOnce(&crate::compute::Scope<'scope>) -> R + Send,
    ) -> R {
        crate::compute::scope(body)
    }

    /// scope 体保留在协调调用线程；spawn 的借用任务只进入此池。
    pub(crate) fn in_place_scope<'scope, R>(
        &self,
        body: impl FnOnce(&crate::compute::Scope<'scope>) -> R,
    ) -> R {
        crate::compute::scope(body)
    }

    pub(crate) fn is_worker(&self) -> bool {
        install_depth() > 0
    }

    /// 并行迭代 burst：整段 par 链在本上下文池执行后回到调用线程。
    pub(crate) fn parallel<R: Send>(&self, burst: impl FnOnce() -> R + Send) -> R {
        self.install(burst)
    }

    /// 方向B·宽度封顶：读取实际参与宽度（恒在 [1, workers()]）。
    pub fn active_width(&self) -> usize {
        let width = self
            .active_width
            .load(std::sync::atomic::Ordering::Relaxed);
        width.clamp(1, self.workers())
    }

    /// 方向B·宽度封顶：设置实际参与宽度（钳制到 [1, workers()]；池宽=上限
    /// 语义不变）。tick 入口按本 tick 并行需求调用；不影响准入不变式
    /// （drives+spawned ≤ workers 仍以全池为界——派发任务仍运行在全池上，
    /// 本值只收缩粒度成形的目标宽度）。
    pub fn set_active_width(&self, width: usize) {
        let clamped = width.clamp(1, self.workers());
        self.active_width
            .store(clamped, std::sync::atomic::Ordering::Relaxed);
    }

    /// 方向B·宽度封顶·scatter-gather 原语：按 active_width 打包的并行 map。
    ///
    /// 与 `compute.parallel(par_iter)` 的关键差异（宽度税归因结论）：
    /// rayon join/par_iter 的驱动线程在等待分裂任务完成时以 SpinLatch
    /// 自旋并做 O(池宽) 全池偷取扫描（128w 实测 ~77 万次/ tick 扫描的
    /// 主力来源）；本原语派发 **min(len, active_width) 个粗 chunk 任务**
    ///（经准入预留），协调线程**阻塞/协作等待**归并（零自旋），空闲
    /// worker 无可见工作即走 rayon 空闲协议入睡。
    ///
    /// 语义合同：每索引恰一次；结果与**首错都按索引序**（比 par_iter 的
    /// try_for_each 完成序更确定——错误实例选择收窄为确定性，登记于
    /// 方向B 台账）；chunk 任务 panic 以原 payload 重抛（与 par_iter 的
    /// panic 传播合同一致）；`len <= active_width()` 时串行（逐项并行只
    /// 付调度税不增并行度）；WASM 串行。
    pub(crate) fn parallel_map_chunks<R, E>(
        self: &std::sync::Arc<Self>,
        len: usize,
        item: impl Fn(usize) -> Result<R, E> + Sync,
    ) -> Result<Vec<R>, E>
    where
        R: Send,
        E: Send,
    {
        let active = self.active_width();
        let chunk_size = parallel_grain(len, active);
        let chunk_count = len.div_ceil(chunk_size);
        // 串行快速路径：空集 / 单 chunk（含 active=1 与 WASM）/ 需求不超过
        // 参与宽度（逐项并行无收益只付调度税）。
        if chunk_count <= 1 || len <= active.max(1) {
            return (0..len).map(&item).collect();
        }
        debug_assert!(chunk_count > 1 && chunk_count <= active.max(1));

        /// chunk 任务消息：按 chunk 序的结果段，或 unwind 的原 panic payload。
        enum ChunkMessage<R, E> {
            Done(usize, Vec<Result<R, E>>),
            Died(Box<dyn std::any::Any + Send>),
        }

        let ranges: std::collections::VecDeque<std::ops::Range<usize>> =
            (0..chunk_count).map(|c| (c * chunk_size)..((c + 1) * chunk_size).min(len)).collect();
        self.in_place_scope(|scope| {
            let (sender, receiver) = std::sync::mpsc::channel::<ChunkMessage<R, E>>();
            let mut origin_sender = Some(sender);
            let mut pending = ranges;
            let mut slots: Vec<Option<Vec<Result<R, E>>>> = (0..chunk_count).map(|_| None).collect();
            let mut in_flight = 0usize;
            let mut died: Option<Box<dyn std::any::Any + Send>> = None;
            loop {
                // (d0) 非阻塞收尽现成结果。
                while in_flight > 0 {
                    let message = match receiver.try_recv() {
                        Ok(message) => message,
                        Err(std::sync::mpsc::TryRecvError::Empty)
                        | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    };
                    in_flight -= 1;
                    match message {
                        ChunkMessage::Done(index, segment) => slots[index] = Some(segment),
                        ChunkMessage::Died(payload) => {
                            if died.is_none() {
                                died = Some(payload);
                            }
                        }
                    }
                }
                if died.is_some() {
                    // 排空（与 NPC 流同纪律）：先 Drop 原始 sender，收至
                    // Disconnected 即全部任务终结；随后重抛原 payload。
                    drop(origin_sender.take().expect("排空只发生一次"));
                    while let Ok(message) = receiver.recv() {
                        match message {
                            ChunkMessage::Done(index, segment) => slots[index] = Some(segment),
                            ChunkMessage::Died(_) => {}
                        }
                    }
                    break;
                }
                // (a) 尽力派发：预留成功才 spawn，失败留 pending。
                while let Some(range) = pending.front().cloned() {
                    let Some(guard) = self.reserve_spawn_slot() else {
                        break;
                    };
                    pending.pop_front();
                    let sender = origin_sender
                        .as_ref()
                        .expect("派发阶段原始 sender 必须仍在")
                        .clone();
                    let item = &item;
                    let timing = crate::verification_evidence::current_phase_timing_binding();
                    #[cfg(any(test, feature = "verification-harness"))]
                    let executor = crate::session::pipeline::current_executor_binding();
                    let index = range.start / chunk_size;
                    scope.spawn(move |_| {
                        // 工作量探针：spawned chunk 任务计 1（cfg(test)）。
                        #[cfg(test)]
                        self.t1_probes.parallel_gather_chunks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let segment = std::panic::catch_unwind(
                            std::panic::AssertUnwindSafe(move || {
                                crate::verification_evidence::with_phase_timing_binding(
                                    timing,
                                    move || {
                                        #[cfg(any(test, feature = "verification-harness"))]
                                        {
                                            crate::session::pipeline::with_executor_binding(
                                                executor,
                                                move || {
                                                    range.clone()
                                                        .map(|i| item(i))
                                                        .collect::<Vec<_>>()
                                                },
                                            )
                                        }
                                        #[cfg(not(any(test, feature = "verification-harness")))]
                                        {
                                            range.map(|i| item(i)).collect::<Vec<_>>()
                                        }
                                    },
                                )
                            }),
                        );
                        match segment {
                            Ok(segment) => {
                                let _ = sender.send(ChunkMessage::Done(index, segment));
                            }
                            Err(payload) => {
                                let _ = sender.send(ChunkMessage::Died(payload));
                            }
                        }
                        drop(guard);
                    });
                    in_flight += 1;
                }
                // (b) 终止。
                if in_flight == 0 && pending.is_empty() {
                    break;
                }
                if in_flight == 0 {
                    // (c) R1-a 零在飞推进：协调线程内联一个 chunk（同步完成）。
                    let range = pending.pop_front().expect("R1-a 内联项来自非空 pending");
                    #[cfg(test)]
                    self.t1_probes.parallel_gather_inline.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let index = range.start / chunk_size;
                    slots[index] = Some(range.map(&item).collect());
                    continue;
                }
                // (d) 等待一个结果：池内协作让位（try_recv+yield_work），
                // 池外真实阻塞——两者都不自旋扫描。
                let message = if self.is_worker() {
                    loop {
                        match receiver.try_recv() {
                            Ok(message) => break message,
                            Err(std::sync::mpsc::TryRecvError::Empty) => yield_work(),
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                panic!("gather chunk 任务未交回结果")
                            }
                        }
                    }
                } else {
                    receiver.recv().expect("gather chunk 任务必须交回结果")
                };
                in_flight -= 1;
                match message {
                    ChunkMessage::Done(index, segment) => slots[index] = Some(segment),
                    ChunkMessage::Died(payload) => {
                        if died.is_none() {
                            died = Some(payload);
                        }
                    }
                }
            }
            if let Some(payload) = died {
                std::panic::resume_unwind(payload);
            }
            // 归并：按 chunk 序拼接，首错按索引序。
            let mut out = Vec::with_capacity(len);
            for (index, segment) in slots.into_iter().enumerate() {
                let segment = segment.unwrap_or_else(|| {
                    panic!("gather chunk {index} 缺少结果（流协议破坏）")
                });
                for result in segment {
                    out.push(result?);
                }
            }
            Ok(out)
        })
    }

    pub fn workers(&self) -> usize {
        self.workers.max(1)
    }

    /// 把整段计算工作放进本上下文的池执行；已在池内则原地进行。
    /// 池内嵌套并行任务与协调等待（阻塞/协作）都留在同一资源域。
    pub fn install<R: Send>(&self, work: impl FnOnce() -> R + Send) -> R {
        // T1 验收探针：窗口内 install 往返计数（cfg(test)，按本上下文隔离）。
        #[cfg(test)]
        {
            if self
                .t1_probes
                .install_roundtrip_probe_active
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                self.t1_probes
                    .install_roundtrip_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        // 串行垫片：直接执行（保留绑定包装；install 深度标记 is_worker）。
        let timing = crate::verification_evidence::current_phase_timing_binding();
        #[cfg(any(test, feature = "verification-harness"))]
        let executor = crate::session::pipeline::current_executor_binding();
        let _guard = InstallDepth::enter();
        crate::verification_evidence::with_phase_timing_binding(timing, move || {
            #[cfg(any(test, feature = "verification-harness"))]
            {
                crate::session::pipeline::with_executor_binding(executor, work)
            }
            #[cfg(not(any(test, feature = "verification-harness")))]
            {
                work()
            }
        })
    }

    #[cfg(any(test, feature = "verification-harness"))]
    pub fn set_observer(&self, observe: std::sync::Arc<dyn Fn(ComputeObservation) + Send + Sync>) {
        *self.observer.write().expect("计算 observer 锁被 poison") = Some(observe);
    }

    /// 台账快照（测试/验证专用）：(占池协调者数 drives, 在飞派发预留数 spawned)。
    /// 供确定性测试断言联合不变式 `drives + spawned ≤ workers` 持续成立
    /// （round4 ①-必修a；该不变式只对 workers>1 的池声明，round5-③）。
    #[cfg(any(test, feature = "verification-harness"))]
    pub(crate) fn admission_snapshot(&self) -> (usize, usize) {
        let admission = self.admission.lock().expect("池准入台账锁被 poison");
        (admission.drives, admission.spawned)
    }

    /// 安装派发门（见字段注释；仅测试/验证构建）。
    #[cfg(any(test, feature = "verification-harness"))]
    pub fn set_dispatch_gate(&self, gate: std::sync::Arc<dyn Fn() + Send + Sync>) {
        *self.dispatch_gate.write().expect("派发门锁被 poison") = Some(gate);
    }

    /// 运行派发门（若已安装）：在派发点成功预留之后、spawn 入队之前调用。
    #[cfg(any(test, feature = "verification-harness"))]
    pub(crate) fn run_dispatch_gate(&self) {
        if let Some(gate) = self
            .dispatch_gate
            .read()
            .expect("派发门锁被 poison")
            .as_ref()
        {
            gate();
        }
    }

    /// 安装容量等待探针（见字段注释；仅测试/验证构建）。
    #[cfg(any(test, feature = "verification-harness"))]
    pub fn set_capacity_wait_probe(&self, probe: std::sync::Arc<dyn Fn() + Send + Sync>) {
        *self.capacity_wait_probe.write().expect("容量探针锁被 poison") = Some(probe);
    }

    /// 运行容量等待探针（若已安装）：`wait_for_spawn_capacity` 在台账锁内、
    /// 确认容量满之后、原子入等之前调用（见字段注释与该函数的锁纪律说明）。
    #[cfg(any(test, feature = "verification-harness"))]
    pub(crate) fn run_capacity_wait_probe(&self) {
        if let Some(probe) = self
            .capacity_wait_probe
            .read()
            .expect("容量探针锁被 poison")
            .as_ref()
        {
            probe();
        }
    }

    #[inline]
    pub(crate) fn record(&self, work: ComputeWork) {
        #[cfg(any(test, feature = "verification-harness"))]
        if let Some(observe) = self
            .observer
            .read()
            .expect("计算 observer 锁被 poison")
            .as_ref()
        {
            let (executing_workers, owner_worker) = (self.workers(), None);
            observe(ComputeObservation {
                work,
                configured_workers: self.workers(),
                executing_workers,
                owner_worker,
            });
        }
        let _ = work;
    }
}

/// 串行 join：先左后右（垫片语义；结果序确定）。
pub(crate) fn join<A: Send, B: Send>(
    left: impl FnOnce() -> A + Send,
    right: impl FnOnce() -> B + Send,
) -> (A, B) {
    (left(), right())
}

/// 串行 Scope：spawn 的借用任务立即执行，不建立队列或调度器。
pub(crate) struct Scope<'scope>(std::marker::PhantomData<&'scope ()>);

impl<'scope> Scope<'scope> {
    pub fn spawn<F>(&self, work: F)
    where
        F: FnOnce(&Scope<'scope>) + Send + 'scope,
    {
        work(self);
    }
}

pub(crate) fn scope<'scope, R>(work: impl FnOnce(&Scope<'scope>) -> R) -> R {
    work(&Scope(std::marker::PhantomData))
}

pub(crate) use scope as in_place_scope;

thread_local! {
    /// install 深度（串行垫片的 is_worker 判据）。
    static INSTALL_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn install_depth() -> u32 {
    INSTALL_DEPTH.with(|d| d.get())
}

/// install 深度守卫。
struct InstallDepth;

impl InstallDepth {
    fn enter() -> Self {
        INSTALL_DEPTH.with(|d| d.set(d.get() + 1));
        InstallDepth
    }
}

impl Drop for InstallDepth {
    fn drop(&mut self) {
        INSTALL_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// 方向A·任务粗化（宽度税治理，诊断见 .tmp/width-diagnosis/）：并行工作按
/// worker 数对齐打包——返回 chunk 内最少元素数。目标 chunk 数 ≈
/// `min(len, workers)`，**精确上界为 ≤**：chunk_size = ceil(len /
/// min(len, workers)) 再向上取整回除可使实际 chunk 数少于目标（例：
/// len=9, workers=4 ⇒ chunk_size=3 ⇒ 3 个 chunk < 4）——Luna 快检（方向A）
/// 收窄口径：本公式是近似目标+确定上界，不是恰等式。
/// 细任务（单账户/单订单/单页）在宽池上触发唤醒风暴与全池空扫描自旋
/// （128w 实测 CPU/tick 放大 23.7x、steal 空扫百万级/ tick），按本粒度
/// 打包后任务数不随池宽放大，粒度随负载自适应（无经验魔数）。
/// `len == 0` 时返回 1（空集合自然不产生任务，粒度无意义但保持正数）。
pub fn parallel_grain(len: usize, workers: usize) -> usize {
    // len==0 时 chunks 钉 1（空集合自然零任务，除零不可能）。
    let chunks = len.min(workers.max(1)).max(1);
    len.div_ceil(chunks).max(1)
}

pub(crate) fn yield_work() {
    std::thread::yield_now();
}

/// 串行 parallel 垫片（全平台；原 wasm 兼容层提升为唯一实现）：par_iter 等
/// 解析为顺序迭代——消费面（pipeline 散点）零改动。
pub(crate) mod parallel {
    /// WASM 不调用 Rayon 初始化或 spawn；只替换独立遍历的执行方式。
    pub trait IntoParallelIterator: IntoIterator + Sized {
        fn into_par_iter(self) -> Self::IntoIter {
            self.into_iter()
        }
    }
    impl<T: IntoIterator> IntoParallelIterator for T {}
    pub trait ParallelRefIterator<'a>: 'a
    where
        &'a Self: IntoIterator,
    {
        fn par_iter(&'a self) -> <&'a Self as IntoIterator>::IntoIter {
            self.into_iter()
        }
    }
    impl<'a, T: ?Sized + 'a> ParallelRefIterator<'a> for T where &'a T: IntoIterator {}
    pub trait ParallelRefMutIterator<'a>: 'a
    where
        &'a mut Self: IntoIterator,
    {
        fn par_iter_mut(&'a mut self) -> <&'a mut Self as IntoIterator>::IntoIter {
            self.into_iter()
        }
    }
    impl<'a, T: ?Sized + 'a> ParallelRefMutIterator<'a> for T where &'a mut T: IntoIterator {}
    pub trait SequentialGrain: Iterator + Sized {
        fn with_min_len(self, _: usize) -> Self {
            self
        }
        /// 顺序实现复用单个 scratch；rayon 按 split 重建 init，两者结果一致（init 纯构造）。
        fn map_init<T, R>(
            self,
            mut init: impl FnMut() -> T,
            mut map: impl FnMut(&mut T, Self::Item) -> R,
        ) -> std::iter::Map<Self, impl FnMut(Self::Item) -> R> {
            let mut scratch = init();
            self.map(move |item| map(&mut scratch, item))
        }
    }
    impl<T: Iterator> SequentialGrain for T {}
}

/// 计算后端 trait：抽象 CPU/GPU 并行计算。
///
/// 实现者负责：
/// - `decide_all`：并行跑所有 NPC 的 decide，返回每人 Intent 列表（按输入顺序）。
///
/// 调用方（session.rs step）负责顺序部分（路由 Intent、撮合、结算）。
pub trait ComputeBackend: Send + Sync {
    /// 并行 NPC decide。
    ///
    /// - `strategies`: 每NPC的策略数据（数据化后，可序列化→GPU buffer）。
    /// - `market`: 每个决策者看到的同一份公共市场视图（无隐藏信息）。
    /// - `selves`: 每NPC的自身视图。
    /// - `seeds`: 每NPC的确定性 RNG 种子（seed ^ tick ^ npc_id）。
    /// - `config`: 本局权威费用配置，供买入数量估算与账户受理共用。
    ///
    /// 返回 Vec<Vec<Intent>>，索引与 strategies 对齐。
    fn decide_all(
        &self,
        strategies: &[StrategyData],
        market: &MarketView,
        selves: &[SelfView],
        seeds: &[u64],
        config: &GameConfig,
    ) -> Result<Vec<Vec<Intent>>, ComputeError>;

    /// 后端名称（"cpu" / "gpu"），用于日志/调试。
    fn name(&self) -> &'static str;
}

/// CPU 后端：rayon 多核并行。
pub struct CpuBackend;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ComputeError {
    #[error("invalid compute input: {0}")]
    InvalidInput(String),
    #[error("requested compute backend is unavailable: {0}")]
    BackendUnavailable(&'static str),
}

impl ComputeBackend for CpuBackend {
    fn decide_all(
        &self,
        strategies: &[StrategyData],
        market: &MarketView,
        selves: &[SelfView],
        seeds: &[u64],
        config: &GameConfig,
    ) -> Result<Vec<Vec<Intent>>, ComputeError> {
        if strategies.len() != selves.len() || strategies.len() != seeds.len() {
            return Err(ComputeError::InvalidInput(format!(
                "decide_all requires equal lengths, got strategies={}, selves={}, seeds={}",
                strategies.len(),
                selves.len(),
                seeds.len()
            )));
        }
        Ok(strategies
            .par_iter()
            .enumerate()
            .map(|(i, s)| {
                let sv = &selves[i];
                let mut rng = SplitMix64::new(seeds[i]);
                crate::strategy::decide_data(s, market, sv, &mut rng, config)
            })
            .collect())
    }

    fn name(&self) -> &'static str {
        "cpu"
    }
}

/// 计算模式（配置项）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Default)]
pub enum ComputeMode {
    /// CPU rayon 并行（默认）。
    #[default]
    Cpu,
    /// GPU wgpu compute shader（需要 engine-gpu crate + SharedArrayBuffer）。
    Gpu,
    /// 自动：有 GPU 且负载大时用 GPU，否则 CPU。
    Auto,
}

/// 创建指定模式的计算后端。
/// CPU 模式直接返回 CpuBackend。
/// GPU 模式返回 GpuBackend（需要 engine-gpu feature + wgpu 初始化）。
pub fn create_backend(mode: &ComputeMode) -> Result<Box<dyn ComputeBackend>, ComputeError> {
    match mode {
        ComputeMode::Cpu | ComputeMode::Auto => Ok(Box::new(CpuBackend)),
        ComputeMode::Gpu => Err(ComputeError::BackendUnavailable(
            "the authoritative session currently supports CPU only; use engine-gpu explicitly after parity validation",
        )),
    }
}
