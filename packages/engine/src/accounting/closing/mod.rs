//! 结账：月末封月 / 年末结账 / 季报与半年报快照 / 差错更正。
//!
//! **期间版本语义**：
//! - `close_month`：试算平衡 → 生成五产物 → 勾稽与比较项诚实性校验 →
//!   封账（拒绝后续入账）→ 不可变版本（sequence 1 起）入库；
//! - `close_year`：12 月封月 + 年报版本（年报 = 全年窗口五产物）；
//! - `snapshot_interim`：季报/半年报 = 已定稿业务范围的**不可变快照**，
//!   不封账、不阻断后续月入账（快照由窗口界定，可从推进后的账套重建）；
//! - `correct`：差错更正（CAS 28 现行口径，存在性经 CAS 30 §63 核验）——
//!   调整分录过账于**当前开放期间**（已封期间的入账守卫由底座强制），经
//!   重述映射作用于目标历史期间的资产负债表/利润表窗口；现金流量表保持
//!   实际收付期间（不双计现金）。更正 = 新版本经 `supersedes` 链接前版，
//!   **原版本永不改写**（重述底稿 = BusinessEventId → 目标期间的纯映射，
//!   不触碰原凭证）。
//!
//! **重述底稿持久化（追溯重述语义）**：`correct` 把（Scope → 来源 → 目标
//! 期间）累积进引擎状态，并在该 Scope 的**所有后续生成**（封月/年结/快照/
//! 再次更正）中生效——调整分录的损益只归入目标历史期间与后续期间的期初
//! 留存收益/比较项，**绝不进入后续期间的当期利润表**（CAS 28 追溯重述法）。
//!
//! 已登记简化（issues.md）：年末不落结转分录（报表由窗口化分录推导，
//! 4103 本年利润科目保留未用；利润表累计列即全年结转成果的列报）。

use std::{collections::BTreeMap, sync::OnceLock};

use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::error::AccountingError;
use crate::accounting::journal::{BusinessEventId, JournalEntry};
use crate::accounting::period::AccountingPeriod;
use crate::accounting::reports::validate::prior_year_facts;
use crate::accounting::reports::{
    generate_report_set, validate_trial_balance, IndustryPresentation, ReportError, ReportKind,
    ReportRequest, ReportSet, ReportSource, ReportVersion, VersionKind,
};
use crate::accounting::Books;

mod save;

/// 版本键（Scope × 期间 × 种类）。
type VersionKey = (ScopeId, AccountingPeriod, ReportKind);

/// 累积重述底稿：Scope →（调整分录来源 → 目标历史期间）。serde 随引擎
/// 状态整体存取（存档形态见 `save`——JSON 键必须为字符串，故平铺为值）。
#[derive(Default, Clone, Debug)]
struct RestatementRegister {
    scopes: BTreeMap<ScopeId, BTreeMap<BusinessEventId, AccountingPeriod>>,
}

type RestatementRows = Vec<(ScopeId, Vec<(BusinessEventId, AccountingPeriod)>)>;

impl RestatementRegister {
    fn record_correction(
        &mut self,
        scope: ScopeId,
        entries: &[JournalEntry],
        target: AccountingPeriod,
    ) -> &BTreeMap<BusinessEventId, AccountingPeriod> {
        let worksheet = self.scopes.entry(scope).or_default();
        for entry in entries {
            worksheet.insert(entry.source, target);
        }
        worksheet
    }

    fn for_scope(&self, scope: &ScopeId) -> Option<&BTreeMap<BusinessEventId, AccountingPeriod>> {
        self.scopes.get(scope)
    }

    fn save_rows(&self) -> RestatementRows {
        self.scopes
            .iter()
            .map(|(scope, worksheet)| {
                (
                    scope.clone(),
                    worksheet
                        .iter()
                        .map(|(source, target)| (*source, *target))
                        .collect(),
                )
            })
            .collect()
    }

    fn from_rows(rows: RestatementRows) -> Self {
        let mut scopes = BTreeMap::new();
        for (scope, worksheet) in rows {
            if !worksheet.is_empty() {
                scopes.insert(scope, worksheet.into_iter().collect());
            }
        }
        Self { scopes }
    }
}

