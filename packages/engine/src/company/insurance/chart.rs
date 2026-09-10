//! 保险科目表 v4（版本化数据表）：保险经营科目（应收保费/未到期责任负债/
//! 已发生赔款负债/保险服务收入/保险服务费用/保险财务损益）。科目代码的
//! 单一真源在 [`crate::accounting::reports::insurance::codes`]（列报映射与
//! 过账共享同一份代码表，杜绝两层漂移）；不改动通用 v1 语义（任务 6 语义
//! 冻结区），恢复优先用存档内科目表。

use crate::accounting::reports::insurance::codes;
use crate::accounting::{AccountChart, AccountDef, LedgerAccountId};

/// 科目代码常量快捷面（等值 re-export；处理器过账引用）。
pub(crate) mod acct {
    pub use crate::accounting::reports::insurance::codes::*;
}

/// 保险科目表 v4（版本 4；全部科目均由保险处理器/列报使用）。
pub fn insurance_chart_v4() -> AccountChart {
    use crate::accounting::AccountElement::*;
    let acc = |code: &str, def: AccountDef| (LedgerAccountId(code.to_string()), def);
    let accounts = vec![
        acc(codes::CASH, AccountDef::new("银行存款", Asset).with_cash()),
        acc(
            codes::PREMIUM_RECEIVABLE,
            AccountDef::new("应收保费", Asset),
        ),
        acc(codes::LRC, AccountDef::new("未到期责任负债", Liability)),
        acc(codes::LIC, AccountDef::new("已发生赔款负债", Liability)),
        acc(codes::CAPITAL, AccountDef::new("实收资本", Equity)),
        acc(codes::PROFIT_CURRENT, AccountDef::new("本年利润", Equity)),
        acc(
            codes::INSURANCE_REVENUE,
            AccountDef::new("保险服务收入", Revenue),
        ),
        acc(
            codes::INSURANCE_EXPENSE,
            AccountDef::new("保险服务费用", Expense),
        ),
        acc(
            codes::INSURANCE_FINANCE,
            AccountDef::new("保险财务损益", Expense),
        ),
    ];
    AccountChart::new(4, accounts).expect("insurance chart v4 is well-formed")
}
