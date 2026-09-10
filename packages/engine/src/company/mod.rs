//! 公司域（K2/K3）：公司实体、开局账套、外部对手方与经营合同。
//!
//! 资金边界铁律（K2）：公司经营资金只存在于公司账套（[`Books`]，现金走科目
//! 1001/1002），外部商业对手方用独立 `CounterpartyId`；投资者交易 `Account`
//! 与公司账套互不复用。发行人映射要求股本精确匹配（`issued_shares` == 股票
//! `total_shares`），不依据初始股价反推任何资产负债价值。
//!
//! 范围边界（任务 7）：本模块提供公司实体 + 显式平衡开局账套 + 合同/授信
//! 数据面；行业经营逻辑（任务 8–11）、经营前史（任务 14）、报表（任务 13）、
//! 历史发布集（任务 15）与会话接线（任务 26）不在此实现。公司注册表在
//! 本任务中独立构建，不修改 `session`。

mod contracts;
mod counterparty;
mod defaults;
mod error;
mod opening;
mod spec;

pub use contracts::{
    ContractBook, ContractId, ContractRole, CreditLine, DayCountBasis, OperatingBudget,
    OperatingContract,
};
pub use counterparty::{
    CounterpartyFlow, CounterpartyId, CounterpartyKind, CounterpartyLedger, ExternalCounterparty,
    FlowDirection,
};
pub use defaults::default_companies;
pub use error::CompanyError;
pub use opening::{
    opening_event_id, AssetSubLedger, CompanyOpening, ContractSubLedger, InventorySubLedger,
    OpeningLine, SubsidiaryLedgers,
};
pub use spec::{CompanyId, CompanyKind, CompanySpec, IndustryId};

use std::collections::BTreeMap;

use crate::account::StockCode;
use crate::accounting::Books;

/// 公司装配输入：规格 + 开局账套 + 外部对手方 + 经营预算（含授信）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompanyConfig {
    pub spec: CompanySpec,
    pub opening: CompanyOpening,
    pub counterparties: Vec<ExternalCounterparty>,
    pub budget: OperatingBudget,
}

/// 公司实体：规格 + 权威账套 + 对手方/合同/预算 + 子账占位。
///
/// 现金在账套（accounting 域）而非交易账户；行业子账内容在任务 8–11 填充。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Company {
    spec: CompanySpec,
    books: Books,
    counterparties: CounterpartyLedger,
    contracts: ContractBook,
    budget: OperatingBudget,
    sub_ledgers: SubsidiaryLedgers,
}

impl Company {
    /// 由配置构造：过账开局凭证（经 [`Books::post_batch`] 验证路径——不平衡/
    /// 未知科目/负现金 ⇒ `CompanyError::OpeningPost`，无半构造公司）并登记
    /// 外部对手方。
    pub fn new(config: CompanyConfig) -> Result<Self, CompanyError> {
        let CompanyConfig {
            spec,
            opening,
            counterparties,
            budget,
        } = config;
        spec.validate()?;
        let company_id = spec.id.clone();
        let voucher = opening.to_voucher();
        let mut books = Books::new(opening.chart);
        books
            .post_batch(vec![voucher])
            .map_err(|source| CompanyError::OpeningPost {
                company: company_id,
                source,
            })?;
        let mut ledger = CounterpartyLedger::new();
        for counterparty in counterparties {
            ledger.register(counterparty)?;
        }
        Ok(Self {
            spec,
            books,
            counterparties: ledger,
            contracts: ContractBook::new(),
            budget,
            sub_ledgers: SubsidiaryLedgers::default(),
        })
    }