/// 结账引擎：不可变期间版本登记簿。
#[derive(Default, Clone, Debug)]
pub struct ClosingEngine {
    versions: BTreeMap<VersionKey, Vec<ReportSet>>,
    restatements: RestatementRegister,
    hash_projection_cache: OnceLock<ClosingEngineHashProjection>,
}

/// Content-sensitive, constant-size projection used by diagnostic state hashes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) struct ClosingEngineHashProjection {
    serialized_len: usize,
    digest: u64,
}

/// 版本句柄（查询凭证）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ReportHandle {
    pub scope: ScopeId,
    pub period: AccountingPeriod,
    pub kind: ReportKind,
    pub sequence: u32,
}

/// 差错更正请求：调整分录（过账于目标之后的开放期间）+ 更正理由。
pub struct CorrectionRequest {
    pub entries: Vec<JournalEntry>,
    pub reason: String,
}

/// 单体生成目标（结账引擎内部参数组）。
struct StandaloneTarget<'a> {
    books: &'a Books,
    id: &'a MemberId,
    industry: IndustryPresentation,
    period: AccountingPeriod,
    kind: ReportKind,
}

/// 结账域错误（类型化，绝不静默；Report 装箱压缩 Err 值域——clippy 先例
/// 见合并域的 DeclaredSide）。
#[derive(Debug, thiserror::Error)]
pub enum ClosingError {
    #[error("report failure: {0}")]
    Report(Box<ReportError>),
    #[error("accounting failure: {0}")]
    Accounting(#[from] AccountingError),
    #[error("no published version to supersede for {scope} at {period}")]
    NoVersionToSupersede {
        scope: ScopeId,
        period: AccountingPeriod,
    },
    #[error("correction target {period} is not closed")]
    CorrectionTargetNotClosed { period: AccountingPeriod },
    #[error("correction entries must post after the target period (found {first_entry_period})")]
    CorrectionEntriesNotForward {
        first_entry_period: AccountingPeriod,
    },
}

/// `?` 直转（装箱由 From 承担——thiserror 的 `#[from]` 不覆盖值→箱路径）。
impl From<ReportError> for ClosingError {
    fn from(err: ReportError) -> Self {
        ClosingError::Report(Box::new(err))
    }
}

impl ClosingEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// 仅验证合并报告的归母损益源必填条件，不替代版本键、重述及完整恢复校验。
    pub(crate) fn validate_consolidated_parent_income(&self) -> Result<(), ReportError> {
        for report in self.versions.values().flatten() {
            report.validate_parent_income_source()?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn missing_parent_income_test_fixture() -> Self {
        let mut registry = tests::registry_with_group_report();
        registry.versions.values_mut().next().unwrap()[0]
            .income
            .net_income_to_parent = None;
        registry
    }

    /// 月末封月：试算 → 报表 → 勾稽 → 封账 → 版本入库。
    pub fn close_month(
        &mut self,
        books: &mut Books,
        id: &MemberId,
        industry: IndustryPresentation,
        period: AccountingPeriod,
    ) -> Result<ReportHandle, ClosingError> {
        self.invalidate_hash_projection();
        validate_trial_balance(&books.ledger().trial_balance()?)?;
        let set = self.generate_validated(StandaloneTarget {
            books,
            id,
            industry,
            period,
            kind: ReportKind::Monthly,
        })?;
        books.close_period(period)?;
        Ok(self.store(set))
    }

    /// 年末结账：12 月封月（月报版本）+ 年报版本。
    pub fn close_year(
        &mut self,
        books: &mut Books,
        id: &MemberId,
        industry: IndustryPresentation,
        year: i32,
    ) -> Result<(ReportHandle, ReportHandle), ClosingError> {
        self.invalidate_hash_projection();
        let december = AccountingPeriod::from_ymd(year, 12)?;
        let monthly = self.close_month(books, id, industry, december)?;
        let set = self.generate_validated(StandaloneTarget {
            books,
            id,
            industry,
            period: december,
            kind: ReportKind::Annual,
        })?;
        let annual = self.store(set);
        Ok((monthly, annual))
    }

    /// 季报/半年报快照：不可变、不封账、不阻断后续入账。
    pub fn snapshot_interim(
        &mut self,
        books: &Books,
        id: &MemberId,
        industry: IndustryPresentation,
        period: AccountingPeriod,
        kind: ReportKind,
    ) -> Result<ReportHandle, ClosingError> {
        self.invalidate_hash_projection();
        if !matches!(kind, ReportKind::Quarter | ReportKind::HalfYear) {
            return Err(ReportError::InvalidReportKind {
                period,
                reason: "interim snapshots cover quarter/half-year kinds",
            }
            .into());
        }
        validate_trial_balance(&books.ledger().trial_balance()?)?;
        let set = self.generate_validated(StandaloneTarget {
            books,
            id,
            industry,
            period,
            kind,
        })?;
        Ok(self.store(set))
    }

    /// 差错更正：过账调整分录（当前开放期间）→ 新版本（链接前版）。
    pub fn correct(
        &mut self,
        books: &mut Books,
        id: &MemberId,
        industry: IndustryPresentation,
        target: (AccountingPeriod, ReportKind),
        request: CorrectionRequest,
    ) -> Result<ReportHandle, ClosingError> {
        self.invalidate_hash_projection();
        let scope = ScopeId::Standalone(id.clone());
        let key = (scope.clone(), target.0, target.1);
        if books.journal().period_status(target.0) != crate::accounting::PeriodStatus::Closed {
            return Err(ClosingError::CorrectionTargetNotClosed { period: target.0 });
        }
        let previous = self
            .versions
            .get(&key)
            .map(|list| list.len() as u32)
            .filter(|count| *count > 0)
            .ok_or(ClosingError::NoVersionToSupersede {
                scope: scope.clone(),
                period: target.0,
            })?;
        for entry in &request.entries {
            if entry.period() <= target.0 {
                return Err(ClosingError::CorrectionEntriesNotForward {
                    first_entry_period: entry.period(),
                });
            }
        }
        // 原子过账（已封期间/重复来源/负现金由底座守卫拒绝；失败 ⇒ 零改动）。
        books.post_batch(request.entries.clone())?;
        // 累积进持久重述底稿：本 Scope 的所有后续生成从此带上该映射。
        let worksheet =
            self.restatements
                .record_correction(scope.clone(), &request.entries, target.0);
        let version = ReportVersion {
            sequence: previous + 1,
            supersedes: Some(previous),
            kind: VersionKind::Correction {
                reason: request.reason,
            },
        };
        let set = Self::generate_with(
            StandaloneTarget {
                books,
                id,
                industry,
                period: target.0,
                kind: target.1,
            },
            version,
            worksheet,
        )?;
        Ok(self.store(set))
    }

    /// 登记外部生成的版本（如合并 Scope 五产物；只做勾稽校验）。
    pub fn record(&mut self, set: ReportSet) -> Result<ReportHandle, ClosingError> {
        self.invalidate_hash_projection();
        set.validate()?;
        let handle = ReportHandle {
            scope: set.scope.clone(),
            period: set.period,
            kind: set.kind,
            sequence: set.version.sequence,
        };
        self.versions
            .entry((set.scope.clone(), set.period, set.kind))
            .or_default()
            .push(set);
        Ok(handle)
    }

    /// 某键下的全部版本（时间序）。
    pub fn versions(
        &self,
        scope: &ScopeId,
        period: AccountingPeriod,
        kind: ReportKind,
    ) -> &[ReportSet] {
        self.versions
            .get(&(scope.clone(), period, kind))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// 按序号取版本（不可变查询：原版本逐字节不变）。
    pub fn version(
        &self,
        scope: &ScopeId,
        period: AccountingPeriod,
        kind: ReportKind,
        sequence: u32,
    ) -> Option<&ReportSet> {
        self.versions(scope, period, kind)
            .get(sequence.checked_sub(1)? as usize)
    }

    pub(crate) fn hash_projection(&self) -> Result<ClosingEngineHashProjection, serde_json::Error> {
        if let Some(projection) = self.hash_projection_cache.get() {
            return Ok(*projection);
        }
        let bytes = serde_json::to_vec(self)?;
        let mut digest = 0xcbf29ce484222325_u64;
        for byte in &bytes {
            digest = (digest ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
        let projection = ClosingEngineHashProjection {
            serialized_len: bytes.len(),
            digest,
        };
        let _ = self.hash_projection_cache.set(projection);
        Ok(projection)
    }

    fn invalidate_hash_projection(&mut self) {
        self.hash_projection_cache = OnceLock::new();
    }

    fn next_sequence(&self, key: &VersionKey) -> u32 {
        self.versions
            .get(key)
            .map_or(1, |list| list.len() as u32 + 1)
    }

    /// 单体生成目标（内部参数组）。后续生成带上该 Scope 的累积重述底稿
    /// （更正损益归入目标历史期间，不进后续期间当期损益）。
    fn generate_validated(&self, target: StandaloneTarget<'_>) -> Result<ReportSet, ClosingError> {
        let scope = ScopeId::Standalone(target.id.clone());
        let sequence = self.next_sequence(&(scope.clone(), target.period, target.kind));
        let version = ReportVersion {
            sequence,
            supersedes: None,
            kind: VersionKind::Original,
        };
        let empty = BTreeMap::new();
        let adjustments = self.restatements.for_scope(&scope).unwrap_or(&empty);
        Self::generate_with(target, version, adjustments)
    }

    fn generate_with(
        target: StandaloneTarget<'_>,
        version: ReportVersion,
        adjustments: &BTreeMap<BusinessEventId, AccountingPeriod>,
    ) -> Result<ReportSet, ClosingError> {
        let set = generate_report_set(ReportRequest {
            period: target.period,
            kind: target.kind,
            source: ReportSource::Standalone {
                id: target.id.clone(),
                books: target.books,
                industry: target.industry,
            },
            version,
            adjustments,
        })?;
        set.validate()?;
        let (has_flows, has_end) = prior_year_facts(target.books, target.period);
        verify_comparative_honesty(&set, has_flows, has_end)?;
        Ok(set)
    }

    fn store(&mut self, set: ReportSet) -> ReportHandle {
        self.invalidate_hash_projection();
        let handle = ReportHandle {
            scope: set.scope.clone(),
            period: set.period,
            kind: set.kind,
            sequence: set.version.sequence,
        };
        self.versions
            .entry((set.scope.clone(), set.period, set.kind))
            .or_default()
            .push(set);
        handle
    }
}

/// 比较项诚实性守卫（公布前置；实现与附注勾稽同处 reports::validate）。
pub use crate::accounting::reports::validate::verify_comparative_honesty;

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn registry_with_group_report() -> ClosingEngine {
        use crate::accounting::consolidation::{ConsolidationRequest, GroupMember, MemberSpec};
        use crate::accounting::{
            AccountChart, AccountingAmount, BusinessKind, CashFlowClass, JournalLine,
            LedgerAccountId, PostingSide,
        };
        let mut books = Books::new(AccountChart::generic_account_chart());
        books
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(1),
                date: crate::calendar::CivilDate::from_iso("2029-12-31").unwrap(),
                kind: BusinessKind::OpeningBalance,
                cash_flow: CashFlowClass::Financing,
                lines: vec![
                    JournalLine {
                        account: LedgerAccountId("1002".into()),
                        side: PostingSide::Debit,
                        amount: AccountingAmount::from_cents(10000),
                    },
                    JournalLine {
                        account: LedgerAccountId("4001".into()),
                        side: PostingSide::Credit,
                        amount: AccountingAmount::from_cents(10000),
                    },
                ],
            }])
            .unwrap();
        let member = |id: &str, parent: Option<&str>, held| GroupMember {
            spec: MemberSpec {
                id: MemberId(id.into()),
                group_parent: parent.map(|id| MemberId(id.into())),
                issued_shares: 100,
                parent_held_shares: held,
            },
            books: &books,
        };
        let set = generate_report_set(ReportRequest {
            period: AccountingPeriod::from_iso("2030-01").unwrap(),
            kind: ReportKind::Monthly,
            source: ReportSource::Consolidated {
                request: ConsolidationRequest {
                    root: MemberId("root".into()),
                    members: vec![member("root", None, 0), member("sub", Some("root"), 80)],
                    intercompany_balances: vec![],
                    intercompany_sales: vec![],
                },
            },
            version: ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original,
            },
            adjustments: &BTreeMap::new(),
        })
        .unwrap();
        let mut registry = ClosingEngine::new();
        registry.record(set).unwrap();
        registry
    }

