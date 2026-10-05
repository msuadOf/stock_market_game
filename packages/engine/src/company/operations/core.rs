//! 经营编排核心：`CompanyOperations` 装配与访问面。冲击注入在
//! `injections.rs`；逐日推进循环在 `day.rs`；前史生成在 `history.rs`。

use std::{collections::BTreeMap, sync::OnceLock};

use crate::accounting::{AccountingAmount, Books};
use crate::calendar::CivilDate;
use crate::company::events::{ShockKind, ShockParams};
use crate::company::operations::config::{
    CompanyOperationsConfig, FlowParams, IndustryBooks, IndustryPairView, OperatingCompanyConfig,
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

pub struct OperatingReportCorrection<'a> {
    pub closing: &'a mut crate::accounting::closing::ClosingEngine,
    pub library: &'a mut crate::information::PublicLibrary,
    pub correction: crate::accounting::closing::CorrectionRequest,
    pub publication: crate::information::PublicationRequest,
    pub posted_on: CivilDate,
}

impl OperatingCompany {
    pub fn spec(&self) -> &crate::company::spec::CompanySpec {
        &self.spec
    }

    pub fn books(&self) -> &IndustryBooks {
        &self.books
    }

    /// 权威账套可变访问（会话装配与执行封账接缝——仅结账引擎使用；经营过账
    /// 仍走行业处理器的 validate→post→apply 路径）。
    pub fn books_mut(&mut self) -> &mut Books {
        self.books.books_mut()
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

/// 付款失败业务状态（会计与资金边界：PaymentFailed 的经营层记录面——公司存活、不补钱）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct PaymentFailureRecord {
    pub company: CompanyId,
    pub what: String,
    pub amount: AccountingAmount,
    pub obligation_status: crate::company::events::PaymentObligationStatus,
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

/// 诊断状态哈希使用的固定大小内容投影。
///
/// 公司 journal 在市场 tick 内不变，但前史生成后可能达到数 MB；在每次
/// 提交前校验中重新序列化会使回滚守卫成为 tick 的主要开销。
/// 缓存属于派生状态：每次 `CompanyOperations` 变更都使其失效，clone 继承
/// 已验证的投影，反序列化则从空缓存开始。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) struct CompanyOperationsHashProjection {
    serialized_len: usize,
    digest: u64,
}

#[derive(Default)]
pub(super) struct CompanyOperationsHashCache(OnceLock<CompanyOperationsHashProjection>);

impl Clone for CompanyOperationsHashCache {
    fn clone(&self) -> Self {
        let cache = Self::default();
        if let Some(projection) = self.0.get() {
            let _ = cache.0.set(*projection);
        }
        cache
    }
}

impl std::fmt::Debug for CompanyOperationsHashCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("CompanyOperationsHashCache")
            .field(&self.0.get())
            .finish()
    }
}

// The cache is a projection of the serialized fields, not part of business equality.
impl PartialEq for CompanyOperationsHashCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for CompanyOperationsHashCache {}

/// 经营编排引擎（经营与信息披露）。持有调度器、分流 RNG 与全部公司；serde 全量持久化。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize)]
pub struct CompanyOperations {
    #[serde(with = "crate::session::u64_decimal")]
    pub(crate) seed: u64,
    pub(crate) shock_params: ShockParams,
    pub(crate) scheduler: OperatingScheduler,
    pub(crate) market_rng: OperatingRng,
    pub(crate) industry_rngs: BTreeMap<IndustryId, OperatingRng>,
    pub(crate) companies: BTreeMap<CompanyId, OperatingCompany>,
    pub(crate) next_expected: Option<CivilDate>,
    pub(crate) history: Option<HistoryMeta>,
    pub(crate) payment_failures: BTreeMap<CivilDate, Vec<PaymentFailureRecord>>,
    #[serde(skip, default)]
    pub(super) hash_projection_cache: CompanyOperationsHashCache,
}

