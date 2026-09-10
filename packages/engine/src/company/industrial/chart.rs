//! 工业科目表 v2（版本化数据表）：通用 v1 全集 + 工业经营科目（存货/在产品/
//! 增值税子科目/长短期借款/坏账与减值准备/递延所得税/费用细分）。不改动通用
//! v1 语义（任务 6 语义冻结区）；恢复优先用存档内科目表。

use crate::accounting::AccountElement::{Asset, Equity, Expense, Liability, Revenue};
use crate::accounting::{AccountChart, AccountDef};

/// 科目代码常量（处理器过账引用；测试快照见 tests/industrial_accounting/main.rs）。
pub(crate) mod acct {
    pub const BANK: &str = "1002";
    pub const AR: &str = "1122";
    pub const BAD_DEBT_ALLOW: &str = "1231";
    pub const FIXED_ASSET: &str = "1601";
    pub const ACC_DEP: &str = "1602";
    pub const ACC_IMPAIR: &str = "1603";
    pub const DTA: &str = "1811";
    pub const ST_DEBT: &str = "2001";
    pub const PAYABLE: &str = "2202";
    pub const VAT_OUT: &str = "222101";
    pub const VAT_IN: &str = "222102";
    pub const CIT_PAYABLE: &str = "222104";
    pub const INT_PAYABLE: &str = "2231";
    pub const LT_DEBT: &str = "2501";
    pub const WIP: &str = "5001";
    pub const REVENUE: &str = "6001";
    pub const COGS: &str = "6401";
    pub const SELLING_EXP: &str = "6601";
    pub const ADMIN_EXP: &str = "6602";
    pub const RND_EXP: &str = "660201";
    pub const FIN_EXP: &str = "6603";
    pub const IMPAIR_LOSS: &str = "6701";
    pub const TAX_EXP: &str = "6801";
}

/// 工业科目表 v2 = 通用 v1 + 工业扩充（版本 2，向前兼容 v1 科目编号）。
pub fn industrial_chart_v2() -> AccountChart {
    let acc =
        |code: &str, def: AccountDef| (crate::accounting::LedgerAccountId(code.to_string()), def);
    let accounts = vec![
        // —— 通用 v1 全集（语义不变）——
        acc("1001", AccountDef::new("库存现金", Asset).with_cash()),
        acc(acct::BANK, AccountDef::new("银行存款", Asset).with_cash()),
        acc(acct::AR, AccountDef::new("应收账款", Asset)),
        acc("1601", AccountDef::new("固定资产", Asset)),
        acc(
            acct::ACC_DEP,
            AccountDef::new("累计折旧", Asset).with_contra(),
        ),
        acc("2001", AccountDef::new("短期借款", Liability)),
        acc("2202", AccountDef::new("应付账款", Liability)),
        acc("2221", AccountDef::new("应交税费", Liability)),
        acc("2231", AccountDef::new("应付利息", Liability)),
        acc("4001", AccountDef::new("实收资本", Equity)),
        acc("4103", AccountDef::new("本年利润", Equity)),
        acc("6001", AccountDef::new("主营业务收入", Revenue)),
        acc("6401", AccountDef::new("主营业务成本", Expense)),
        acc("6602", AccountDef::new("管理费用", Expense)),
        acc("6603", AccountDef::new("财务费用", Expense)),
        acc("6801", AccountDef::new("所得税费用", Expense)),
        // —— 工业扩充 ——
        acc(
            acct::BAD_DEBT_ALLOW,
            AccountDef::new("坏账准备", Asset).with_contra(),
        ),
        acc("1403", AccountDef::new("原材料", Asset)),
        acc("1405", AccountDef::new("库存商品", Asset)),
        acc(
            acct::ACC_IMPAIR,
            AccountDef::new("固定资产减值准备", Asset).with_contra(),
        ),
        acc(acct::DTA, AccountDef::new("递延所得税资产", Asset)),
        // 增值税子科目（财会〔2016〕22号：销项/进项分开；进项正常为借方余额）。
        acc(
            acct::VAT_OUT,
            AccountDef::new("应交税费—应交增值税（销项税额）", Liability),
        ),
        acc(
            acct::VAT_IN,
            AccountDef::new("应交税费—应交增值税（进项税额）", Liability).with_contra(),
        ),
        acc(
            acct::CIT_PAYABLE,
            AccountDef::new("应交税费—应交所得税", Liability),
        ),
        acc(acct::LT_DEBT, AccountDef::new("长期借款", Liability)),
        // 递延所得税负债（2901）：简化模型只确认亏损 DTA（1811），科目预留
        // 完整性（任务 13 报表如需应税暂时性差异再启用）。
        acc("2901", AccountDef::new("递延所得税负债", Liability)),
        // 生产成本（在产品）按经济实质计入存货（资产负债表在产品属存货）。
        acc(acct::WIP, AccountDef::new("生产成本（在产品）", Asset)),
        acc(acct::SELLING_EXP, AccountDef::new("销售费用", Expense)),
        // 研发费用按财会〔2018〕15号列报口径单列（管理费用子科目位）。
        acc(acct::RND_EXP, AccountDef::new("管理费用—研发费用", Expense)),
        acc(acct::IMPAIR_LOSS, AccountDef::new("资产减值损失", Expense)),
    ];
    AccountChart::new(2, accounts).expect("industrial chart v2 is well-formed")
}
