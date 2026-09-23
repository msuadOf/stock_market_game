//! 地产科目表 v5（版本化数据表）：开发存货/减值准备/合同负债/长短期借款/
//! 应收尾款/收入成本/财务费用/减值损失。科目代码的单一真源在
//! [`crate::accounting::reports::real_estate::codes`]（列报映射与过账共享
//! 同一份代码表，杜绝两层漂移）；不改动通用 v1 语义（任务 6 语义冻结区），
//! 恢复优先用存档内科目表。

use crate::accounting::reports::real_estate::codes;
use crate::accounting::{AccountChart, AccountDef, LedgerAccountId};

/// 科目代码常量快捷面（等值 re-export；处理器过账引用）。
pub(crate) mod acct {
    pub use crate::accounting::reports::real_estate::codes::*;
}

/// 地产科目表 v5（版本 5；全部科目均由地产处理器/列报使用）。
pub fn real_estate_chart_v5() -> AccountChart {
    use crate::accounting::AccountElement::*;
    let acc = |code: &str, def: AccountDef| (LedgerAccountId(code.to_string()), def);
    let accounts = vec![
        acc(codes::CASH, AccountDef::new("银行存款", Asset).with_cash()),
        acc(codes::AR, AccountDef::new("应收账款（交付尾款）", Asset)),
        acc(codes::DEV_INVENTORY, AccountDef::new("开发存货", Asset)),
        acc(
            codes::DEV_IMPAIR_ALLOW,
            AccountDef::new("开发存货减值准备", Asset).with_contra(),
        ),
        acc(codes::ST_DEBT, AccountDef::new("短期借款", Liability)),
        acc(
            codes::CONTRACT_LIAB,
            AccountDef::new("合同负债（预售款）", Liability),
        ),
        acc(codes::INT_PAYABLE, AccountDef::new("应付利息", Liability)),
        acc(
            codes::LT_DEBT,
            AccountDef::new("长期借款（项目借款）", Liability),
        ),
        acc(codes::CAPITAL, AccountDef::new("实收资本", Equity)),
        acc(codes::PROFIT_CURRENT, AccountDef::new("本年利润", Equity)),
        acc(codes::REVENUE, AccountDef::new("主营业务收入", Revenue)),
        acc(codes::COGS, AccountDef::new("主营业务成本", Expense)),
        acc(codes::FIN_EXP, AccountDef::new("财务费用", Expense)),
        acc(codes::IMPAIR_LOSS, AccountDef::new("资产减值损失", Expense)),
    ];
    AccountChart::new(5, accounts).expect("real estate chart v5 is well-formed")
}
