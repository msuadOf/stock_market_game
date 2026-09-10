//! 经营编排核心：`CompanyOperations` 装配与访问面。冲击注入在
//! `injections.rs`；逐日推进循环在 `day.rs`；前史生成在 `history.rs`。

use std::collections::BTreeMap;

use crate::accounting::AccountingAmount;
use crate::calendar::CivilDate;
use crate::company::events::{ShockKind, ShockParams};
use crate::company::operations::config::{
    CompanyOperationsConfig, FlowParams, IndustryBooks, OperatingCompanyConfig,
};
use crate::company::operations::error::OperationsError;
use crate::company::operations::history::HistoryMeta;
use crate::company::operations::state::CompanyEconomicState;
use crate::company::rng::{OperatingRng, RngStream};
use crate::company::scheduler::OperatingScheduler;
use crate::company::spec::{CompanyId, IndustryId};

/// 单公司经营实体（账套 + 流参数 + 经营 RNG + 经济状态 + 流序号）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct OperatingCompany {
    pub(crate) spec: crate::company::spec::CompanySpec,
    pub(crate) books: IndustryBooks,
    pub(crate) params: FlowParams,
    pub(crate) rng: OperatingRng,
    pub(crate) economy: CompanyEconomicState,
    pub(crate) next_flow_seq: i64,
}

impl OperatingCompany {
    pub fn spec(&self) -> &crate::company::spec::CompanySpec {
        &self.spec
    }

    pub fn books(&self) -> &IndustryBooks {
        &self.books
    }

    pub fn economy(&self) -> &CompanyEconomicState {
        &self.economy
    }
}

/// 市场级激活记录（`company = None` 表示作用于全部公司）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ActivatedShockRecord {
    pub company: Option<CompanyId>,
    pub kind: ShockKind,
    pub amplitude_bp: i32,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExpiredShockRecord {
    pub company: CompanyId,
    pub kind: ShockKind,
}

/// 付款失败业务状态（K2：PaymentFailed 的经营层记录面——公司存活、不补钱）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct PaymentFailureRecord {
    pub company: CompanyId,
    pub what: String,
    pub amount: AccountingAmount,
}

/// 一次经营日的权威记录（确定性金样的比较面之一）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompanyDayReport {
    pub date: CivilDate,
    pub activated: Vec<ActivatedShockRecord>,
    pub expired: Vec<ExpiredShockRecord>,
    pub payment_failures: Vec<PaymentFailureRecord>,
    pub dispatched_due: usize,
    pub posted_entries: usize,
}

/// 经营编排引擎（K4）。持有调度器、分流 RNG 与全部公司；serde 全量持久化。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompanyOperations {
    pub(crate) seed: u64,
    pub(crate) shock_params: ShockParams,
    pub(crate) scheduler: OperatingScheduler,
    pub(crate) market_rng: OperatingRng,
    pub(crate) industry_rngs: BTreeMap<IndustryId, OperatingRng>,
    pub(crate) companies: BTreeMap<CompanyId, OperatingCompany>,
    pub(crate) next_expected: Option<CivilDate>,
    pub(crate) history: Option<HistoryMeta>,
}

impl CompanyOperations {
    /// live 构造：提交首经营日的滚动利息 due（保险无计息承载面，不注册）。
    pub fn new(
        config: CompanyOperationsConfig,
        start_date: CivilDate,
    ) -> Result<Self, OperationsError> {
        let mut ops = Self::build(config, start_date, false)?;
        ops.submit_rolling_interest(start_date)?;
        Ok(ops)
    }

    /// 初始化专用前史生成（K2：开局前 2 个完整自然年度 + 当年截至开局前日；
    /// 独立 init RNG 流；不创建历史证券成交、不组装公开报告）。
    pub fn generate_history(
        config: CompanyOperationsConfig,
        start_date: CivilDate,
    ) -> Result<Self, OperationsError> {
        crate::company::operations::history::generate_history(config, start_date)
    }

    /// 装配（live/前史共用；前史用 init 流）。不注册任何 due。
    pub(crate) fn build(
        config: CompanyOperationsConfig,
        first_day: CivilDate,
        init_streams: bool,
    ) -> Result<Self, OperationsError> {
        config.shock_params.validate()?;
        let CompanyOperationsConfig {
            seed,
            shock_params,
            companies: company_configs,
        } = config;
        let stream = |stream_kind: RngStream, stable_id: &str| {
            let kind = if init_streams {
                RngStream::InitHistory
            } else {
                stream_kind
            };
            OperatingRng::derive(seed, kind, stable_id)
        };
        let mut companies = BTreeMap::new();
        let mut industry_rngs = BTreeMap::new();
        for OperatingCompanyConfig { spec, books, flow } in company_configs {
            spec.validate()?;
            let consistent = spec.kind == books.kind()
                && matches!(
                    (&books, &flow),
                    (IndustryBooks::Industrial(_), FlowParams::Industrial(_))
                        | (IndustryBooks::Bank(_), FlowParams::Bank(_))
                        | (IndustryBooks::Insurance(_), FlowParams::Insurance(_))
                        | (IndustryBooks::RealEstate(_), FlowParams::RealEstate(_))
                );
            if !consistent {
                return Err(OperationsError::KindFlowMismatch {
                    company: spec.id.clone(),
                    kind: spec.kind,
                    flow: flow.variant_name(),
                });
            }
            industry_rngs
                .entry(spec.industry.clone())
                .or_insert_with(|| stream(RngStream::IndustryShock, &spec.industry.0));
            let rng = stream(RngStream::CompanyOperating, &spec.id.0);
            let id = spec.id.clone();
            companies.insert(
                id,
                OperatingCompany {
                    spec,
                    books,
                    params: flow,
                    rng,
                    economy: CompanyEconomicState::default(),
                    next_flow_seq: 1,
                },
            );
        }
        Ok(Self {
            seed,
            shock_params,
            scheduler: OperatingScheduler::new(),
            market_rng: stream(RngStream::MarketShock, "market"),
            industry_rngs,
            companies,
            next_expected: Some(first_day),
            history: None,
        })
    }

    // ===== 只读访问面 =====

    pub fn scheduler(&self) -> &OperatingScheduler {
        &self.scheduler
    }

    pub fn company(&self, id: &CompanyId) -> Option<&OperatingCompany> {
        self.companies.get(id)
    }

    pub fn shock_params(&self) -> &ShockParams {
        &self.shock_params
    }

    pub fn next_expected_date(&self) -> CivilDate {
        self.next_expected
            .expect("operations always constructed with a first day")
    }

    pub fn history_meta(&self) -> Option<&HistoryMeta> {
        self.history.as_ref()
    }

    fn books_of(&self, id: &CompanyId) -> Option<&IndustryBooks> {
        self.companies.get(id).map(|company| &company.books)
    }

    pub fn industrial_books(
        &self,
        id: &CompanyId,
    ) -> Option<&crate::company::industrial::IndustrialBooks> {
        self.books_of(id).and_then(|books| books.as_industrial())
    }

    pub fn bank_books(&self, id: &CompanyId) -> Option<&crate::company::bank::BankBooks> {
        self.books_of(id).and_then(|books| books.as_bank())
    }

    pub fn insurance_books(
        &self,
        id: &CompanyId,
    ) -> Option<&crate::company::insurance::InsuranceBooks> {
        self.books_of(id).and_then(|books| books.as_insurance())
    }

    pub fn real_estate_books(
        &self,
        id: &CompanyId,
    ) -> Option<&crate::company::real_estate::RealEstateBooks> {
        self.books_of(id).and_then(|books| books.as_real_estate())
    }
}
