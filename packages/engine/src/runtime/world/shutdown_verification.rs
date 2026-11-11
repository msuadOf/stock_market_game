//! 仅验证feature可达的短World fixture；执行真实caller，不替代worker或join。
use super::*;
use std::sync::atomic::AtomicU32;

/// 同一case独占的共享观测字；wasm trap后宿主仍可读取阶段和真实join索引。
#[repr(C)]
pub struct WorldStopProbe {
    words: [AtomicU32; 16],
}

impl WorldStopProbe {
    pub fn new(mode: u32) -> Result<Self, String> {
        if mode > 4 {
            return Err("World stop验证mode必须为0..4".to_owned());
        }
        let probe = Self {
            words: [const { AtomicU32::new(0) }; 16],
        };
        probe.words[0].store(mode, Ordering::SeqCst);
        Ok(probe)
    }

    pub fn address(&self) -> u32 {
        self.words.as_ptr() as usize as u32
    }
    pub(super) fn mode(&self) -> u32 {
        self.words[0].load(Ordering::SeqCst)
    }
    fn stage(&self, stage: u32) {
        self.words[1].store(stage, Ordering::SeqCst);
    }
    pub(super) fn note_bind_failure(&self) {
        self.stage(40);
    }

    pub(super) fn record_handles(
        &self,
        handles: &[(
            WorkerId,
            WorldWorkerHandle,
        )],
    ) {
        assert_eq!(handles.len(), 4, "fixture必须拥有2组2部真实handles");
        for (index, (who, handle)) in handles.iter().enumerate() {
            self.words[4 + index].store(handle.join_index(), Ordering::SeqCst);
            self.words[8 + index].store(
                match who {
                    WorkerId::Group(g) => u32::from(*g),
                    WorkerId::Desk(s) => 0x10000 + u32::from(*s),
                },
                Ordering::SeqCst,
            );
        }
        self.words[3].store(4, Ordering::SeqCst);
        self.stage(20);
    }
}

pub(super) fn inject_calendar_binding_failure(
    probe: &WorldStopProbe,
    groups: &[super::super::downstream::Sender<GroupIn>],
    desks: &mut [super::super::downstream::Sender<DeskMsg>],
    up: &super::super::upstream::UpReceiver<UpMsg>,
    deadline: &super::wait_contract::DeadlineConfig,
) {
    assert_eq!(probe.mode(), 4);
    probe.stage(30);
    groups[0]
        .send(GroupIn::Continue { tick: 0, round: 0 })
        .expect("真实失序Continue入队");
    match recv_deadline(up, super::wait_contract::Phase::TickConverge, deadline).expect("必须先收到原group错误握手")
    {
        UpMsg::Failed {
            who: WorkerId::Group(0),
            error: RuntimeError::ProtocolInvariant { detail },
        } if detail.contains("Continue 错序") => {}
        other => panic!("Calendar故障注入未得到预期原worker错误：{other:?}"),
    }
    probe.words[2].store(1, Ordering::SeqCst);
    let (disconnected, receiver) = super::super::downstream::channel().expect("验证用下行故障槽");
    drop(receiver);
    desks[0] = disconnected;
    // guard独立wake仍保留原活desk的Sender；随后运行真实BindCalendar send/map_err?。
    probe.stage(35);
}

fn short_spec() -> WorldSpec {
    let date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("验证交易日期合法");
    WorldSpec {
        initial_plan_books: Vec::new(),
        initial_repurchase_projections: Default::default(),
        admin_treasury_bindings: Vec::new(),
        n_groups: 2,
        accounts_per_group: 1,
        initial_cash: 1_000_000,
        initial_shares: vec![vec![vec![1000; 2]]; 2],
        initial_invested: Vec::new(),
        initial_recovered: Vec::new(),
        initial_ever_held: Vec::new(),
        initial_frozen_today: Vec::new(),
        initial_purse_held: Vec::new(),
        initial_pending_sell: Vec::new(),
        script_for: Box::new(|_| Box::new(|_, _| super::super::group::TickPlan::default())),
        chunk: 8,
        deadline: super::wait_contract::DeadlineConfig {
            normal: Duration::from_secs(5),
            long: Duration::from_secs(5),
        },
        rounds_cap: 4,
        stocks: vec![DeskStockSpec::main_board(1000); 2],
        config: Arc::new(GameConfig::proposed_defaults()),
        ticks_per_day: 30,
        auction_ticks: 0,
        closing_auction_ticks: 3,
        order_ttl_ticks: 100,
        history_len: 4,
        initial_committed_tick: 0,
        initial_last_day_end_tick: None,
        decision: None,
        rights_computer: None,
        initial_cash_override: None,
        plan_factory: None,
        belief_seeds: Vec::new(),
        library: None,
        initial_desk_orders: Vec::new(),
        initial_desk_next_serial: Vec::new(),
        root_projection: None,
        civil_start_date: date,
        calendar_start_date: date,
    }
}

