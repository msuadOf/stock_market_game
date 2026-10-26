//! INFO-P 短 fixture：真实观察点和逐股事实，不接生产披露时钟。
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::{CompanyId, CompanyKind, CompanyReadFacts};
use crate::runtime::protocol::{InformationObservation, InformationPreparePayload};
use crate::runtime::world::{BeliefParticipantSnapshot, RootProjection};
use crate::{AccountId, StockId};
use std::collections::BTreeMap;
use std::sync::Arc;

pub fn at(hour: u32, minute: u32) -> CivilInstant {
    CivilInstant::from_hms(CivilDate::from_iso("2031-04-01").unwrap(), hour, minute, 0).unwrap()
}
pub fn facts(shares: u64) -> Arc<CompanyReadFacts> {
    Arc::new(
        CompanyReadFacts::new(
            [(StockId(0), CompanyId(0)), (StockId(1), CompanyId(1))].into(),
            [
                (CompanyId(0), (CompanyKind::Industrial, shares)),
                (CompanyId(1), (CompanyKind::Industrial, shares)),
            ]
            .into(),
        )
        .unwrap(),
    )
}
pub fn personal(id: AccountId) -> BeliefParticipantSnapshot {
    let mut rng = crate::session::SplitMix64::new(id.0 + 17);
    let profile = crate::strategy::StrategyProfile::Institution(
        crate::strategy::InstitutionStyle::ActiveTrader,
    );
    let analysis = crate::strategy::derive_analysis_profile(&profile, id, &mut rng).unwrap();
    BeliefParticipantSnapshot {
        watchlist: Default::default(),
        price_memory: Default::default(),
        information: crate::information::NpcInformationState::new(id),
        belief: crate::strategy::BeliefBook::new(id, profile, analysis, &mut rng),
        attention: crate::session::NpcAttentionState {
            information_cadence: crate::session::notices::NpcInformationCadence::Daily {
                checks: 1,
            },
            next_information_check: at(9, 0),
            base_probability: 0.7,
            next_attention_candidate_tick: 31,
            rng_state: id.0 + 23,
        },
        history_reads: Default::default(),
        retail_experience: None,
    }
}
pub fn projection(ids: Vec<AccountId>) -> Arc<RootProjection> {
    Arc::new(RootProjection {
        catalog: crate::stock::StockCatalog::from_metas(vec![
            crate::stock::StockMeta::new("600000"),
            crate::stock::StockMeta::new("600001"),
        ])
        .unwrap(),
        company_registry: Arc::new(crate::company::CompanyRegistry::new(Vec::new()).unwrap()),
        operations: Arc::new(
            crate::company::CompanyOperations::new(
                crate::company::CompanyOperationsConfig {
                    seed: 1,
                    shock_params: crate::company::ShockParams::current_default_parameters(),
                    catalog: crate::company::CompanyCatalog::from_codes(&[]).unwrap(),
                    companies: Vec::new(),
                },
                at(9, 0).date(),
            )
            .unwrap(),
        ),
        chain_params: vec![
            Some((
                crate::strategy::BeliefChainParams {
                    position_step_bp: 1000,
                    margin: 0.1,
                    order_size: 100,
                    daily_plan_review: false
                },
                None
            ));
            ids.len()
        ],
        account_ids: ids,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        session_seed: 7,
        urgency_policy: Default::default(),
        total_shares: BTreeMap::new(),
        max_order_qty_limit: BTreeMap::new(),
        tick_size_cents: BTreeMap::new(),
        stock_codes: Vec::new(),
    })
}
pub fn payload(observe: bool) -> InformationPreparePayload {
    let current_facts = facts(10_000);
    let market = Arc::new(crate::strategy::MarketView {
        tick: 0,
        market_minute: 17,
        stocks: (0..2)
            .map(|n| {
                (
                    StockId(n),
                    crate::strategy::StockView {
                        best_bid: None,
                        best_ask: None,
                        last_price: crate::Money::from_cents(1000),
                        max_buy_price: crate::Money::from_cents(1100),
                        daily_upper_limit: crate::Money::from_cents(1100),
                        min_sell_price: crate::Money::from_cents(900),
                        recent_prices: Vec::new(),
                        recent_market_minute_prices: Vec::new(),
                        relative_volume: 1.0,
                        order_book_imbalance: 0.0,
                    },
                )
            })
            .collect(),
    });
    InformationPreparePayload {
        library: Arc::new(crate::information::PublicLibrary::new()),
        current_facts: Arc::clone(&current_facts),
        observation: if observe {
            Some(InformationObservation {
                through: at(19, 0),
                market,
                company_facts: current_facts,
                as_of_trading_days: [(StockId(0), 7), (StockId(1), 12)].into(),
            })
        } else {
            None
        },
    }
}
pub fn announcement(
    payload: &mut InformationPreparePayload,
    company: u32,
) -> crate::information::PublicationId {
    Arc::make_mut(&mut payload.library)
        .publish_announcement(crate::information::AnnouncementRequest {
            company: CompanyId(company),
            occurred_on: at(18, 0).date(),
            published_at: at(18, 0),
            content: crate::information::AnnouncementContent::Shock(
                crate::information::AnnouncedEvent {
                    kind: crate::company::ShockKind::ContractWon,
                    amplitude_bp: 100,
                    starts_on: at(18, 0).date(),
                    expires_on: at(18, 0).date(),
                },
            ),
        })
        .unwrap()
}

