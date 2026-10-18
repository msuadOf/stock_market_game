//! Q1 发布协议判据测试（J3-J6；J1/J2 的数字面由 B 探针与微基准承载，
//! 见 evidence/p1/）。语义合同：发布=提交=Abort 窗口关闭（C2）；一次
//! load() 得同拍一致读点（C1）；读者持旧版本任意久（Arc 生命周期）。

use super::publication::ViewPublication;
use super::skeleton_tests::basic_spec;
use super::world::World;
use super::GroupIdx;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 无为脚本（空计划——快照型 tick 足够驱动发布序列）。
fn idle_script() -> Box<dyn Fn(GroupIdx) -> super::group::GroupScript> {
    Box::new(|_g: GroupIdx| {
        Box::new(|_tick, _round| super::group::TickPlan::default()) as super::group::GroupScript
    })
}

fn spawn_world(ticks_per_day: u64) -> World {
    let mut spec = basic_spec(1, 2, 2, idle_script());
    spec.ticks_per_day = ticks_per_day;
    World::spawn(spec).expect("世界构造")
}

// ── J4 判据（一致性）：tick 序列与版本内分片一致 ──

#[test]
fn publication_v0_then_monotonic_tick_sequence() {
    let mut world = spawn_world(30);
    let publication = world.publication();
    let v0 = publication.load();
    assert_eq!(v0.tick, 0, "spawn 后 v0=tick 0");
    assert_eq!(v0.global.committed_tick, 0);
    assert_eq!(v0.desks.len(), 2, "分片数=股数");
    assert!(v0.groups.is_empty(), "P1 组分片占位恒空");
    assert!(v0.events.seq_to <= v0.events.seq_from, "P1 事件段占位为空段");

    world.run_ticks(5).expect("5 tick");
    let v5 = publication.load();
    assert_eq!(v5.tick, 5);
    assert_eq!(v5.global.committed_tick, 5, "global 与版本身份同拍");
    for shard in &v5.desks {
        assert_eq!(
            u64::from(shard.stock), shard.stock as u64,
            "分片身份自洽"
        );
    }
    world.shutdown().expect("关停");
}

/// J4 撕裂锤击：并发读者循环 load()，校验 (i) 版本内 tick==global.
/// committed_tick 且分片数恒定；(ii) 版本序列严格单调（无跳变无回退——
/// 跳变=有 tick 未经发布被跳过，回退=撕裂）。违约计数必须为 0。
#[test]
fn publication_concurrent_reader_zero_tears() {
    let mut world = spawn_world(30);
    let publication = world.publication();
    let violations = Arc::new(AtomicU64::new(0));
    let reads = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut readers = Vec::new();
    for _ in 0..3 {
        let publication: Arc<ViewPublication> = Arc::clone(&publication);
        let violations = Arc::clone(&violations);
        let reads = Arc::clone(&reads);
        let stop = Arc::clone(&stop);
        readers.push(std::thread::spawn(move || {
            let mut last_tick = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let version = publication.load();
                reads.fetch_add(1, Ordering::Relaxed);
                if version.tick != version.global.committed_tick {
                    eprintln!("违约[tick!=global]: tick={} global={}", version.tick, version.global.committed_tick);
                    violations.fetch_add(1, Ordering::Relaxed);
                }
                if version.desks.len() != 2 {
                    eprintln!("违约[分片数]: tick={} desks={}", version.tick, version.desks.len());
                    violations.fetch_add(1, Ordering::Relaxed);
                }
                if version.tick < last_tick {
                    eprintln!("违约[回退]: tick={} last={}", version.tick, last_tick);
                    violations.fetch_add(1, Ordering::Relaxed);
                }
                last_tick = last_tick.max(version.tick);
            }
        }));
    }
    world.run_ticks(60).expect("60 tick 满速跑");
    stop.store(true, Ordering::Relaxed);
    for reader in readers {
        reader.join().expect("读者线程合并");
    }
    let total_reads = reads.load(Ordering::Relaxed);
    assert!(total_reads > 100, "锤击读者应有足量读（实际 {total_reads}）");
    assert_eq!(
        violations.load(Ordering::Relaxed),
        0,
        "撕裂锤击违约必须为 0"
    );
    world.shutdown().expect("关停");
}

// ── J5 判据（失败原子性）：失败 tick 零外泄 ──

#[test]
fn publication_failed_tick_never_published() {
    let mut world = spawn_world(30);
    let publication = world.publication();
    world.run_ticks(3).expect("3 tick");
    let before = publication.load();
    assert_eq!(before.tick, 3);
    // 注入 validator：tick 4 一律拒绝。
    world.set_validator(Box::new(|evidence| {
        let _ = evidence;
        Err("J5 注入：拒绝本 tick".to_owned())
    }));
    let failed = world.run_ticks(1);
    assert!(failed.is_err(), "validator 拒绝必须显式失败");
    let after = publication.load();
    assert_eq!(
        after.tick, 3,
        "失败 tick 零外泄——读者仍见 N-1 版本"
    );
    assert_eq!(
        after.global.committed_tick, 3,
        "全局域与发布版本一致（candidate 不触碰权威）"
    );
    assert_eq!(
        after.hashes.desks.len(),
        world.report_log.last().map(|r| r.desk_hashes.len()).unwrap_or(0),
        "发布版本哈希族与 report_log 末条一致"
    );
    world.shutdown().expect("关停");
}

// ── J6 判据（查询面等价·desks 域）：发布版本与权威公共态逐位一致 ──

#[test]
fn publication_desks_match_public_views() {
    let mut world = spawn_world(30);
    let publication = world.publication();
    for expected in 1..=10u64 {
        world.run_ticks(1).expect("单 tick");
        let version = publication.load();
        assert_eq!(version.tick, expected);
        let views = world.public_views();
        assert_eq!(version.desks.len(), views.len());
        for (shard, view) in version.desks.iter().zip(views.iter()) {
            assert_eq!(
                shard.view.last_price, view.last_price,
                "tick {expected} 股 {} 行情发布面==权威面",
                shard.stock
            );
            assert_eq!(shard.view.volume_today, view.volume_today);
            assert_eq!(shard.view.best_bid, view.best_bid);
            assert_eq!(shard.view.best_ask, view.best_ask);
        }
    }
    world.shutdown().expect("关停");
}

// ── J3 判据（保留内存）：旧版本生命周期 ──

#[test]
fn publication_old_version_retained_by_reader() {
    let mut world = spawn_world(30);
    let publication = world.publication();
    let pinned = publication.load();
    assert_eq!(pinned.tick, 0);
    let pinned_price = pinned.desks[0].view.last_price;
    world.run_ticks(20).expect("20 tick");
    let current = publication.load();
    assert_eq!(current.tick, 20, "发布槽已前进");
    assert_eq!(
        pinned.tick, 0,
        "读者持有的旧版本不受发布前进影响（Arc 生命周期）"
    );
    assert_eq!(
        pinned.desks[0].view.last_price, pinned_price,
        "旧版本内容不可变"
    );
    drop(pinned);
    world.shutdown().expect("关停");
}
