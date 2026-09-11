//! 公告曝光进入发现权重的唯一通道：任务 16 公开面（`discovery_candidates`）。
//!
//! - 公告公布 ⇒ 所属公司股票进入曝光集合 ⇒ 发现权重 +2.0（发现机会提高，
//!   绝不自动获知——获知只能经 `record_acquisition`，K4「新曝光不等于已读」）。
//! - 公布时点之前 `as_of` 查询 ⇒ 候选面为空 ⇒ 权重不变（未披露事实惰性）。
//! - 一次公共公告提高的是**个体**发现概率：每个 NPC 用自己的注意力 RNG 流
//!   抽样 ⇒ 全体候选绝不同步（分布断言，不是单次结果断言）。
//!
//! 公共夹具（公开库 + 私有账套 + 未披露过账助手）供 `failures/discovery.rs`
//! 复用。

use std::collections::BTreeSet;

use engine::accounting::{
    AccountingAmount, Books, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry,
    JournalLine, LedgerAccountId, PostingSide,
};
use engine::calendar::{CivilDate, CivilInstant};
use engine::company::industrial::industrial_chart_v2;
use engine::company::{CompanyId, ShockKind};
use engine::experience::PersonalWatchlist;
use engine::information::{
    discovery_candidates, AnnouncedEvent, AnnouncementRequest, PublicLibrary,
};
use engine::session::NpcAttentionState;
use engine::strategy::MarketView;
use engine::StockCode;

use super::{attention, code, market_of, view};

/// 公司 id ↔ 股票代码映射由会话接线（任务 26）持有；测试用固定映射。
pub(crate) fn company_of(code: &StockCode) -> CompanyId {
    CompanyId(format!("C-{}", code.0))
}

pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 开局账套（私有经营事实的载体；与公开库无连接）。
pub(crate) fn private_books() -> Books {
    let mut books = Books::new(industrial_chart_v2());
    books
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(1),
            date: d("2030-01-02"),
            kind: BusinessKind::OpeningBalance,
            cash_flow: CashFlowClass::Financing,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".to_string()),
                    side: PostingSide::Debit,
                    amount: AccountingAmount::from_cents(900_000),
                },
                JournalLine {
                    account: LedgerAccountId("4001".to_string()),
                    side: PostingSide::Credit,
                    amount: AccountingAmount::from_cents(900_000),
                },
            ],
        }])
        .expect("opening entry posts");
    books
}

/// 未披露私有事实：向开放期间过账新收入分录——总账事实改变，但不结账、不公布。
pub(crate) fn post_undisclosed_fact(books: &mut Books, source: u64) {
    books
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(source),
            date: d("2030-06-10"),
            kind: BusinessKind::CashRevenue,
            cash_flow: CashFlowClass::Operating,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".to_string()),
                    side: PostingSide::Debit,
                    amount: AccountingAmount::from_cents(80_000),
                },
                JournalLine {
                    account: LedgerAccountId("6001".to_string()),
                    side: PostingSide::Credit,
                    amount: AccountingAmount::from_cents(80_000),
                },
            ],
        }])
        .expect("undisclosed entry posts");
}

/// 公开库：一条已公布的临时公告（发生日当天 18:00 相位）。
pub(crate) fn library_with_announcement() -> (PublicLibrary, CivilInstant) {
    let occurred = d("2030-06-11");
    let published = CivilInstant::from_hms(occurred, 18, 0, 0).expect("18:00 phase");
    let mut library = PublicLibrary::new();
    library
        .publish_announcement(AnnouncementRequest {
            company: company_of(&code("600101")),
            occurred_on: occurred,
            published_at: published,
            event: AnnouncedEvent {
                kind: ShockKind::ContractWon,
                amplitude_bp: 1_200,
                starts_on: occurred,
                expires_on: d("2030-06-20"),
            },
        })
        .expect("announcement publishes");
    (library, published)
}

/// 会话接线形态的曝光集合派生：公司 × 股票映射 + `discovery_candidates`。
pub(crate) fn exposed_codes(
    library: &PublicLibrary,
    codes: &[StockCode],
    as_of: CivilInstant,
) -> BTreeSet<StockCode> {
    codes
        .iter()
        .filter(|code| !discovery_candidates(library, &company_of(code), as_of).is_empty())
        .cloned()
        .collect()
}

