//! Seal 攻顶刀 A-1'（GroupAccount 域 InlineVec 内联化）合同测试。
//! 设计稿：agents/view-campaign/plans/seal-floor-decomposition-design.md
//! §3（协调者批+三守卫）。批 1：InlineVec 类型（真实现——合同 3 的
//! 载体）+分片等价锚（合同 1/2 现状绿基线——批 2 迁移后必须仍绿）
//! +旋钮桩红先行（分片构建未消费旋钮 ⇒ 无效果 ⇒ 判别断言红）。
//!
//! 守卫映射：①容量数据驱动=现役口径 P100=5 股（A/B 探针 5、fixture 2）
//! →内联容量 8；spill（>8）基准在批 2 落地（本文件 spill 等价合同先行）。
//! ②等价面=Deref 切片+按内容 PartialEq——分片对账/断言零变体歧义。

use super::group::{GroupAccount, InlineVec};
use super::skeleton_tests::basic_spec;
use super::world::World;
use crate::orderbook::Side;

/// 成交 fixture（seal_tests 同款）：tick1 挂对手、tick2 起撮合。
fn trading_script() -> Box<dyn Fn(super::GroupIdx) -> super::group::GroupScript> {
    Box::new(move |_g| {
        Box::new(move |tick: u64, _round: u32| {
            let mut plan = super::group::TickPlan::default();
            if tick == 1 {
                plan.places.push(super::group::PlaceSpec {
                    local: 1,
                    stock: 0,
                    side: Side::Sell,
                    price: 1000,
                    qty: 100,
                });
            }
            if tick == 2 {
                plan.places.push(super::group::PlaceSpec {
                    local: 2,
                    stock: 0,
                    side: Side::Buy,
                    price: 1000,
                    qty: 100,
                });
            }
            plan
        })
    })
}

/// 合同 3（InlineVec 内容等价守卫——类型随批 1 建，现状即绿防回归）：
/// Inline/Heap 两路径的 filled/clone/索引/切片逐位一致；**按内容判等**
///（Inline 与 Heap 同内容必须相等——分片对账面的变体歧义守卫）；
/// spill 边界（len=N 内联/len=N+1 堆）两侧语义一致。
#[test]
fn inline_vec_content_equivalence() {
    type IV = InlineVec<u32, 8>;
    // 内联路径。
    let a = IV::filled(5, 7);
    assert_eq!(a.len(), 5, "deref 切片长度");
    assert!(a.iter().all(|&x| x == 7), "填充值");
    // clone 保内容（栈拷贝路径）。
    let a2 = a.clone();
    assert_eq!(a, a2, "Inline clone 内容相等");
    assert_eq!(a.as_slice(), a2.as_slice());
    // 可变切片写（长度不变）。
    let mut b = IV::filled(5, 0);
    b.as_mut_slice()[3] = 42;
    assert_eq!(b[3], 42, "索引写读一致");
    // spill 边界：len=8 内联 / len=9 堆——两变体同语义。
    let inline_full = IV::filled(8, 9);
    let heap = IV::filled(9, 9);
    assert_eq!(inline_full.len(), 8);
    assert_eq!(heap.len(), 9);
    assert_eq!(heap.last(), Some(&9), "spill 堆路径末位");
    // **按内容判等守卫**：同内容 Inline vs Heap 相等（手写 PartialEq——
    /// 变体不参与判等）。
    let same_inline = IV::filled(9, 9);
    match (&same_inline, &heap) {
        (InlineVec::Heap(_), InlineVec::Heap(_)) => {}
        other => panic!("len=9 恒堆变体（构造不变量）：{other:?}"),
    }
    // 同内容异构判等（用 length 相同的两实例：一个构造期即堆、一个
    /// 由内联 clone 后扩……——本类型无扩容接口，此面由 PartialEq 按内容
    /// 的实现直接验证：同长度同内容必等）。
    let x = IV::filled(5, 3);
    let y = IV::filled(5, 3);
    assert_eq!(x, y, "同构造内容相等");
    let z = IV::filled(5, 4);
    assert_ne!(x, z, "内容差异判不等");
}

