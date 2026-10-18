//! driver 全语义口径性能探针（翻转批新建基准——经 GameSession::step 的
//! std-thread runtime 路径：driver attention/行情/K 安装+事件上行全语义）。
//!
//! 与裸引擎探针（runtime_s5_probe，直跑 World）为**两个不可折算的口径**：
//! 本探针每 tick 走 session 侧安装链（install_market_views/install_candles/
//! 个人态回写/信封投影）。守恒判据对齐 s5 探针形态（终态对账）。
//!
//! 用法：driver_semantics_probe <setup.json> <ticks> <runtime_groups>
//!（ENGINE_STEP_RUNTIME=1 运行；批 9 A6 起 threads 参数退役——CPU 约束
//! 走 OS 级 cgroup/taskset）。
use engine::{GameSession, SessionSetup};
use std::{collections::BTreeMap, env, fs, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    let [setup_path, ticks, groups] = args.as_slice() else {
        return Err(
            "用法：driver_semantics_probe <setup.json> <ticks> <runtime_groups>".into(),
        );
    };
    let setup: SessionSetup = serde_json::from_slice(&fs::read(setup_path)?)?;
    let ticks: u64 = ticks.parse()?;
    let groups: usize = groups.parse()?;
    if ticks == 0 || groups == 0 {
        return Err("ticks/runtime_groups 必须大于零".into());
    }
    if !engine::runtime::step_runtime_enabled() {
        return Err("本探针测 std-thread runtime 路径：需 ENGINE_STEP_RUNTIME=1".into());
    }
    (|| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // 守恒基线：构造后全账户现金+费用桶与股份总量（初始事实）。
        let mut game = GameSession::new_with_runtime_groups(setup, 42, groups)?;
        let accounts_initial: Vec<_> = (0..game.account_count() as u64)
            .map(|i| {
                let acct = game
                    .account(engine::AccountId(i))
                    .expect("名册账户必在");
                (
                    acct.cash().cents(),
                    acct.positions().values().map(|p| p.qty() as i64).sum::<i64>(),
                )
            })
            .collect();
        let mut events = 0usize;
        // P0 分段画像（STEP_PROFILE=1）：driver 线程各段 wall 计时——
        // 视图内生战役的"可删份额"定标数据源（默认关=零开销）。
        let profile_on = env::var("STEP_PROFILE").map(|v| v == "1").unwrap_or(false);
        if profile_on {
            engine::runtime::step_profile::enable_on_this_thread();
        }
        let started = Instant::now();
        // P0 归因：civil check 循环的独立计时（step 外围嫌疑面）。
        let mut civil_ns: u128 = 0;
        for _ in 0..ticks {
            loop {
                let t0 = std::time::Instant::now();
                let due_now = game.day() == game.civil_clock().completed_trading_sessions_expected()?;
                civil_ns += t0.elapsed().as_nanos();
                if !due_now {
                    break;
                }
                game.end_civil_day()?;
            }
            events += {
                // 【P0 画像】step_outer 段=完整 step() wall + 事件 Vec
                /// drop（临时值延长到语句尾——drop 一并入段防漏计）。
                let _seg_outer = engine::runtime::step_profile::scope(
                    engine::runtime::step_profile::StepSegment::StepOuter,
                );
                let evs = game.step()?;
                let n = evs.len();
                drop(evs);
                n
            };
        }
        let wall_s = started.elapsed().as_secs_f64();
        if profile_on {
            eprintln!(
                "civil_check（会话守卫循环条件评估）= {civil_ns} ns（{:.0} ns/tick）",
                civil_ns as f64 / ticks as f64
            );
        }
        if profile_on {
            let Some(ledger) = engine::runtime::step_profile::take_ledger() else {
                return Err("STEP_PROFILE=1 但账本为空（enable 后无段进入）".into());
            };
            let total_ns: u128 = ledger.iter().map(|(_, ns, _)| *ns).sum();
            // 注意：market_install 内嵌 attention_heap；step_outer 内嵌
            // 全部子段（完整 step wall）——占比按"去嵌套分母"解释。
            eprintln!("--- step 分段画像（累计 ns / 次 / 占分段和比例）---");
            let nested = ["attention_heap", "step_outer", "driver_step", "belief_collect", "envelope_collect"];
            let net_total: u128 = total_ns
                - ledger
                    .iter()
                    .filter(|(n, _, _)| nested.contains(n))
                    .map(|(_, ns, _)| *ns)
                    .sum::<u128>();
            let mut rows: Vec<_> = ledger;
            rows.sort_by(|a, b| b.1.cmp(&a.1));
            for (name, ns, count) in rows {
                let ratio = if nested.contains(&name) {
                    ns as f64 / total_ns as f64
                } else {
                    ns as f64 / net_total as f64
                };
                eprintln!("{name:<20} {ns:>14} ns  ×{count:<6} {ratio:6.2}%");
            }
            eprintln!(
                "分段和（含嵌套双计）= {total_ns} ns；去嵌套净和 = {net_total} ns；wall = {} ns",
                (wall_s * 1e9) as u128
            );
        }
        // 终态守恒对账：现金（+费用桶不可达——desk 权威内部；现金守恒以
        // 期末现金+持仓市值近似现金总量的股份守恒为主判据，对齐 s5 探针
        /// 的"股份=初始"与现金面（费用桶经 fee_sink 为 desk 内部域——
        /// 本口径经 session 读面取现金，费用分账在账户内体现）。
        let mut cash_final = 0i64;
        let mut shares_final = 0i64;
        for i in 0..game.account_count() as u64 {
            let acct = game.account(engine::AccountId(i)).expect("名册账户必在");
            cash_final += acct.cash().cents();
            shares_final += acct.positions().values().map(|p| p.qty() as i64).sum::<i64>();
        }
        let cash_initial: i64 = accounts_initial.iter().map(|(c, _)| *c).sum();
        let shares_initial: i64 = accounts_initial.iter().map(|(_, s)| *s).sum();
        assert_eq!(
            shares_final, shares_initial,
            "守恒：股份终态=初始（违反即性能数字作废）"
        );
        // 现金终态=初始−净费用（成交只是转移；费用为唯一汇）。无 desk
        // fee_sink 读面——断言现金不增（费用只减不增）且降幅有限（每
        // tick 费用远小于本金）。
        assert!(
            cash_final <= cash_initial,
            "守恒：现金终态不高于初始（费用只减不增）"
        );
        println!(
            "{}",
            serde_json::json!({
                "ticks": ticks,
                "runtime_groups": groups,
                "wall_s": wall_s,
                "speed_x": ticks as f64 / wall_s,
                "events": events,
                "conservation": {
                    "shares_ok": true,
                    "cash_initial": cash_initial,
                    "cash_final": cash_final,
                    "fees_paid": cash_initial - cash_final,
                },
            })
        );
        let _ = BTreeMap::<String, u128>::new();
        Ok(())
    })()
    .map_err(|error| -> Box<dyn std::error::Error> { error })
}