impl CompanyOperations {
    pub(crate) fn correct_company_report_for_session(
        &mut self,
        id: &CompanyId,
        request: OperatingReportCorrection<'_>,
    ) -> Result<
        (
            crate::accounting::closing::ReportHandle,
            crate::information::PublicationId,
        ),
        crate::company::CompanyCorrectionError,
    > {
        use crate::company::report_correction::{prepare_correction, CompanyCorrectionInput};
        use crate::company::CompanyCorrectionError;
        let reason = request.correction.reason.clone();
        let invalid = |detail: String| CompanyCorrectionError::InvalidInput {
            reason: reason.clone(),
            detail,
        };
        let mut trial = self
            .companies
            .get(id)
            .cloned()
            .ok_or_else(|| invalid(format!("经营公司 {id:?} 不存在")))?;
        let member = crate::accounting::consolidation::MemberId(id.0.clone());
        if request.publication.company != *id
            || request.publication.scope
                != crate::accounting::consolidation::ScopeId::Standalone(member.clone())
        {
            return Err(invalid(
                "更正公司与单体 Scope 必须来自实际经营 owner".into(),
            ));
        }
        for entry in &request.correction.entries {
            for line in &entry.lines {
                let protected = match &trial.books {
                    IndustryBooks::Industrial(books) => {
                        [
                            "1122", "2202", "1403", "1405", "5001", "1601", "1602", "1603", "2001",
                            "2501", "1811", "222104", "6801",
                        ]
                        .contains(&line.account.0.as_str())
                            || (line.account.0 == "2231" && books.loans().next().is_some())
                    }
                    IndustryBooks::Bank(_) => [
                        "1131", "1301", "1303", "2011", "2601", "2231", "1811", "222104", "6801",
                    ]
                    .contains(&line.account.0.as_str()),
                    IndustryBooks::Insurance(_) => [
                        "1122", "2501", "2502", "6051", "6451", "6541", "1811", "222104", "6801",
                    ]
                    .contains(&line.account.0.as_str()),
                    IndustryBooks::RealEstate(_) => [
                        "1122", "1541", "1542", "2001", "2203", "2231", "2501", "1811", "222104",
                        "6801",
                    ]
                    .contains(&line.account.0.as_str()),
                };
                if protected {
                    return Err(invalid(format!(
                        "科目 {} 需要 {:?} 的结构化子账事实，不能直接通过 Journal 更正",
                        line.account.0, trial.spec.kind
                    )));
                }
            }
        }
        let (books, position, policy, next_event_id) = trial.books.income_tax_owner();
        let scratch = prepare_correction(CompanyCorrectionInput {
            books,
            position,
            policy,
            next_event_id,
            closing: request.closing,
            library: request.library,
            member: &member,
            industry: crate::information::industry_presentation(trial.spec.kind),
            correction: request.correction,
            publication: request.publication,
            posted_on: request.posted_on,
        })?;
        trial.books.install_income_tax_owner(
            scratch.books,
            scratch.position,
            scratch.next_event_id,
        );
        let encoded = serde_json::to_value(&trial.books).map_err(|cause| {
            CompanyCorrectionError::Serialization {
                reason: reason.clone(),
                cause: Box::new(cause),
            }
        })?;
        trial.books = serde_json::from_value(encoded).map_err(|cause| {
            CompanyCorrectionError::Serialization {
                reason: reason.clone(),
                cause: Box::new(cause),
            }
        })?;
        trial
            .books
            .validate_owner_state()
            .map_err(|cause| CompanyCorrectionError::Owner {
                reason,
                cause: Box::new(cause),
            })?;
        self.companies.insert(id.clone(), trial);
        *request.closing = scratch.closing;
        *request.library = scratch.library;
        self.invalidate_hash_projection();
        Ok((scratch.report, scratch.publication))
    }

    pub fn correct_industrial_report(
        &mut self,
        id: &CompanyId,
        request: OperatingReportCorrection<'_>,
    ) -> Result<
        (
            crate::accounting::closing::ReportHandle,
            crate::information::PublicationId,
        ),
        crate::company::industrial::IndustrialCorrectionError,
    > {
        use crate::company::industrial::{
            IndustrialCorrectionError, IndustrialError, IndustrialReportCorrection,
        };
        let reason = request.correction.reason.clone();
        let invalid = |detail| IndustrialCorrectionError::Industrial {
            reason: reason.clone(),
            cause: IndustrialError::IncomeTaxStateInconsistent { detail },
        };
        let company = self
            .companies
            .get(id)
            .ok_or_else(|| invalid(format!("经营公司 {id:?} 不存在")))?;
        if company.spec.group_parent.is_some()
            || self
                .companies
                .values()
                .any(|candidate| candidate.spec.group_parent.as_ref() == Some(id))
        {
            return Err(IndustrialCorrectionError::Industrial {
                reason,
                cause: IndustrialError::GroupedTaxCorrectionUnsupported {
                    company: id.clone(),
                },
            });
        }
        let member = crate::accounting::consolidation::MemberId(id.0.clone());
        if request.publication.company != *id
            || request.publication.scope
                != crate::accounting::consolidation::ScopeId::Standalone(member.clone())
        {
            return Err(invalid(
                "工商更正公司和单体 Scope 必须来自实际经营 owner".into(),
            ));
        }
        let company = self.companies.get_mut(id).expect("已验证经营公司存在");
        let IndustryBooks::Industrial(books) = &mut company.books else {
            return Err(invalid(format!("经营公司 {id:?} 不是工商账套")));
        };
        let result = books.correct_and_publish_with_tax(IndustrialReportCorrection {
            closing: request.closing,
            library: request.library,
            member: &member,
            correction: request.correction,
            publication: request.publication,
            posted_on: request.posted_on,
        })?;
        self.invalidate_hash_projection();
        Ok(result)
    }

