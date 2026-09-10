//! 任务 12（company-information-npc-intentions）：固定集团合并与抵销集成测试。
//!
//! K3 合并语义：固定母子公司（开局后不变，无并购/股权交易）、控制范围与
//! 持股比例（母公司持股 > 5000bp 方为控制）、少数股东损益/权益、内部往来
//! 与内部销售未实现利润抵销。抵销分录只存在于**合并工作底稿**——绝不回记
//! 任何成员账套（集团现金 == Σ 成员现金，逐分相等）。无子公司返回类型化
//! `NotApplicable`，不伪装空合并。
//!
//! 金样单位约定：注释写「元」便于人读，执行值一律「分」。金样全部手工
//! 可复核（80/20 拆分、上游/下游未实现利润 200,000 元等，见各测试注释）。
//!
//! QA 入口：`cargo test -p engine --test consolidation`（happy 与 failure
//! 同命令覆盖）。

mod failures;
mod gold;
mod intercompany_gold;

use engine::accounting::consolidation::{
    ConsolidationRequest, GroupMember, IntercompanyBalance, IntercompanySale, MemberId, MemberSpec,
};
use engine::accounting::{
    AccountChart, AccountingAmount, Books, BusinessEventId, BusinessKind, CashFlowClass,
    JournalEntry, JournalLine, LedgerAccountId, PostingSide,
};
use engine::calendar::CivilDate;

/// 测试用 ISO 日期；输入本身必须合法（否则测试夹具写错）。
pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 元 → AccountingAmount（分）。仅测试夹具用；执行值恒为分。
pub(crate) fn yuan(yuan_value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(yuan_value.checked_mul(100).expect("fixture yuan overflow"))
}

/// 快速构造分录（金额参数单位 = 元）。
pub(crate) fn entry(
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

/// 由科目表 + 分录清单构造账套（夹具分录必须全部可过账）。
pub(crate) fn books_with(chart: AccountChart, entries: Vec<JournalEntry>) -> Books {
    let mut books = Books::new(chart);
    books
        .post_batch(entries)
        .expect("fixture entries must post");
    books
}

/// 成员规格 + 账套引用。
pub(crate) fn member<'a>(
    id: &str,
    parent: Option<&str>,
    issued_shares: u64,
    parent_held_shares: u64,
    books: &'a Books,
) -> GroupMember<'a> {
    GroupMember {
        spec: MemberSpec {
            id: MemberId(id.to_string()),
            group_parent: parent.map(|p| MemberId(p.to_string())),
            issued_shares,
            parent_held_shares,
        },
        books,
    }
}

/// 无内部交易的合并请求。
pub(crate) fn request<'a>(root: &str, members: Vec<GroupMember<'a>>) -> ConsolidationRequest<'a> {
    ConsolidationRequest {
        root: MemberId(root.to_string()),
        members,
        intercompany_balances: Vec::new(),
        intercompany_sales: Vec::new(),
    }
}

/// 内部往来余额申报（金额单位 = 元）。
pub(crate) fn ic_balance(
    holder: &str,
    counterparty: &str,
    account: &str,
    amount_yuan: i128,
) -> IntercompanyBalance {
    IntercompanyBalance {
        member: MemberId(holder.to_string()),
        counterparty: MemberId(counterparty.to_string()),
        account: LedgerAccountId(account.to_string()),
        amount: yuan(amount_yuan),
    }
}

/// 内部商品交易申报（工业 v2 科目：6001 收入 / 6401 成本 / 1405 库存商品；
/// 金额单位 = 元）。
pub(crate) fn ic_sale(
    seller: &str,
    buyer: &str,
    invoice: i128,
    cost: i128,
    unsold: i128,
) -> IntercompanySale {
    IntercompanySale {
        seller: MemberId(seller.to_string()),
        buyer: MemberId(buyer.to_string()),
        revenue_account: LedgerAccountId("6001".to_string()),
        cost_account: LedgerAccountId("6401".to_string()),
        inventory_account: LedgerAccountId("1405".to_string()),
        invoice_amount: yuan(invoice),
        cost_amount: yuan(cost),
        unsold_inventory: yuan(unsold),
    }
}

/// 科目代码（工业 v2 / 银行 v3 夹具引用）。
pub(crate) mod acct {
    pub const BANK: &str = "1002"; // 银行存款（现金类）
    pub const CENTRAL_BANK: &str = "1003"; // 存放中央银行款项（银行现金类）
    pub const AR: &str = "1122"; // 应收账款
    pub const INVENTORY: &str = "1405"; // 库存商品
    pub const FIXED_ASSETS: &str = "1601"; // 固定资产
    pub const PAYABLE: &str = "2202"; // 应付账款
    pub const CAPITAL: &str = "4001"; // 实收资本
    pub const REVENUE: &str = "6001"; // 主营业务收入
    pub const COGS: &str = "6401"; // 主营业务成本
    pub const ADMIN_EXP: &str = "6602"; // 管理费用
    pub const FEE_INCOME: &str = "6021"; // 手续费及佣金收入（银行）
    pub const INTEREST_EXP: &str = "6411"; // 利息支出（银行）
}