    #[test]
    fn closing_restore_rejects_consolidated_missing_parent_income() {
        let mut registry = registry_with_group_report();
        let report = &mut registry.versions.values_mut().next().unwrap()[0];
        report.income.net_income_to_parent = None;
        let serialized = serde_json::to_value(&registry).unwrap();
        let error = serde_json::from_value::<ClosingEngine>(serialized)
            .expect_err("closing restore must reject missing parent income");
        assert!(error.to_string().contains("income.net_income_to_parent"));
    }

    #[test]
    fn closing_restore_checks_parent_income_before_duplicate_key_overwrite() {
        let mut registry = registry_with_group_report();
        let valid_row = serde_json::to_value(&registry).unwrap()["versions"][0].clone();
        registry.versions.values_mut().next().unwrap()[0]
            .income
            .net_income_to_parent = None;
        let mut serialized = serde_json::to_value(&registry).unwrap();
        serialized["versions"]
            .as_array_mut()
            .unwrap()
            .push(valid_row);
        let error = serde_json::from_value::<ClosingEngine>(serialized)
            .expect_err("later duplicate key cannot hide missing parent income");
        assert!(error.to_string().contains("income.net_income_to_parent"));
    }

    #[test]
    fn closing_restore_preserves_valid_zero_parent_income() {
        let registry = registry_with_group_report();
        let restored: ClosingEngine =
            serde_json::from_value(serde_json::to_value(&registry).unwrap()).unwrap();
        assert_eq!(
            restored.versions.values().next().unwrap()[0]
                .income
                .net_income_to_parent,
            Some(crate::accounting::AccountingAmount::ZERO)
        );
    }

