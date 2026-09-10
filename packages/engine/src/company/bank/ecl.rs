//! 预期信用损失（ECL）三阶段模型（K3 银行，任务 9）。
//!
//! 官方依据（已核验，docs/company-accounting.md §2.3）：CAS 22（2017）
//! §46–§48（三阶段：12 个月 / 显著增加→存续期 / 已发生信用减值）、§57–§58、
//! §60（初始确认处于阶段一的假设 + 概率加权）、§61。**K3 红线：PD/LGD/EAD
//! 全部是显式版本化配置/情景输入（游戏假设，测试标注 Fixture），绝不从股票
//! 跌幅或市场价格推导信用损失。**
//!
//! 计量（简化，登记于 issues）：准备目标 = rhe(Σ(权重×PD×LGD×账面余额)/
//! 10^12)——概率加权（三个基点因子）单次整数半偶舍入；不折现（无贴现参数
//! 即不虚构贴现假设）。EAD = 贷款本金 + 应计利息（账面余额）；阶段转移是
//! 显式事件（日期 + 理由留痕）。阶段 3 计息转净额法（见 interest.rs）。

use crate::accounting::{
    AccountingAmount, AccountingError, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry,
    PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::bank::error::map_post_error;
use crate::company::bank::{chart, BankBooks, BankError};
use crate::company::contracts::ContractId;

/// ECL 三阶段（CAS 22 §46–§48）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum EclStage {
    /// 阶段一：信用风险未显著增加 → 12 个月 ECL。
    Stage1,
    /// 阶段二：信用风险显著增加 → 整个存续期 ECL。
    Stage2,
    /// 阶段三：已发生信用减值 → 存续期 ECL + 净额法计息。
    Stage3,
}

/// 单个 ECL 情景：概率权重 + 违约概率 + 违约损失率（全部整数基点）。
/// 一个情景列表 = 一次概率加权计量（Σ权重须 = 10000bp）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct EclScenario {
    /// 概率权重（0..=10000bp；列表内 Σ = 10000bp）。
    pub weight_bp: i32,
    /// 违约概率 PD（0..=10000bp；阶段一列表 = 12 个月 PD，存续期列表 = 终身 PD）。
    pub pd_bp: i32,
    /// 违约损失率 LGD（0..=10000bp）。
    pub lgd_bp: i32,
}

/// 版本化 ECL 政策：阶段一（初始确认日终计量）与存续期（阶段二/三重估）的
/// 默认概率加权情景。生产默认值属游戏假设（待校准），测试一律标注 Fixture。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct EclPolicy {
    pub version: u32,
    pub stage1_default: Vec<EclScenario>,
    pub lifetime_default: Vec<EclScenario>,
}

/// 阶段转移记录（子账留痕：日期 + 目标阶段 + 理由）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct StageTransferRecord {
    pub date: CivilDate,
    pub from_stage: EclStage,
    pub to_stage: EclStage,
    pub reason: String,
}

/// 单情景字段域（0..=10000bp）。
const BP_CAP: i32 = 10_000;

impl EclScenario {
    /// 字段校验：权重/PD/LGD 各自落在 [0, 10000]bp。
    pub fn validate(&self) -> Result<(), BankError> {
        for (label, value) in [
            ("weight_bp", self.weight_bp),
            ("pd_bp", self.pd_bp),
            ("lgd_bp", self.lgd_bp),
        ] {
            if !(0..=BP_CAP).contains(&value) {
                return Err(BankError::EclInvalid {
                    detail: format!("{label} {value}bp out of [0, {BP_CAP}]"),
                });
            }
        }
        Ok(())
    }
}

impl EclPolicy {
    /// 政策校验：两张情景表非空、逐情景合法、各自概率权重和恰为 10000bp。
    pub fn validate(&self) -> Result<(), BankError> {
        validate_scenarios("stage1_default", &self.stage1_default)?;
        validate_scenarios("lifetime_default", &self.lifetime_default)
    }
}

fn validate_scenarios(label: &str, scenarios: &[EclScenario]) -> Result<(), BankError> {
    if scenarios.is_empty() {
        return Err(BankError::EclInvalid {
            detail: format!("{label} scenario list is empty"),
        });
    }
    let mut total: i64 = 0;
    for scenario in scenarios {
        scenario.validate()?;
        total += i64::from(scenario.weight_bp);
    }
    if total != i64::from(BP_CAP) {
        return Err(BankError::EclInvalid {
            detail: format!("{label} weights sum to {total}bp, expected {BP_CAP}bp"),
        });
    }
    Ok(())
}

