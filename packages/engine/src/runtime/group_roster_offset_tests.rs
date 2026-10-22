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
