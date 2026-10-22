//! 多组名册偏移红证（P0 预存缺陷：组端 root 读上下文账户簿漏加
//! `roster_offset`）。
//!
//! 背景：组线程持有**组内局部坐标** `local`（0..per_group），而
//! `RootProjection.account_ids` / `.chain_params` 是**全局名册序**
//! （drive.rs 由全局 `ids` 映射构造）。`roster_offset` 字段不变量
//! （group.rs:791）要求：组内 local 访问全局投影须加偏移；单组
//! `roster_offset==0` 恒正确 ⇒ 缺陷被掩盖。
//!
//! 触发面：`World::collect_open_derivations()` 向**每一组**广播
//! `EchoOpenDerivation`，组内遍历**全部** belief 位直调
//! `run_chain_decision`（`write_back=false` 纯观察读面）。该路径不经
//! `next_due` 采样（诊断已证伪的"触发时点随机"猜测），因此**每拍必触发**
//! ——红证稳定可复现，不靠运气。
//!
//! 失效形态分两类，本文件两者都钉：
//! - 显式：`account ID is absent`（roots.rs:276——账户簿键与解析出的
//!   全局 AccountId 不符）；
//! - 静默：组 g≥1 取到**组 0** 的链参数/现金/持仓（数值错、不 panic
//!   ——比显式 panic 更隐蔽）。故除"不 panic"外另加**跨组相异断言**。

use super::bridge_tests::setup_fixture;
use super::drive::RuntimeDriver;
use crate::GameSession;

/// 双组世界（会话 + 驱动器）：机构位被均分到两组（组 1 的 `roster_offset
/// > 0`——缺陷的必要条件）。沿用 `d3a2c_group_open_derivation_matches_
/// session_observe` 的会话 fixture（完整 legal 个人态：attention/belief/
/// information 六件齐全——observe 链需跑完才显红，不从零造 fixture）。
fn grouped_world(n_groups: usize, inst_count: u32) -> (GameSession, RuntimeDriver) {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = inst_count;
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 17).expect("会话构造");
    let mut driver =
        RuntimeDriver::from_session_grouped(&mut session, n_groups).expect("多组驱动器构造");
    driver.adopt_session_seq(&session);
    (session, driver)
}

/// 推进一拍（会话锁步：步进 + 行情/K 安装——与 d3a2c 红证同式）。
fn advance_one_tick(session: &mut GameSession, driver: &mut RuntimeDriver) {
    driver
        .step_frame_equivalent(session)
        .expect("步进");
    driver.install_market_views(session).expect("行情安装");
    driver.install_candles(session).expect("K 安装");
}

/// 红证 1（显式 panic）：双组下 `collect_open_derivations` 不得失败。
///
/// 修复前：组 1（`roster_offset = per_group > 0`）经 `build_root_read_
/// context` 用裸 local 取 `account_ids`，把**组 0 的账户 id** 装进
/// `AccountBook`；`run_chain_decision`（正确解析 `roster_offset+local`
/// 的全局 id）拿该 id 去 roots.rs:276 查簿 ⇒ 缺键 panic。
/// 修复后：各组键=本组名册 id，收集成功。
#[test]
fn multi_group_open_derivation_does_not_panic_on_tail_group() {
    let (mut session, mut driver) = grouped_world(2, 4);
    advance_one_tick(&mut session, &mut driver);

    let collected = driver
        .collect_open_derivations()
        .expect("双组开仓派生收集成功（尾组账户簿键=本组名册 id）");
    assert_eq!(collected.len(), 2, "两组各一条上行");
    // 尾组必须有派生记录——空记录会让本红证退化成"没跑到"的假绿。
    let (tail_group, .., tail_records) = collected
        .iter()
        .max_by_key(|(group, ..)| *group)
        .expect("存在尾组");
    assert!(
        !tail_records.is_empty(),
        "尾组 {tail_group} 须有派生记录（否则本红证未触达缺陷面）"
    );
}