/// 概率加权准备目标：rhe(Σ(权重×PD×LGD×gross)/10^12)——三个基点因子
/// （10^4×10^4×10^4），单次舍入，无中间舍入损耗。gross ≥ 0 且各因子 ≥ 0
/// ⇒ 结果 ≥ 0。
pub(super) fn ecl_allowance_target(
    gross: AccountingAmount,
    scenarios: &[EclScenario],
) -> Result<AccountingAmount, BankError> {
    /// 权重×PD×LGD 的基点分母（10^12）。
    const ECL_DENOM: i128 = 1_000_000_000_000;
    let mut scaled: i128 = 0;
    for scenario in scenarios {
        let factor = i128::from(scenario.weight_bp)
            .checked_mul(i128::from(scenario.pd_bp))
            .and_then(|v| v.checked_mul(i128::from(scenario.lgd_bp)))
            .ok_or(BankError::Accounting(AccountingError::AmountOverflow {
                op: "ecl factor",
                detail: format!(
                    "{}×{}×{}",
                    scenario.weight_bp, scenario.pd_bp, scenario.lgd_bp
                ),
            }))?;
        let contribution = gross
            .cents()
            .checked_mul(factor)
            .ok_or(BankError::Accounting(AccountingError::AmountOverflow {
                op: "ecl target",
                detail: format!("{} × {factor}", gross.cents()),
            }))?;
        scaled = scaled
            .checked_add(contribution)
            .ok_or(BankError::Accounting(AccountingError::AmountOverflow {
                op: "ecl target",
                detail: format!("{scaled} + {contribution}"),
            }))?;
    }
    let target = crate::company::bank::loans::rhe_div(scaled, ECL_DENOM)?;
    Ok(AccountingAmount::from_cents(target))
}

impl BankBooks {
    /// 信用重估 / 阶段转移（显式事件，CAS 22 §46–§48/§60/§61）：
    /// 1. 校验贷款存在、情景合法、（阶段转移时）贷款未核销；
    /// 2. 概率加权目标 vs 当前准备 → 差额过账（补提：Dr 6701 / Cr 1303；
    ///    转回：Dr 1303 / Cr 6701；非现金）；
    /// 3. 子账落地：阶段 + 准备 + 转移留痕。
    ///
    /// 差额为零不产生分录（返回 None），阶段与留痕照常更新。
    /// 已核销贷款允许**同阶段**重估（回收后准备转回路径）；改阶段被拒。
    pub fn assess_credit(
        &mut self,
        loan: &ContractId,
        date: CivilDate,
        target_stage: EclStage,
        reason: &str,
        scenarios: Vec<EclScenario>,
    ) -> Result<Option<BusinessEventId>, BankError> {
        validate_scenarios("assess", &scenarios)?;
        let state = self.loan(loan).ok_or(BankError::UnknownLoan {
            contract: loan.clone(),
        })?;
        if state.is_written_off() && target_stage != state.stage() {
            return Err(BankError::StageTransferOnWrittenOff {
                contract: loan.clone(),
                to_stage: target_stage,
            });
        }
        let target = ecl_allowance_target(state.gross_carrying(), &scenarios)?;
        let delta = target.sub(state.allowance())?;
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        let entries = if delta.is_zero() {
            Vec::new()
        } else {
            let magnitude = if delta.is_positive() {
                delta
            } else {
                delta.neg()?
            };
            let (debit_account, credit_account) = if delta.is_positive() {
                (chart::acct::CREDIT_IMPAIR, chart::acct::LOAN_ALLOWANCE)
            } else {
                (chart::acct::LOAN_ALLOWANCE, chart::acct::CREDIT_IMPAIR)
            };
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::CreditImpairment,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(debit_account, PostingSide::Debit, magnitude),
                    super::line(credit_account, PostingSide::Credit, magnitude),
                ],
            }]
        };
        let posted = if entries.is_empty() {
            None
        } else {
            Some(event)
        };
        self.books.post_batch(entries).map_err(map_post_error)?;
        self.next_event_id = base + 1;
        if let Some(state) = self.loans.get_mut(loan) {
            state.apply_assessment(target, date, target_stage, reason);
        }
        Ok(posted)
    }
}
