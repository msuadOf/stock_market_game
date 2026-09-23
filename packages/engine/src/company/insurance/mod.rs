//! 保险经营会计（K3，任务 10）。
//!
//! [`InsuranceBooks`] = 权威账套（[`Books`]，任务 6）+ 合同组子账（GMM 组件：
//! 预期赔付/风险调整/CSM/亏损成分/贴现差 + 责任单元进度 + 赔案子账）+ 外部
//! 投保人对手方 + 版本化贴现假设。与任务 8/9 `IndustrialBooks`/`BankBooks`
//! 同一组合模式：**独立引擎**，不修改 `Company` 注册表壳；会话接线在任务 26。
//!
//! 计量框架：一般计量模型（GMM，CAS 25（2020）§21–§34），依据锚点见
//! docs/company-accounting.md §2.4（§27 初始确认/§28 未到期责任负债+已发生
//! 赔款负债/§30–32 责任单元释放/§33/34 保险财务损益/§46–49 亏损组/§84/85
//! 列报）。适用窗口：非境内外同时上市 2026-01-01 起执行——默认 2030 开局
//! 直接适用；更早开局为提前执行游戏假设（`game-assumption-early-adoption`）。
//!
//! 事件处理不变量（每个处理器一致执行）：
//! 1. **validate → post → apply**：全部业务校验先于过账；过账走
//!    [`Books::post_batch`] 原子提交；子账变更仅在过账成功后落地——任何
//!    拒绝（含 `PaymentFailed`）账套与子账**字节不变**（测试逐一断言），
//!    事件 id 不倒退（成功/零过账均单调推进，永不复用）。
//! 2. K3 红线：保费不立即全额计收入（收保费贷记 2501，收入随责任单元
//!    释放）；分红/投连/再保险一律类型化 `UnsupportedContract`，不把此类
//!    保费流变成股东支付。
//! 3. 现金流分类：保费收讫/赔款支付均经营活动（CAS 30（2026）§55(二)）。
//! 4. 客户流动性约束：赔款支付超可支付现金 → `PaymentFailed`（类型化，
//!    险企继续运行；无透支、无自动补钱、K2 无兜底）。
//!
//! [`Books::post_batch`]: crate::accounting::Books::post_batch

mod chart;
mod claims;
mod config;
mod csm;
mod error;
mod groups;
mod premium;
mod remeasure;
mod service_release;

pub use chart::insurance_chart_v4;
pub use claims::{ClaimId, ClaimState};
pub use config::{DiscountAssumption, InsuranceConfig};
pub use error::InsuranceError;
pub use groups::ContractGroupState;

use crate::accounting::reports::insurance::{
    insurance_presentation_lines, InsurancePresentationLines,
};
use crate::accounting::{AccountingAmount, AccountingError, Books, JournalLine, LedgerAccountId};
use crate::company::contracts::ContractId;
use crate::company::counterparty::{
    CounterpartyFlow, CounterpartyId, CounterpartyLedger, FlowDirection,
};
use crate::company::opening::opening_event_id;

use std::collections::BTreeMap;

/// 保险产品种类（入口显式分类；未支持种类一律类型化 `UnsupportedContract`，
/// docs/company-accounting.md §6——不冒充已实现、不静默 fallback）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum InsuranceProductKind {
    /// 定期保障型（支持；一般计量模型的明确期限非分红合同）。
    TermProtection,
    /// 分红（参与分配）保险合同（不支持：CAS 25 §29/§39–§44 直接参与分红
    /// 特征——docs §6 第 4 项）。
    Participating,
    /// 投连险合同（不支持：CAS 25 §39–§44 基础项目/浮动收费安排）。
    UnitLinked,
    /// 分出的再保险合同（不支持：CAS 25 §58–§72；K3 不做任何分出/分入分录）。
    ReinsuranceCeded,
    /// 分入的再保险合同（不支持：同上）。
    ReinsuranceAccepted,
}

impl InsuranceProductKind {
    fn require_supported(self) -> Result<(), InsuranceError> {
        match self {
            InsuranceProductKind::TermProtection => Ok(()),
            other => Err(InsuranceError::UnsupportedContract {
                kind: other,
                detail: "only TermProtection (GMM, definite term) is supported (docs §6)",
            }),
        }
    }
}