/// 合同 1（分片内容等价锚——现状绿，批 2 迁移后必须仍绿）：发布版本
/// 的 SealAck 账户分片（groups[].accounts）逐条内容==组权威全量读面
/// （snapshot_accounts）同组同 local 账户——分片是拍内 dirty 子集 ⊆
/// 全量，条目内容必须逐位一致（InlineVec 迁移不改变等价面）。
#[test]
fn seal_shard_matches_group_authority() {
    let mut spec = basic_spec(2, 1, 4, trading_script());
    spec.ticks_per_day = 30;
    spec.auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(2).expect("2 tick（tick2 成交拍——分片=当拍 dirty，take 清空式消费后末拍须即成交拍对照）");
    // 分片（发布版本——SealAck 上行）。
    let published = world.publication().load();
    assert_eq!(published.tick, 2, "发布版本推进到成交拍");
    assert!(!published.groups.is_empty(), "组分片在位");
    let mut shard_entries = 0usize;
    for shard in &published.groups {
        for (local, account) in shard.accounts.iter() {
            shard_entries += 1;
            // 组权威全量读面：同组同 local 的账户内容对照。
            let authority = world
                .snapshot_accounts()
                .expect("组权威全量读面")
                .into_iter()
                .find(|(g, _)| *g == shard.group)
                .map(|(_, accounts)| accounts[*local as usize].clone())
                .expect("全量读面含该组");
            assert_eq!(
                format!("{account:?}"),
                format!("{authority:?}"),
                "组 {} 位 {local} 分片内容==组权威（InlineVec 迁移等价锚）",
                shard.group
            );
        }
    }
    assert!(shard_entries > 0, "成交 fixture 须有 dirty 分片条目");
    world.shutdown().expect("关停");
}

/// 合同 2（Abort 不复活锚）：SealAck 失败注入→Abort→重试成功——重试
/// 后的分片==新终态（旧分片不复活；InlineVec 迁移不改变 Abort 语义）。
#[test]
fn seal_shard_abort_retry_fresh() {
    let mut spec = basic_spec(1, 1, 4, trading_script());
    spec.ticks_per_day = 30;
    spec.auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    world.run_ticks(2).expect("2 tick 基线");
    world.set_seal_ack_failure_once_for_test();
    assert!(world.run_ticks(1).is_err(), "注入的 SealAck 失败显式失败");
    world.run_ticks(1).expect("重试成功（worker 存活）");
    let published = world.publication().load();
    for shard in &published.groups {
        let authority = world
            .snapshot_accounts()
            .expect("组权威读面")
            .into_iter()
            .find(|(g, _)| *g == shard.group)
            .map(|(_, accounts)| accounts)
            .expect("组在册");
        for (local, account) in shard.accounts.iter() {
            assert_eq!(
                format!("{account:?}"),
                format!("{:?}", authority[*local as usize]),
                "Abort 重试后分片==新终态（组 {} 位 {local}）",
                shard.group
            );
        }
    }
    world.shutdown().expect("关停");
}

/// 旋钮 1（突变红判别——批 1 桩红先行）：开篡改旋钮后分片首条目
/// cash 必须偏离组权威 +1 分。批 1 桩：分片构建未消费 ⇒ 无偏差 ⇒
/// 本测试红；批 2 消费后翻转为负控红证（开旋钮→合同 1 形态红）。
#[test]
fn seal_knob1_tamper_proves_shard_source() {
    let mut spec = basic_spec(1, 1, 4, trading_script());
    spec.ticks_per_day = 30;
    spec.auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    world.seal_knobs_for_test(true, false);
    world.run_ticks(2).expect("2 tick（成交拍）");
    let published = world.publication().load();
    let mut tampered = 0usize;
    for shard in &published.groups {
        let authority = world
            .snapshot_accounts()
            .expect("组权威读面")
            .into_iter()
            .find(|(g, _)| *g == shard.group)
            .map(|(_, accounts)| accounts)
            .expect("组在册");
        if let Some((local, account)) = shard.accounts.first() {
            if account.cash == authority[*local as usize].cash + 1 {
                tampered += 1;
            }
        }
    }
    assert!(
        tampered > 0,
        "旋钮 1 开启后至少一组首条目 cash+1 偏离权威（批 1 红证：分片构建未消费旋钮）"
    );
    world.shutdown().expect("关停");
}

/// 旋钮 2（dirty 集完整性判别——批 1 桩红先行）：开漏末条目旋钮的
/// 世界 vs 同 seed 干净世界——分片长度必须恰少 1。批 1 桩（未消费）
/// ⇒ 两份等长 ⇒ 断言红；批 2 消费后差==1 绿（dirty 集完整性负控）。
#[test]
fn seal_knob2_skip_proves_dirty_completeness() {
    fn shard_len(with_knob: bool) -> usize {
        let mut spec = basic_spec(1, 1, 4, trading_script());
        spec.ticks_per_day = 30;
        spec.auction_ticks = 0;
        let mut world = World::spawn(spec).expect("世界构造");
        if with_knob {
            world.seal_knobs_for_test(false, true);
        }
        world.run_ticks(2).expect("2 tick（成交拍）");
        let len: usize = world
            .publication()
            .load()
            .groups
            .iter()
            .map(|shard| shard.accounts.len())
            .sum();
        world.shutdown().expect("关停");
        len
    }
    let clean = shard_len(false);
    let skipped = shard_len(true);
    assert!(
        clean >= 2,
        "fixture 须有 ≥2 条 dirty（漏末条目可观察）——实际 {clean}"
    );
    assert_eq!(
        skipped + 1,
        clean,
        "旋钮 2 开启后分片恰少 1 条（批 1 红证：未消费时两份等长）"
    );
}
