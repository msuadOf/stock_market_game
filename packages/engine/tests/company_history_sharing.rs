//! 公司日结历史的批次共享负控（架构项 #13）。
//!
//! 真实热路径：自然日日结在旧快照（tick shadow / 日结 checkpoint）仍存活的
//! 情况下经 `Arc::make_mut` 推进 `CompanyOperations`——每公司 journal 的已
//! 积累批次必须按新增批次共享（复用 AppendOnlyHistory 模式），不得每次
//! 复制整个已积累 Vec。本文件用引擎内建的 `JournalEntry` 深拷贝计数做规模
//! 递增负控：历史 8 倍增长时，单日推进的分录深拷贝次数必须保持同量级
//! （O(新增)，而非 O(全历史)）。
//!
//! 隔离：本二进制内的测试用静态互斥串行，计数窗口内只有被测单线程路径在
//! 运行。

use std::sync::{Arc, Mutex, OnceLock};

fn measurement_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// 复用 company_operations 的四行业装配（真实日结路径）；#[path] 共享同一文件，
// 不复制 fixture。
#[path = "company_operations/fixtures.rs"]
mod fixtures;

use engine::company::operations::CompanyOperations;
use engine::company::CompanyId;

fn build_with_history(days: usize) -> CompanyOperations {
    let start = fixtures::d("2030-01-05");
    let mut ops = CompanyOperations::new(
        fixtures::four_company_config(21, fixtures::quiet_params(), fixtures::d("2030-01-04")),
        start,
    )
    .expect("ops assembles");
    let mut day = start;
    for _ in 0..days {
        ops.advance_civil_day(day).expect("history day advances");
        day = day.next().expect("next civil day");
    }
    ops
}

/// 在「旧快照仍存活」的真实日结语义下（session 用 Arc::make_mut），
/// 推进一个自然日的存活字节增量（深拷贝的批次被新旧两份持有，全部计入）。
fn measured_day_end_advance(ops: &mut Arc<CompanyOperations>) -> u64 {
    let snapshot = Arc::clone(ops);
    let before = engine::accounting::journal_entry_clone_count();
    let next = ops.next_expected_date();
    std::sync::Arc::make_mut(ops)
        .advance_civil_day(next)
        .expect("measured day advances");
    let after = engine::accounting::journal_entry_clone_count();
    drop(snapshot);
    after - before
}

#[test]
fn day_end_append_cost_stays_flat_as_history_grows() {
    let _guard = measurement_lock().lock().unwrap_or_else(|poison| poison.into_inner());
    let mut small = Arc::new(build_with_history(20));
    let mut large = Arc::new(build_with_history(160));
    let small_clones = measured_day_end_advance(&mut small);
    let large_clones = measured_day_end_advance(&mut large);
    // 绿日志也保留绝对数（验收 report 捕获 stdout，供规模复核直接读取）。
    println!("day_end_append_clone_counts small={small_clones} large={large_clones}");
    assert!(
        large_clones < small_clones.saturating_mul(3),
        "历史 8 倍增长时单日推进的分录深拷贝必须保持同量级：small={small_clones} large={large_clones}"
    );
}

#[test]
fn long_history_keeps_facts_order_and_roundtrips_identically() {
    let _guard = measurement_lock().lock().unwrap_or_else(|poison| poison.into_inner());
    let ops = build_with_history(120);
    let ids: Vec<CompanyId> = ops.catalog().iter().map(|(id, _)| id).collect();
    let counts: usize = ids
        .iter()
        .map(|id| ops.company(id).unwrap().books().books().journal().entry_count())
        .sum();
    assert!(counts > 1000, "120 日四行业须积累足量分录：{counts}");
    let wire = serde_json::to_vec(&ops).expect("serializes");
    let restored: CompanyOperations = serde_json::from_slice(&wire).expect("restores");
    let restored_counts: usize = ids
        .iter()
        .map(|id| restored.company(id).unwrap().books().books().journal().entry_count())
        .sum();
    assert_eq!(restored_counts, counts, "存档投影不得丢批");
    assert_eq!(
        serde_json::to_vec(&restored).unwrap(),
        wire,
        "roundtrip 后再序列化必须字节一致"
    );
}

#[test]
fn surviving_snapshot_stays_frozen_while_history_advances() {
    let _guard = measurement_lock().lock().unwrap_or_else(|poison| poison.into_inner());
    let mut ops = Arc::new(build_with_history(40));
    let snapshot = Arc::clone(&ops);
    let snapshot_wire = serde_json::to_vec(snapshot.company(&CompanyId(1)).unwrap())
        .expect("snapshot serializes");
    let snapshot_count = snapshot
        .company(&CompanyId(1))
        .unwrap()
        .books()
        .books()
        .journal()
        .entry_count();
    let next = ops.next_expected_date();
    std::sync::Arc::make_mut(&mut ops)
        .advance_civil_day(next)
        .expect("advances");
    assert_eq!(
        serde_json::to_vec(snapshot.company(&CompanyId(1)).unwrap()).unwrap(),
        snapshot_wire,
        "推进不得改写旧快照的任何历史事实"
    );
    assert_eq!(
        snapshot
            .company(&CompanyId(1))
            .unwrap()
            .books()
            .books()
            .journal()
            .entry_count(),
        snapshot_count
    );
    assert!(
        ops.company(&CompanyId(1))
            .unwrap()
            .books()
            .books()
            .journal()
            .entry_count()
            > snapshot_count,
        "新事实只进入推进后的副本"
    );
}

#[test]
fn concurrent_final_owner_drop_releases_long_history_without_recursion() {
    let _guard = measurement_lock().lock().unwrap_or_else(|poison| poison.into_inner());
    let ops = Arc::new(build_with_history(80));
    let clone = Arc::clone(&ops);
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let thread_barrier = barrier.clone();
    let other = std::thread::spawn(move || {
        thread_barrier.wait();
        drop(clone);
    });
    barrier.wait();
    drop(ops);
    other.join().expect("并发释放线程 panic");
}

#[test]
fn failed_advance_leaves_mutable_copy_unchanged_after_make_mut() {
    let _guard = measurement_lock().lock().unwrap_or_else(|poison| poison.into_inner());
    let mut ops = Arc::new(build_with_history(15));
    let before = serde_json::to_vec(&ops).expect("serializes");
    let snapshot = Arc::clone(&ops);
    let wrong_date = ops
        .next_expected_date()
        .next()
        .expect("跳过一日即非法");
    assert!(
        std::sync::Arc::make_mut(&mut ops)
            .advance_civil_day(wrong_date)
            .is_err(),
        "跳日必须显式失败"
    );
    assert_eq!(
        serde_json::to_vec(&ops).unwrap(),
        before,
        "失败推进（含 make_mut 触发的复制路径）不得残留任何批次/索引改动"
    );
    drop(snapshot);
}