/// 保险账套：Books + 合同组子账 + 对手方 + 贴现假设（全部随存档序列化；
/// `Books` 恢复走重放路径，其余结构体 serde 直存）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct InsuranceBooks {
    books: Books,
    groups: BTreeMap<ContractId, ContractGroupState>,
    counterparties: CounterpartyLedger,
    discount: DiscountAssumption,
    next_event_id: u64,
}

impl InsuranceBooks {
    /// 装配：贴现假设校验 → 开局行子账种子守卫 → 开局凭证过账（原子）→
    /// 对手方登记。任一步失败 ⇒ 不产生半构造账套。
    pub fn new(config: InsuranceConfig) -> Result<Self, InsuranceError> {
        config.discount.validate()?;
        config::check_opening_lines(&config.opening_lines)?;
        let mut books = Books::new(config.chart);
        books.post_batch(vec![crate::accounting::JournalEntry {
            source: opening_event_id(),
            date: config.as_of,
            kind: crate::accounting::BusinessKind::OpeningBalance,
            cash_flow: crate::accounting::CashFlowClass::Financing,
            lines: config.opening_lines,
        }])?;
        let mut counterparties = CounterpartyLedger::new();
        for counterparty in config.counterparties {
            counterparties.register(counterparty)?;
        }
        Ok(Self {
            books,
            groups: BTreeMap::new(),
            counterparties,
            discount: config.discount,
            next_event_id: 2,
        })
    }

    // ===== 只读访问面（子账与状态）=====

    pub fn books(&self) -> &Books {
        &self.books
    }

    pub fn counterparties(&self) -> &CounterpartyLedger {
        &self.counterparties
    }

    pub fn group(&self, id: &ContractId) -> Option<&ContractGroupState> {
        self.groups.get(id)
    }

    pub fn groups(&self) -> impl Iterator<Item = (&ContractId, &ContractGroupState)> {
        self.groups.iter()
    }

    pub fn discount(&self) -> &DiscountAssumption {
        &self.discount
    }

    /// 报表分类层胶水：总账 → CAS 25 §84/§85 + CAS 30 §55(二) 保险列报行
    /// （任务 13 消费）。
    pub fn presentation_lines(&self) -> Result<InsurancePresentationLines, AccountingError> {
        insurance_presentation_lines(self.books.ledger())
    }

    // ===== 处理器共享内部 =====

    /// 原子过账 + 领域错误映射 + 事件 id 提交：批末负现金 → `PaymentFailed`。
    /// `next_event_id` 只在过账路径执行后推进（成功/零过账均单调，永不复用）。
    pub(super) fn post_with_commit(
        &mut self,
        next_event_id: u64,
        entries: Vec<crate::accounting::JournalEntry>,
    ) -> Result<(), InsuranceError> {
        self.books
            .post_batch(entries)
            .map_err(error::map_post_error)?;
        self.next_event_id = next_event_id;
        Ok(())
    }

    /// 对手方已登记（处理器前置校验；资金流跨边界留痕的 K2 要求）。
    pub(super) fn ensure_counterparty(&self, id: &CounterpartyId) -> Result<(), InsuranceError> {
        if self.counterparties.get(id).is_none() {
            return Err(InsuranceError::Company(
                crate::company::CompanyError::UnknownCounterparty {
                    counterparty: id.clone(),
                },
            ));
        }
        Ok(())
    }

    /// 记录跨边界资金流（过账成功后；对手方已在 validate 段校验登记）。
    pub(super) fn record_flow(
        &mut self,
        date: crate::calendar::CivilDate,
        counterparty: &CounterpartyId,
        direction: FlowDirection,
        amount: AccountingAmount,
        memo: &str,
    ) -> Result<(), InsuranceError> {
        self.counterparties.record_flow(CounterpartyFlow {
            date,
            counterparty: counterparty.clone(),
            direction,
            amount,
            memo: memo.to_string(),
        })?;
        Ok(())
    }
}

/// 快速构造分录行。
pub(super) fn line(
    code: &str,
    side: crate::accounting::PostingSide,
    amount: AccountingAmount,
) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount,
    }
}