pub fn reports(
    payload: &mut InformationPreparePayload,
    company: u32,
    monthly: bool,
) -> (
    crate::information::PublicationId,
    Option<crate::information::PublicationId>,
) {
    reports_at(payload, company, monthly, 10 * 3600 + 30 * 60)
}

pub fn reports_at(
    payload: &mut InformationPreparePayload,
    company: u32,
    monthly: bool,
    monthly_second: u32,
) -> (
    crate::information::PublicationId,
    Option<crate::information::PublicationId>,
) {
    use crate::accounting::consolidation::{MemberId, ScopeId};
    use crate::accounting::reports::{
        generate_report_set, IndustryPresentation, ReportKind, ReportRequest, ReportSource,
        ReportVersion, VersionKind,
    };
    use crate::information::*;
    let member = MemberId(company);
    let books =
        crate::accounting::Books::new(crate::company::industrial::industrial_account_chart());
    let mut closing = crate::accounting::closing::ClosingEngine::new();
    let mut publish = |period: crate::accounting::AccountingPeriod,
                       kind: ReportKind,
                       origin: PublicationOrigin,
                       published_at: CivilInstant| {
        let report = generate_report_set(ReportRequest {
            period,
            kind,
            source: ReportSource::Standalone {
                id: member.clone(),
                books: &books,
                industry: IndustryPresentation::Industrial,
            },
            version: ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original,
            },
            adjustments: &BTreeMap::new(),
        })
        .unwrap();
        closing.record(report).unwrap();
        Arc::make_mut(&mut payload.library)
            .publish_closed(
                &closing,
                PublicationRequest {
                    company: CompanyId(company),
                    scope: ScopeId::Standalone(member.clone()),
                    period,
                    kind,
                    sequence: 1,
                    policy: AccountingPolicyRef { chart_version: 2 },
                    approved_at: CivilInstant::from_hms(published_at.date(), 8, 0, 0).unwrap(),
                    published_at,
                    origin,
                    supersedes: None,
                },
            )
            .unwrap()
    };
    let annual = publish(
        crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap(),
        ReportKind::Annual,
        PublicationOrigin::ScheduledDisclosure {
            fiscal_year: 2030,
            kind: ScheduledReportKind::Annual,
            offset_days: 0,
        },
        CivilInstant::from_hms(CivilDate::from_iso("2031-03-20").unwrap(), 18, 0, 0).unwrap(),
    );
    let monthly = monthly.then(|| {
        publish(
            crate::accounting::AccountingPeriod::from_ymd(2031, 3).unwrap(),
            ReportKind::Monthly,
            PublicationOrigin::MonthlyDisclosure {
                schedule: MonthlyReportSchedule::Custom {
                    day: 1,
                    second_of_day: monthly_second,
                    delay: MonthlyReportDelay::None,
                },
                delay_days: 0,
                seeded: false,
            },
            CivilInstant::new(at(10, 30).date(), monthly_second).unwrap(),
        )
    });
    (annual, monthly)
}
