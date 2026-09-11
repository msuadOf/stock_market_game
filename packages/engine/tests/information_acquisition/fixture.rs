//! 共享夹具：单公司披露场景（2030 全年账 → 年结 v1 → 按排期公开 + 一条
//! 临时公告），以及「未披露事实变更」与「更正 v2 事后公布」两个推进助手
//! （books_fixture/fixture 同 publications 套件形态）。
//!
//! 时间线（对一切偏移 0..=7 确定成立）：
//! 年报 v1 公布 2031-03-20..27 18:00 < 公告 2031-04-01 18:00 <
//! 更正 v2 公布 2031-04-20..27 18:00。

use engine::accounting::closing::{ClosingEngine, CorrectionRequest};
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
    scheduled_instant, stable_company_offset, AccountingPolicyRef, AnnouncedEvent,
    AnnouncementRequest, PublicLibrary, PublicationId, PublicationOrigin, PublicationRequest,
    ScheduledReportKind,
};
use engine::orderbook::AccountId;
use std::collections::BTreeMap;

/// 披露域种子（测试固定值，确定性钉死）。
pub(crate) const OPS_SEED: u64 = 11;

/// 场景公司 id。
pub(crate) const COMPANY: &str = "C-INFO";

/// NPC 甲（观察窗口内阅读公开信息）。
pub(crate) fn npc_a() -> AccountId {
    AccountId(11)
}

/// NPC 乙（观察窗口内不阅读任何内容）。
pub(crate) fn npc_b() -> AccountId {
    AccountId(22)
}

pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 元 → AccountingAmount（分）。
fn yuan(v: i128) -> AccountingAmount {
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

/// 现款收入（Dr 1002 / Cr 6001，元）——夹具高频形态的紧凑构造。
fn cash_rev(source: u64, date: &str, amount: i128) -> JournalEntry {
    entry(
        source,
        date,
        BusinessKind::CashRevenue,
        CashFlowClass::Operating,
        &[
            ("1002", PostingSide::Debit, amount),
            ("6001", PostingSide::Credit, amount),
        ],
    )
}

/// 双行分录（借贷各一行，元）——夹具高频形态的紧凑构造。
fn two_line(
    source: u64,
    date: &str,
    kind: BusinessKind,
    cash_flow: CashFlowClass,
    debit: (&str, i128),
    credit: (&str, i128),
) -> JournalEntry {
    entry(
        source,
        date,
        kind,
        cash_flow,
        &[
            (debit.0, PostingSide::Debit, debit.1),
            (credit.0, PostingSide::Credit, credit.1),
        ],
    )
}

/// 可见行情夹具：`NpcObservationContext` 的行情输入面是泛型参数，生产侧由
/// session 装配真实可见行情快照（engine 信息域不依赖 session）；本夹具只
/// 证明任意快照类型即可实例化上下文。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FixtureMarket {
    pub last_close_cents: BTreeMap<String, i64>,
}

impl FixtureMarket {
    pub(crate) fn quiet() -> Self {
        Self {
            last_close_cents: BTreeMap::from([(COMPANY.to_string(), 1_000)]),
        }
    }
}

/// 完整披露场景：账套 + 结账登记簿（年报 v1 序列 1）+ 公开库（v1 已公开、
/// 一条公告已公开）。
pub(crate) struct Scenario {
    pub company: CompanyId,
    pub member: MemberId,
    pub books: Books,
    pub closing: ClosingEngine,
    pub library: PublicLibrary,
    /// 年报 v1 公布时点（18:00 相位）。
    pub annual_instant: CivilInstant,
    /// 更正 v2 公布时点（2031 Q1 排期相位，晚于一切已登记获知时点）。
    pub correction_instant: CivilInstant,
    /// 年报 v1 公布 id。
    pub annual_v1_id: PublicationId,
    /// 公告公布 id。
    pub announcement_id: PublicationId,
    /// 公告公布时点（2031-04-01 18:00）。
    pub announcement_instant: CivilInstant,
}

