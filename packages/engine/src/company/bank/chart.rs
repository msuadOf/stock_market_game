//! 银行科目表 v3（版本化数据表）：银行经营科目（存放中央银行款项/贷款本金/
//! 应收利息/贷款减值准备/短期与长期吸收存款/利息收支/手续费及佣金/信用减值
//! 损失）。科目代码的单一真源在 [`crate::accounting::reports::bank::codes`]
//! （列报映射与过账共享同一份代码表，杜绝两层漂移）；不改动通用 v1 语义
//! （任务 6 语义冻结区），恢复优先用存档内科目表。

use crate::accounting::reports::bank::codes;
use crate::accounting::{AccountChart, AccountDef, LedgerAccountId};

/// 科目代码常量快捷面（等值 re-export；处理器过账引用）。
pub(crate) mod acct {
    pub use crate::accounting::reports::bank::codes::*;
}

/// 银行科目表 v3（版本 3；全部科目均由银行处理器/列报使用）。
pub fn bank_chart_v3() -> AccountChart {
    use crate::accounting::AccountElement::*;
    let acc = |code: &str, def: AccountDef| (LedgerAccountId(code.to_string()), def);
    let accounts = vec![
        acc(
            codes::CASH,
            AccountDef::new("存放中央银行款项", Asset).with_cash(),
        ),
        acc(
            codes::LOAN_INT_RCV,
            AccountDef::new("应收利息（贷款）", Asset),
        ),
        acc(codes::LOAN_PRINCIPAL, AccountDef::new("贷款——本金", Asset)),
        acc(
            codes::LOAN_ALLOWANCE,
            AccountDef::new("贷款减值准备", Asset).with_contra(),
        ),
        acc(
            codes::ST_DEPOSIT,
            AccountDef::new("吸收存款——短期", Liability),
        ),
        acc(
            codes::LT_DEPOSIT,
            AccountDef::new("吸收存款——长期", Liability),
        ),
        acc(
            codes::DEP_INT_PAYABLE,
            AccountDef::new("应付利息（存款）", Liability),
        ),
        acc(codes::CAPITAL, AccountDef::new("实收资本", Equity)),
        acc(codes::PROFIT_CURRENT, AccountDef::new("本年利润", Equity)),
        acc(codes::INTEREST_INCOME, AccountDef::new("利息收入", Revenue)),
        acc(
            codes::FEE_INCOME,
            AccountDef::new("手续费及佣金收入", Revenue),
        ),
        acc(
            codes::INTEREST_EXPENSE,
            AccountDef::new("利息支出", Expense),
        ),
        acc(
            codes::CREDIT_IMPAIR,
            AccountDef::new("信用减值损失", Expense),
        ),
    ];
    AccountChart::new(3, accounts).expect("bank chart v3 is well-formed")
}
