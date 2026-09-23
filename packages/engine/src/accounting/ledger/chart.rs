//! 版本化科目表（K2/K3）：通用科目 + 五要素分类 + 现金类/备抵标志。
//!
//! 行业科目表（银行/保险/地产专属科目）在任务 8–11 以**新版本或扩展表**落
//! 地，不改动本表语义。科目编号沿用企业会计准则通用科目体系（四位数字串，
//! 支持子科目扩展），与交易域 `AccountId` 是完全独立的命名空间。

use std::collections::BTreeMap;

use crate::accounting::error::AccountingError;
use crate::accounting::journal::PostingSide;

/// 总账科目 newtype（与交易域 `AccountId` 独立的命名空间）。
#[derive(
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
#[serde(transparent)]
#[ts(type = "string")]
pub struct LedgerAccountId(pub String);

impl std::fmt::Display for LedgerAccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 会计五要素（基本准则；「利润」并入权益的滚动口径，结账在任务 13）。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum AccountElement {
    Asset,
    Liability,
    Equity,
    Revenue,
    Expense,
}

impl AccountElement {
    /// 该要素的默认正常方向（备抵科目由 `AccountDef::is_contra` 翻转）。
    pub fn normal_side(self) -> PostingSide {
        match self {
            AccountElement::Asset | AccountElement::Expense => PostingSide::Debit,
            AccountElement::Liability | AccountElement::Equity | AccountElement::Revenue => {
                PostingSide::Credit
            }
        }
    }
}

/// 科目定义。`is_cash` 参与负现金守卫与现金流索引；`is_contra` 翻转正常方向
/// （如累计折旧是资产备抵、贷方余额）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct AccountDef {
    pub name: String,
    pub element: AccountElement,
    pub is_cash: bool,
    pub is_contra: bool,
}

impl AccountDef {
    /// 常规科目（非现金、非备抵）。
    pub fn new(name: &str, element: AccountElement) -> Self {
        Self {
            name: name.to_string(),
            element,
            is_cash: false,
            is_contra: false,
        }
    }

    /// 现金科目（库存现金/银行存款：负现金守卫与现金流索引依据）。
    pub fn with_cash(mut self) -> Self {
        self.is_cash = true;
        self
    }

    /// 备抵科目（正常方向与要素默认相反，如累计折旧）。
    pub fn with_contra(mut self) -> Self {
        self.is_contra = true;
        self
    }
}

/// 版本化科目表：随存档保存；恢复优先用存档表，不用最新默认表覆盖。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct AccountChart {
    version: u32,
    accounts: BTreeMap<LedgerAccountId, AccountDef>,
}

impl AccountChart {
    /// 由显式清单构造：重复科目 / 空代码 / 空名称 → `ChartInvalid`。
    pub fn new(
        version: u32,
        accounts: Vec<(LedgerAccountId, AccountDef)>,
    ) -> Result<Self, AccountingError> {
        let mut map = BTreeMap::new();
        for (id, def) in accounts {
            if id.0.trim().is_empty() {
                return Err(AccountingError::ChartInvalid {
                    detail: "empty account code".to_string(),
                });
            }
            if def.name.trim().is_empty() {
                return Err(AccountingError::ChartInvalid {
                    detail: format!("empty name for account {id}"),
                });
            }
            if map.contains_key(&id) {
                return Err(AccountingError::ChartInvalid {
                    detail: format!("duplicate account {id}"),
                });
            }
            map.insert(id, def);
        }
        Ok(Self {
            version,
            accounts: map,
        })
    }

    /// 通用 v1 科目表（企业会计准则通用科目编号；行业表在任务 8–11 扩充）。
    pub fn generic_v1() -> Self {
        use AccountElement::*;
        let acc = |code: &str, def: AccountDef| (LedgerAccountId(code.to_string()), def);
        let accounts = vec![
            acc("1001", AccountDef::new("库存现金", Asset).with_cash()),
            acc("1002", AccountDef::new("银行存款", Asset).with_cash()),
            acc("1122", AccountDef::new("应收账款", Asset)),
            acc("1601", AccountDef::new("固定资产", Asset)),
            acc("1602", AccountDef::new("累计折旧", Asset).with_contra()),
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
        ];
        Self::new(1, accounts).expect("generic v1 chart is well-formed")
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn get(&self, id: &LedgerAccountId) -> Option<&AccountDef> {
        self.accounts.get(id)
    }

    pub fn contains(&self, id: &LedgerAccountId) -> bool {
        self.accounts.contains_key(id)
    }

    /// 是否现金科目（负现金守卫与现金流索引依据）。
    pub fn is_cash(&self, id: &LedgerAccountId) -> bool {
        self.accounts.get(id).is_some_and(|def| def.is_cash)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&LedgerAccountId, &AccountDef)> {
        self.accounts.iter()
    }
}