/// 组装场景（确定性：同 seed 同公司 ⇒ 同偏移 ⇒ 同公布时点/id 序列）。
pub(crate) fn scenario() -> Scenario {
    let company = CompanyId(COMPANY.to_string());
    let member = MemberId(COMPANY.to_string());
    let offset = stable_company_offset(OPS_SEED, &company);
    let annual_instant = scheduled_instant(ScheduledReportKind::Annual, 2030, offset)
        .expect("annual 2030 schedule legal");
    let correction_instant =
        scheduled_instant(ScheduledReportKind::Q1, 2031, offset).expect("q1 2031 schedule legal");

    // 2029 开局 + 2030 全年经营流（publications/books_fixture 同款已证可年结）。
    let mut books = Books::new(industrial_chart_v2());
    books
        .post_batch(vec![
            entry(
                1,
                "2029-12-31",
                BusinessKind::OpeningBalance,
                CashFlowClass::Financing,
                &[
                    ("1002", PostingSide::Debit, 90_000),
                    ("1601", PostingSide::Debit, 10_000),
                    ("4001", PostingSide::Credit, 100_000),
                ],
            ),
            cash_rev(2, "2030-01-15", 2_000),
            two_line(
                3,
                "2030-02-10",
                BusinessKind::CreditSale,
                CashFlowClass::NonCash,
                ("1122", 1_500),
                ("6001", 1_500),
            ),
            two_line(
                4,
                "2030-02-20",
                BusinessKind::ReceivableCollection,
                CashFlowClass::Operating,
                ("1002", 1_000),
                ("1122", 1_000),
            ),
            two_line(
                5,
                "2030-03-15",
                BusinessKind::Depreciation,
                CashFlowClass::NonCash,
                ("6602", 120),
                ("1602", 120),
            ),
            cash_rev(6, "2030-11-05", 3_000),
        ])
        .expect("fixture entries must post");

    let mut closing = ClosingEngine::new();
    let (_monthly, annual_v1) = closing
        .close_year(&mut books, &member, IndustryPresentation::Industrial, 2030)
        .expect("year close");
    assert_eq!(annual_v1.sequence, 1);

    let approval = CivilInstant::from_hms(annual_instant.date(), 8, 0, 0).expect("approval");
    let mut library = PublicLibrary::new();
    let annual_v1_id = library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: company.clone(),
                scope: ScopeId::Standalone(member.clone()),
                period: AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
                kind: ReportKind::Annual,
                sequence: 1,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: approval,
                published_at: annual_instant,
                origin: PublicationOrigin::ScheduledDisclosure {
                    fiscal_year: 2030,
                    kind: ScheduledReportKind::Annual,
                    offset_days: offset,
                },
                supersedes: None,
            },
        )
        .expect("annual v1 publishes");

    // 临时公告：发生日 = 公布日（发生后的下一个 18:00 相位），条款只含已确认事实。
    let announcement_date = d("2031-04-01");
    let announcement_instant =
        CivilInstant::from_hms(announcement_date, 18, 0, 0).expect("announcement phase");
    let announcement_id = library
        .publish_announcement(AnnouncementRequest {
            company: company.clone(),
            occurred_on: announcement_date,
            published_at: announcement_instant,
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
        member,
        books,
        closing,
        library,
        annual_instant,
        correction_instant,
        annual_v1_id,
        announcement_id,
        announcement_instant,
    }
}

/// 未披露事实变更：向开放期间（2031-02）过账新分录——总账事实改变，但
/// 不结账、不公布：公开库与一切个人状态都不受影响。
pub(crate) fn post_undisclosed_fact(scenario: &mut Scenario) {
    scenario
        .books
        .post_batch(vec![cash_rev(90, "2031-02-10", 800)])
        .expect("undisclosed entry posts");
}

/// 公开更正 v2：先经结账登记簿重述（调整分录过账于开放期间 2031-01，
/// 生成年报 v2 supersedes v1），再在更正后的下一个排期相位公开。
pub(crate) fn publish_correction(scenario: &mut Scenario) -> PublicationId {
    let corrected = scenario
        .closing
        .correct(
            &mut scenario.books,
            &scenario.member,
            IndustryPresentation::Industrial,
            (
                AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
                ReportKind::Annual,
            ),
            CorrectionRequest {
                entries: vec![cash_rev(99, "2031-01-15", 300)],
                reason: "遗漏现金收入更正".to_string(),
            },
        )
        .expect("correction registers v2");
    assert_eq!(corrected.sequence, 2);

    let approval =
        CivilInstant::from_hms(scenario.correction_instant.date(), 8, 0, 0).expect("approval");
    scenario
        .library
        .publish_closed(
            &scenario.closing,
            PublicationRequest {
                company: scenario.company.clone(),
                scope: ScopeId::Standalone(scenario.member.clone()),
                period: AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
                kind: ReportKind::Annual,
                sequence: 2,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: approval,
                published_at: scenario.correction_instant,
                origin: PublicationOrigin::Correction,
                supersedes: Some(scenario.annual_v1_id),
            },
        )
        .expect("correction publishes as v2")
}

/// 18:00 相位后一小时（合法获知时点：≥ published_at 且同日）。
pub(crate) fn hour_after(instant: CivilInstant) -> CivilInstant {
    CivilInstant::new(instant.date(), instant.second_of_day() + 3600).expect("19:00 within day")
}

/// 18:00 相位前一分钟（非法获知时点：observed_at < published_at）。
pub(crate) fn minute_before(instant: CivilInstant) -> CivilInstant {
    CivilInstant::new(instant.date(), instant.second_of_day() - 60).expect("17:59 within day")
}