/// 红证 2（静默类）：组 1 的派生账户**不得**与组 0 重复。
///
/// 只验"不 panic"会漏掉 2798/3323 这类静默取组 0 数据的缺陷——它们不
/// panic，只是把错误的链参数/现金/持仓喂进决策链。故显式断言跨组相异。
#[test]
fn multi_group_account_ids_do_not_collide_across_groups() {
    let (mut session, mut driver) = grouped_world(2, 4);
    advance_one_tick(&mut session, &mut driver);

    let collected = driver
        .collect_open_derivations()
        .expect("双组开仓派生收集成功");
    let accounts_by_group: Vec<Vec<crate::AccountId>> = collected
        .iter()
        .map(|(.., records)| records.iter().map(|record| record.account).collect())
        .collect();
    assert_eq!(accounts_by_group.len(), 2, "两组");
    assert!(
        !accounts_by_group[0].is_empty() && !accounts_by_group[1].is_empty(),
        "两组都有机构位（均分保证——空组使断言失去判别力）"
    );
    let overlap: Vec<_> = accounts_by_group[1]
        .iter()
        .filter(|id| accounts_by_group[0].contains(id))
        .collect();
    assert!(
        overlap.is_empty(),
        "组 1 派生账户须为本组名册 id，不得与组 0 重复（漏 roster_offset \
         会把组 0 的 id 装进组 1 的账户簿）：重叠={overlap:?}"
    );
}

/// 红证 3（规模面）：2/3/4 组均可跑完（缺陷对任意 n_groups ≥ 2 成立
/// ——诊断已证伪 padding 猜测：groups=2 整除、groups=4 无 padding 均
/// panic）。仅确认不 panic 且各组出数，不作性能断言。
#[test]
fn multi_group_open_derivation_scales_over_group_counts() {
    for n_groups in [2usize, 3, 4] {
        let (mut session, mut driver) = grouped_world(n_groups, 6);
        advance_one_tick(&mut session, &mut driver);
        let collected = driver
            .collect_open_derivations()
            .unwrap_or_else(|error| panic!("{n_groups} 组开仓派生收集失败：{error:?}"));
        assert_eq!(collected.len(), n_groups, "{n_groups} 组各一条上行");
        for (group, .., records) in &collected {
            assert!(
                !records.is_empty(),
                "{n_groups} 组下组 {group} 须有派生记录（空记录=未触达）"
            );
        }
    }
}

/// 多拍探针：多组下连续步进（含成交 → `apply_fill_plan_progress` 的
/// 1565 路径）。1565 漏偏移是**静默**的（尾组命中组 0 账户 ⇒ 无票则
/// `return Ok(())`，计划推进整段跳过），不 panic；本探针确认多拍不失败
/// 且派生面持续出数（尾组不因静默跳过而退化为空）。
#[test]
fn multi_group_stays_consistent_over_several_ticks() {
    let (mut session, mut driver) = grouped_world(2, 6);
    for tick in 1..=6 {
        advance_one_tick(&mut session, &mut driver);
        let collected = driver
            .collect_open_derivations()
            .unwrap_or_else(|error| panic!("第 {tick} 拍双组派生收集失败：{error:?}"));
        assert_eq!(collected.len(), 2, "第 {tick} 拍两组各一条上行");
        for (group, .., records) in &collected {
            assert!(
                !records.is_empty(),
                "第 {tick} 拍组 {group} 须有派生记录（空=尾组静默跳过）"
            );
        }
    }
}