pub(crate) fn quiet_market() -> MarketView {
    market_of(vec![
        (code("600101"), view(1_000, &[1_000, 1_002], 1.0)),
        (code("600102"), view(1_000, &[1_000, 1_002], 1.0)),
    ])
}

#[test]
fn announcement_exposure_enters_weights_only_through_the_public_surface() {
    let (library, published) = library_with_announcement();
    let market = quiet_market();
    let all_codes = vec![code("600101"), code("600102")];

    // 公布时点之前：候选面为空 ⇒ 权重全为基础值（未披露事实惰性）。
    let before = exposed_codes(
        &library,
        &all_codes,
        CivilInstant::from_hms(d("2030-06-11"), 17, 0, 0).expect("17:00"),
    );
    assert!(before.is_empty(), "公布前无公共曝光");
    let weights = NpcAttentionState::discovery_weights(&market, &before);
    assert_eq!(weights[&code("600101")], 1.0);

    // 公布后：公告所属公司股票进入曝光集合 ⇒ 发现权重 +2.0；其他股票不变。
    let after = exposed_codes(&library, &all_codes, published);
    let expected_exposed: BTreeSet<StockCode> = [code("600101")].into_iter().collect();
    assert_eq!(after, expected_exposed, "只有公告所属股票曝光");
    let weights = NpcAttentionState::discovery_weights(&market, &after);
    assert_eq!(weights[&code("600101")], 3.0, "公告曝光 +2.0");
    assert_eq!(weights[&code("600102")], 1.0, "未涉及股票权重不变");
}

#[test]
fn one_public_announcement_never_synchronizes_all_npc_candidates() {
    let (library, published) = library_with_announcement();
    let market = quiet_market();
    let all_codes = vec![code("600101"), code("600102")];
    let exposed = exposed_codes(&library, &all_codes, published);
    let watchlist = PersonalWatchlist::new();
    let held = BTreeSet::new();

    // 40 个 NPC，各自独立的注意力 RNG 流（会话接线形态：seed ^ id 派生），
    // 每人抽样 25 次。曝光股权重 3、平静股权重 1 ⇒ P(曝光) = 0.75。
    // 断言分布而非单次结果：绝不要求（也绝不允许）全体同选。
    let npcs = 40u64;
    let draws_per_npc = 25usize;
    let mut exposed_count = 0usize;
    let mut quiet_count = 0usize;
    let mut first_run: Vec<Option<StockCode>> = Vec::new();
    for npc in 1..=npcs {
        let mut state = attention(0x5EED_0000 ^ npc.wrapping_mul(0x6A09_E667_F3BC_C908));
        for _ in 0..draws_per_npc {
            let picked = state.sample_discovery_stock(&market, &held, &watchlist, &exposed);
            if npc == 1 {
                first_run.push(picked.clone());
            }
            match picked {
                Some(picked) if picked == code("600101") => exposed_count += 1,
                Some(_) => quiet_count += 1,
                None => panic!("non-empty market must always yield a candidate"),
            }
        }
    }
    let total = npcs as usize * draws_per_npc;
    assert_eq!(exposed_count + quiet_count, total);
    let sigma = (total as f64 * 0.75 * 0.25).sqrt();
    let expected = total as f64 * 0.75;
    assert!(
        (exposed_count as f64 - expected).abs() <= 5.0 * sigma,
        "曝光股发现率 {exposed_count}/{total} 偏离加权期望 {expected:.0} 超过 5σ"
    );
    assert!(
        exposed_count < total && quiet_count > 0,
        "一次公共公告不得让全体 NPC 候选同步（发现只是机会，不是命令）"
    );

    // 确定性：个体流可重放（同派生种子 ⇒ 同序列）。
    let mut replay = attention(0x5EED_0000 ^ 1u64.wrapping_mul(0x6A09_E667_F3BC_C908));
    let second_run: Vec<Option<StockCode>> = (0..draws_per_npc)
        .map(|_| replay.sample_discovery_stock(&market, &held, &watchlist, &exposed))
        .collect();
    assert_eq!(first_run, second_run, "同种子个体序列逐次一致");
}