    /// 登记经营合同：数据验证 + 对手方已登记 + 借款授信占用检查
    /// （无授信/超授信 ⇒ 类型化拒绝；恰好用满授信合法）。
    pub fn register_contract(&mut self, contract: OperatingContract) -> Result<(), CompanyError> {
        contract.validate()?;
        if self.counterparties.get(&contract.counterparty).is_none() {
            return Err(CompanyError::UnknownCounterparty {
                counterparty: contract.counterparty,
            });
        }
        if contract.role == ContractRole::Borrowing {
            let requested = contract.principal;
            let outstanding = self
                .contracts
                .outstanding_borrowings(&contract.counterparty)?;
            let Some(credit_limit) = self.budget.credit_line(&contract.counterparty) else {
                return Err(CompanyError::NoCreditLine {
                    company: self.spec.id.clone(),
                    lender: contract.counterparty,
                    requested,
                });
            };
            let projected = outstanding.add(requested)?;
            if projected > credit_limit {
                return Err(CompanyError::DebtBeyondCreditLine {
                    company: self.spec.id.clone(),
                    lender: contract.counterparty,
                    credit_limit,
                    outstanding,
                    requested,
                });
            }
        }
        self.contracts.register(contract)
    }

    pub fn spec(&self) -> &CompanySpec {
        &self.spec
    }

    /// 公司权威账套（开局凭证已入账；经营过账入口随任务 8–11 提供）。
    pub fn books(&self) -> &Books {
        &self.books
    }

    pub fn counterparties(&self) -> &CounterpartyLedger {
        &self.counterparties
    }

    pub fn contracts(&self) -> &ContractBook {
        &self.contracts
    }

    pub fn budget(&self) -> &OperatingBudget {
        &self.budget
    }

    pub fn sub_ledgers(&self) -> &SubsidiaryLedgers {
        &self.sub_ledgers
    }
}

/// 公司注册表：规格集合校验 + 逐公司装配。任一失败 ⇒ 整体不产生（无部分状态）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompanyRegistry {
    companies: BTreeMap<CompanyId, Company>,
}

impl CompanyRegistry {
    pub fn new(configs: Vec<CompanyConfig>) -> Result<Self, CompanyError> {
        let specs: Vec<CompanySpec> = configs.iter().map(|config| config.spec.clone()).collect();
        CompanySpec::validate_set(&specs)?;
        let mut companies = BTreeMap::new();
        for config in configs {
            let company = Company::new(config)?;
            companies.insert(company.spec.id.clone(), company);
        }
        Ok(Self { companies })
    }

    /// 发行人映射精确校验：上市公司映射的股票必须在清单内且股本精确相等；
    /// 清单内每只股票必须有唯一发行人（唯一性由规格集合校验保证）。
    /// 输入是 `(股票代码, total_shares)` 对——公司域不依赖 session 类型。
    pub fn validate_issuer_mapping(&self, stocks: &[(StockCode, u64)]) -> Result<(), CompanyError> {
        for (id, company) in &self.companies {
            if let Some(stock) = &company.spec.listed_stock {
                let Some(total_shares) = stocks
                    .iter()
                    .find(|(code, _)| code == stock)
                    .map(|(_, total)| *total)
                else {
                    return Err(CompanyError::UnknownIssuerStock {
                        company: id.clone(),
                        stock: stock.clone(),
                    });
                };
                if company.spec.issued_shares != total_shares {
                    return Err(CompanyError::IssuedSharesMismatch {
                        company: id.clone(),
                        stock: stock.clone(),
                        issued_shares: company.spec.issued_shares,
                        total_shares,
                    });
                }
            }
        }
        for (code, _) in stocks {
            if self.issuer_of(code).is_none() {
                return Err(CompanyError::UnmappedStock {
                    stock: code.clone(),
                });
            }
        }
        Ok(())
    }

    /// 股票的发行人公司 id（未映射 = None）。
    pub fn issuer_of(&self, stock: &StockCode) -> Option<&CompanyId> {
        self.companies
            .values()
            .find(|company| company.spec.listed_stock.as_ref() == Some(stock))
            .map(|company| &company.spec.id)
    }

    pub fn get(&self, id: &CompanyId) -> Option<&Company> {
        self.companies.get(id)
    }

    pub fn get_mut(&mut self, id: &CompanyId) -> Option<&mut Company> {
        self.companies.get_mut(id)
    }

    pub fn len(&self) -> usize {
        self.companies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.companies.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&CompanyId, &Company)> {
        self.companies.iter()
    }
}