    /// live 构造：提交首经营日的滚动利息 due（保险无计息承载面，不注册）。
    pub fn new(
        config: CompanyOperationsConfig,
        start_date: CivilDate,
    ) -> Result<Self, OperationsError> {
        let mut ops = Self::build(config, start_date, false)?;
        ops.submit_rolling_interest(start_date)?;
        ops.schedule_debt_maturities(start_date)?;
        Ok(ops)
    }

    /// 初始化专用前史生成（会计与资金边界：开局前 2 个完整自然年度 + 当年截至开局前日；
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
        for OperatingCompanyConfig {
            spec,
            mut books,
            flow,
        } in company_configs
        {
            spec.validate()?;
            flow.validate_durations(&spec)?;
            IndustryPairView::at_build_guard(&mut books, &flow, &spec)?;
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
            payment_failures: BTreeMap::new(),
            hash_projection_cache: CompanyOperationsHashCache::default(),
        })
    }

    // ===== 只读访问面 =====

    pub fn scheduler(&self) -> &OperatingScheduler {
        &self.scheduler
    }

    pub fn payment_failures_on(&self, date: CivilDate) -> &[PaymentFailureRecord] {
        self.payment_failures
            .get(&date)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn validate_payment_history(&self) -> Result<(), OperationsError> {
        for (date, failures) in &self.payment_failures {
            if Some(*date) >= self.next_expected || failures.is_empty() {
                return Err(OperationsError::InvalidPaymentHistory {
                    detail: format!("invalid payment failure date/list on {date}"),
                });
            }
            for failure in failures {
                if !self.companies.contains_key(&failure.company)
                    || failure.what.trim().is_empty()
                    || !failure.amount.is_positive()
                {
                    return Err(OperationsError::InvalidPaymentHistory {
                        detail: format!("invalid payment failure on {date}: {failure:?}"),
                    });
                }
            }
        }
        for company in self.companies.values() {
            if company
                .economy
                .active()
                .iter()
                .any(|shock| matches!(shock.kind, ShockKind::PaymentFailure { .. }))
            {
                return Err(OperationsError::InvalidPaymentHistory {
                    detail: format!(
                        "payment failure cannot be active economic shock for {:?}",
                        company.spec.id
                    ),
                });
            }
        }
        Ok(())
    }

    pub fn company(&self, id: &CompanyId) -> Option<&OperatingCompany> {
        self.companies.get(id)
    }

    /// 可变公司访问（会话装配与执行封账接缝：`close_month`/`close_year` 需要
    /// `&mut Books`；其他经营路径仍走日终编排，不经此面）。
    pub fn company_mut(&mut self, id: &CompanyId) -> Option<&mut OperatingCompany> {
        self.invalidate_hash_projection();
        self.companies.get_mut(id)
    }

    pub(crate) fn hash_projection(
        &self,
    ) -> Result<CompanyOperationsHashProjection, serde_json::Error> {
        if let Some(projection) = self.hash_projection_cache.0.get() {
            return Ok(*projection);
        }
        let bytes = serde_json::to_vec(self)?;
        let mut digest = 0xcbf29ce484222325_u64;
        for byte in &bytes {
            digest = (digest ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
        let projection = CompanyOperationsHashProjection {
            serialized_len: bytes.len(),
            digest,
        };
        let _ = self.hash_projection_cache.0.set(projection);
        Ok(projection)
    }

    pub(crate) fn invalidate_hash_projection(&mut self) {
        self.hash_projection_cache = CompanyOperationsHashCache::default();
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
