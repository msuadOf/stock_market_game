//! 任务 18 测试夹具（fundamental_beliefs）：四年度工业账套 → 逐年结账 →
//! 按排程公开年报（FY2027..FY2030）+ 一条临时公告；顺序 f64 RNG；手工
//! ReportSet 构造器（核心抽取层的直测面——不经结账机器）。
//!
//! 年度数字（元，全部现款、无税）：收入 100,000 / 80,000 / 180,000 / 198,000，
//! 管理费用恒为收入 10% ⇒ 归母净利 90,000 / 72,000 / 162,000 / 178,200。
//! FY2030 相对 FY2029 收入与净利均 **+10%**（same-news 金样的「新闻」）。
//! 开局 2026-12-31：现金 90,000 + 固定资产 10,000 / 实收资本 100,000。

mod centers;
mod failures;
mod gold;
mod hand;
mod per_share;
mod priors;

use engine::accounting::closing::ClosingEngine;
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::{IndustryPresentation, ReportKind};
use engine::accounting::{
    AccountingAmount, AccountingPeriod, Books, BusinessEventId, BusinessKind, CashFlowClass,
    JournalEntry, JournalLine, LedgerAccountId, PostingSide,
};
use engine::calendar::{CivilDate, CivilInstant};
use engine::company::industrial::industrial_chart_v2;
use engine::company::{CompanyId, ShockKind};
use engine::information::{
    AccountingPolicyRef, AnnouncedEvent, AnnouncementRequest, PublicLibrary, PublicationId,
    PublicationOrigin, PublicationRequest, ScheduledReportKind, scheduled_instant,
    stable_company_offset,
};
use engine::strategy::Rng;

/// 披露域种子（测试固定值，确定性钉死）。
pub(crate) const OPS_SEED: u64 = 21;
/// 场景公司 id。
pub(crate) const COMPANY: &str = "C-FUND";
/// 发行人固定的已发行普通股总股数（绝不使用流通股数）。
pub(crate) const ISSUED_SHARES: u64 = 100_000;

pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 元 → AccountingAmount（分）。
pub(crate) fn yuan(v: i128) -> AccountingAmount {
    AccountingAmount::from_cents(v.checked_mul(100).expect("fixture yuan overflow"))
}

fn entry(
    source: u64,
    date: &str,
    kind: BusinessKind,
    cash_flow: CashFlowClass,
    lines: &[(&str, PostingSide, i128)],
) -> JournalEntry {
    JournalEntry {
        source: BusinessEventId::new(source),
        date: d(date),
        kind,
        cash_flow,
        lines: lines
            .iter()
            .map(|(account, side, amount)| JournalLine {
                account: LedgerAccountId((*account).to_string()),
                side: *side,
                amount: yuan(*amount),
            })
            .collect(),
    }
}

/// 可见行情夹具（belief 层不读取行情——字段仅供「不被消费」的对照断言）。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BeliefMarket {
    pub last_close_cents: i64,
    pub float_shares: u64,
}

pub(crate) fn market(float_shares: u64) -> BeliefMarket {
    BeliefMarket {
        last_close_cents: 1_000,
        float_shares,
    }
}

/// 完整披露场景：四年度账套 + 逐年结账公开 + 一条临时公告。
pub(crate) struct Scenario {
    pub company: CompanyId,
    pub library: PublicLibrary,
    /// FY2027..FY2030 年报公布 id（下标 0..3 对应会计年 2027..2030）。
    pub annual_ids: [PublicationId; 4],
    /// 各年报公布时点（18:00 相位）。
    pub annual_instants: [CivilInstant; 4],
    /// 临时公告公布 id（2031-04-01 18:00，ContractWon）。
    pub announcement_id: PublicationId,
}