fn spawn(probe: &WorldStopProbe) -> Result<World, RuntimeError> {
    let spec = short_spec();
    let bindings = vec![
        (crate::StockId(0), crate::calendar::CalendarExchange::Sse),
        (crate::StockId(1), crate::calendar::CalendarExchange::Sse),
    ];
    let source = crate::session::market_calendar_context::MarketCalendarSource::new(
        crate::calendar::TradingCalendar::current_default_calendar().expect("显式Calendar合法"),
        spec.calendar_start_date,
        bindings.clone(),
    )
    .expect("短fixture Calendar来源合法");
    probe.stage(10);
    World::spawn_with_calendar_source(spec, source, bindings, Some(probe))
}

/// 0=fatal Start，1=sever后Drop，2=直接Drop，3=prepared回收，4=Calendar构造错误。
pub fn verify_world_stop(mode: u32, probe: &WorldStopProbe) -> Result<String, String> {
    if probe.mode() != mode {
        return Err("World stop probe与case不一致".to_owned());
    }
    if mode == 4 {
        let error = match spawn(probe) {
            Err(error) => error,
            Ok(world) => {
                drop(world);
                return Err("Calendar故障未触发真实构造错误".to_owned());
            }
        };
        let RuntimeError::ConstructionFailure { primary, joined_workers, cleanup_errors } = error else {
            return Err(format!("错误类型丢失：{error}"));
        };
        if primary != "交易部 Calendar owner 绑定失败" || joined_workers != 4
            || !cleanup_errors.iter().any(|error| error.contains("group-0") && error.contains("Continue 错序")) {
            return Err(format!("构造错误或回收事实不符：primary={primary} handles={joined_workers} cleanup={cleanup_errors:?}"));
        }
        probe.stage(60);
        return Ok(format!("{primary}；已消费 {joined_workers} 个 worker 句柄；回收错误={cleanup_errors:?}"));
    }
    let mut world = spawn(probe).map_err(|error| error.to_string())?;
    let publication = world.publication();
    let before = publication.load();
    probe.stage(45);
    match mode {
        0 => {
            let (disconnected, receiver) = super::super::downstream::channel().expect("验证用下行故障槽");
            drop(receiver);
            world.group_txs[0] = disconnected;
            let error = world
                .run_ticks(1)
                .expect_err("部分Start必须进入真实fatal停止");
            assert!(matches!(
                error,
                RuntimeError::SendFailed { .. } | RuntimeError::ProtocolInvariant { .. }
            ));
            assert!(
                world.dead_reason.is_some()
                    && world.handles.is_empty()
                    && world.shutdown_wake.is_none()
            );
            drop(world);
        }
        1 => {
            world.sever();
            assert!(
                world.shutdown_wake.is_none()
                    && world.group_txs.is_empty()
                    && world.desk_txs.is_empty()
            );
            assert_eq!(world.handles.len(), 4);
            drop(world);
        }
        2 => drop(world),
        3 => {
            let report = world
                .stop_without_drain()
                .map_err(|error| error.to_string())?;
            assert_eq!(report.joined_workers, 4);
        }
        _ => return Err("未知World停止case".to_owned()),
    }
    assert!(
        Arc::ptr_eq(&before, &publication.load()),
        "私有回收不得安装Final或发布市场事实"
    );
    probe.stage(60);
    Ok(format!(
        "World真实停止case={mode}，4handles已消费，publication未推进"
    ))
}