    #[test]
    fn closing_restore_preserves_standalone_parent_income_none() {
        let books = Books::new(crate::accounting::AccountChart::generic_account_chart());
        let set = generate_report_set(ReportRequest {
            period: AccountingPeriod::from_iso("2030-01").unwrap(),
            kind: ReportKind::Monthly,
            source: ReportSource::Standalone {
                id: MemberId("single".into()),
                books: &books,
                industry: IndustryPresentation::Industrial,
            },
            version: ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original,
            },
            adjustments: &BTreeMap::new(),
        })
        .unwrap();
        let mut registry = ClosingEngine::new();
        registry.record(set).unwrap();
        let restored: ClosingEngine =
            serde_json::from_value(serde_json::to_value(&registry).unwrap()).unwrap();
        restored.validate_consolidated_parent_income().unwrap();
        assert_eq!(
            restored.versions.values().next().unwrap()[0]
                .income
                .net_income_to_parent,
            None
        );
    }

    #[test]
    fn restatement_rows_preserve_overwrite_empty_and_scope_semantics() {
        let first = ScopeId::Standalone(MemberId("a".into()));
        let second = ScopeId::Standalone(MemberId("b".into()));
        let old = AccountingPeriod::from_ymd(2030, 1).unwrap();
        let new = AccountingPeriod::from_ymd(2030, 2).unwrap();
        let source = BusinessEventId::new(1);
        let register = RestatementRegister::from_rows(vec![
            (first.clone(), vec![(source, old)]),
            (second.clone(), vec![(source, old), (source, new)]),
            (first.clone(), vec![]),
            (second.clone(), vec![(source, old)]),
        ]);
        assert_eq!(register.for_scope(&first).unwrap()[&source], old);
        assert_eq!(register.for_scope(&second).unwrap()[&source], old);
        assert_eq!(
            register.save_rows(),
            vec![(first, vec![(source, old)]), (second, vec![(source, old)])]
        );
        let duplicate_sources = RestatementRegister::from_rows(vec![(
            ScopeId::Standalone(MemberId("c".into())),
            vec![(source, old), (source, new)],
        )]);
        assert_eq!(duplicate_sources.save_rows()[0].1, vec![(source, new)]);
    }
}