/// 组数参数化下推进 `ticks` 拍，取**全量计划指纹**（生产读面
/// `collect_plans`——组线程簿现值，非 Echo 门控通道）。
///
/// 指纹含账户/股票/方向/目标量/已成交量/状态——即 `open_position_action`
/// 的全部输出字段（消费链参数、权益、持仓）。
fn plan_fingerprints(n_groups: usize, ticks: usize) -> Vec<String> {
    let (mut session, mut driver) = grouped_world(n_groups, 4);
    for _ in 0..ticks {
        advance_one_tick(&mut session, &mut driver);
    }
    let mut out: Vec<String> = driver
        .collect_plans()
        .unwrap_or_else(|error| panic!("{n_groups} 组计划收集失败：{error}"))
        .iter()
        .flat_map(|(_group, book)| {
            book.plan_ids()
                .filter_map(|pid| book.plan(pid).ok())
                .map(|plan| {
                    format!(
                        "{:?} {:?} {:?} {:?} filled={} {:?}",
                        plan.account(),
                        plan.code(),
                        plan.direction(),
                        plan.target(),
                        plan.filled_qty(),
                        plan.status()
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    // 组序/簿序不参与语义（组切分只改归属，不改计划内容）⇒ 排序消除。
    out.sort();
    out
}

/// 红证 5（**调用层**·生产入口级）：组切分不得改变任何账户的开仓计划。
///
/// 与红证 1~4 的区别：本红证**不直接调** `roster_slice` /
/// `roster_chain_params`，而是走生产入口——`step_frame_equivalent`
/// → `GroupIn::Start` → `run_chain_decision(write_back=true)` →
/// `collect_plans`。计划由 `open_position_action` 派生，其输入的链参数
/// 来自收口点 `roster_chain_params`、权益来自 `equity_cents_with_pending`
/// （→ `cash/held_..._with_pending` → `roster_slice`）。
///
/// 判据=**跨组数不变式**：账户被分到组 0 还是组 1 是纯调度事实，不得
/// 影响它自己的决策产物（单组 `roster_offset==0` 恒正确 ⇒ 单组指纹即
/// oracle）。这是 Sol 阻断项要的"调用点传对参数"证明。
///
/// 实测覆盖范围（mutation 结论，勿夸大）：
/// - 回退 `run_chain_decision` 的 `roster_chain_params`（group.rs:2858）
///   → **显红**；
/// - 回退 `chain_quote_children` 的 `roster_chain_params`（group.rs:3387）
///   → **显红**（两点分别验证——Sol：一处回退不能证明两点都受保护）；
/// - 回退 `roster_slice` 五处（2917/2987/3017/4622/4859，含改传全局切片
///   与恒空段两种形态）→ **仍绿**：本 fixture 无跨拍在途票，
///   `cash/held_..._with_pending` 的加回量恒为 0，该族失效在计划指纹
///   上不可观测。故本红证只钉 chain 参数族；roster_slice 族的调用层
///   红证仍需另立（需构造跨拍在途票的世界）。
fn assert_grouping_preserves_plans(ticks: usize) {
    let single = plan_fingerprints(1, ticks);
    let grouped = plan_fingerprints(2, ticks);
    assert!(
        !single.is_empty(),
        "{ticks} 拍单组须有开仓计划（空指纹=本红证未触达开仓面，\
         退化为假绿）"
    );
    assert_eq!(
        grouped, single,
        "{ticks} 拍：双组下各账户开仓计划须与单组逐字段相同——\
         组切分是纯调度事实，不得影响决策产物。\
         差异即某个 `roster_offset` 调用点传错（组 g≥1 读到组 0 的\
         链参数/现金/持仓）。\n单组={single:#?}\n双组={grouped:#?}"
    );
}

/// 短程（4 拍）：首开仓面（链参数 + 权益 → 目标量）。
#[test]
fn multi_group_open_plans_match_single_group_at_4_ticks() {
    assert_grouping_preserves_plans(4);
}

/// 中程（8 拍）：开仓 + 复审面（`equity_cents_with_pending` 的复审权益
/// 读面——单拍覆盖不到）。
#[test]
fn multi_group_open_plans_match_single_group_at_8_ticks() {
    assert_grouping_preserves_plans(8);
}

/// 长程（12 拍）：跨拍成交推进（`apply_fill_plan_progress` 的 owner
/// 解析面 + 二次开仓）。
#[test]
fn multi_group_open_plans_match_single_group_at_12_ticks() {
    assert_grouping_preserves_plans(12);
}