pub(crate) fn scenario() -> Scenario {
    let company = CompanyId(COMPANY.to_string());
    let member = MemberId(COMPANY.to_string());
    let offset = stable_company_offset(OPS_SEED, &company);

    let mut books = Books::new(industrial_chart_v2());
    let revenues = [100_000i128, 80_000, 180_000, 198_000];
    let mut batch = vec![entry(
        1,
        "2026-12-31",
        BusinessKind::OpeningBalance,
        CashFlowClass::Financing,
        &[
            ("1002", PostingSide::Debit, 90_000),
            ("1601", PostingSide::Debit, 10_000),
            ("4001", PostingSide::Credit, 100_000),
        ],
    )];
    for (index, revenue) in revenues.iter().enumerate() {
        let year = 2027 + index;
        let expense = revenue / 10;
        batch.push(entry(
            2 + (index as u64) * 2,
            &format!("{year}-06-15"),
            BusinessKind::CashRevenue,
            CashFlowClass::Operating,
            &[
                ("1002", PostingSide::Debit, *revenue),
                ("6001", PostingSide::Credit, *revenue),
            ],
        ));
        batch.push(entry(
            3 + (index as u64) * 2,
            &format!("{year}-09-10"),
            BusinessKind::CashExpense,
            CashFlowClass::Operating,
            &[
                ("6602", PostingSide::Debit, expense),
                ("1002", PostingSide::Credit, expense),
            ],
        ));
    }
    books.post_batch(batch).expect("fixture entries must post");

    let mut closing = ClosingEngine::new();
    let mut library = PublicLibrary::new();
    let mut annual_ids = [PublicationId::new(0); 4];
    let mut annual_instants =
        [CivilInstant::from_hms(d("2028-03-20"), 18, 0, 0).expect("phase"); 4];
    for (index, &year) in [2027i32, 2028, 2029, 2030].iter().enumerate() {
        closing
            .close_year(&mut books, &member, IndustryPresentation::Industrial, year)
            .expect("year close");
        let instant = scheduled_instant(ScheduledReportKind::Annual, year, offset)
            .expect("annual schedule legal");
        let approval = CivilInstant::from_hms(instant.date(), 8, 0, 0).expect("approval");
        let id = library
            .publish_closed(
                &closing,
                PublicationRequest {
                    company: company.clone(),
                    scope: ScopeId::Standalone(member.clone()),
                    period: AccountingPeriod::from_ymd(year, 12).expect("annual period"),
                    kind: ReportKind::Annual,
                    sequence: 1,
                    policy: AccountingPolicyRef { chart_version: 2 },
                    approved_at: approval,
                    published_at: instant,
                    origin: PublicationOrigin::ScheduledDisclosure {
                        fiscal_year: year,
                        kind: ScheduledReportKind::Annual,
                        offset_days: offset,
                    },
                    supersedes: None,
                },
            )
            .expect("annual publishes");
        annual_ids[index] = id;
        annual_instants[index] = instant;
    }

    let announcement_date = d("2031-04-01");
    let announcement_id = library
        .publish_announcement(AnnouncementRequest {
            company: company.clone(),
            occurred_on: announcement_date,
            published_at: CivilInstant::from_hms(announcement_date, 18, 0, 0).expect("phase"),
            event: AnnouncedEvent {
                kind: ShockKind::ContractWon,
                amplitude_bp: 1_200,
                starts_on: announcement_date,
                expires_on: d("2031-04-10"),
            },
        })
        .expect("announcement publishes");

    Scenario {
        company,
        library,
        annual_ids,
        annual_instants,
        announcement_id,
    }
}

/// 18:00 相位后一小时（合法获知时点）。
pub(crate) fn hour_after(instant: CivilInstant) -> CivilInstant {
    CivilInstant::new(instant.date(), instant.second_of_day() + 3600).expect("19:00 within day")
}

/// 顺序 f64 序列：恰好消耗给定次数，多余抽取即 panic（锁定一次性抽样契约）。
pub(crate) struct SeqRng {
    values: Vec<f64>,
    next: usize,
}

impl SeqRng {
    pub(crate) fn new(values: Vec<f64>) -> Self {
        Self { values, next: 0 }
    }

    pub(crate) fn draws(&self) -> usize {
        self.next
    }
}

impl Rng for SeqRng {
    fn next_f64(&mut self) -> f64 {
        let value = *self
            .values
            .get(self.next)
            .expect("belief assumption draws are exactly six per profile lifetime");
        self.next += 1;
        value
    }

    fn next_range_u32(&mut self, lo: u32, _hi: u32) -> u32 {
        lo
    }
}

/// 个人假设抽样序列（canonical 序：增长偏差/质量系数/PE/权益成本/终值增长/
/// ROE 偏差）。首抽 f ∈ {1.0, 0.0} ⇒ 偏差 ±1000bp；其余落 quality 10000 /
/// pe 10（others 档）/ cost 1000 / terminal 0 / roe 0。
pub(crate) fn assumptions_rng(deviation_draw: f64) -> SeqRng {
    SeqRng::new(vec![deviation_draw, 0.5, 0.0, 0.25, 0.0, 0.5])
}
