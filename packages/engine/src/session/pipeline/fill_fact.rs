//! 收据产出点的唯一规范化与不可拆分批载体；消费者只借用已验证事实。
use super::{EnvelopeKey, EnvelopeReceipt, FeeComponents, ReceiptIndex, ReceiptKind, StepFatal};
use crate::Money;
use std::collections::BTreeSet;

pub(super) struct FillFactInput<'a> {
    pub index: ReceiptIndex,
    pub key: &'a EnvelopeKey,
    pub qty_before: u32,
    pub qty_after: u32,
    pub value_before: Money,
    pub value_after: Money,
    pub charged: FeeComponents,
}

/// 字段私有、不可serde构造；qty/gross只在唯一产出入口计算。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FillFact {
    index: ReceiptIndex,
    key: EnvelopeKey,
    qty: u32,
    gross: Money,
    charged: FeeComponents,
}

impl FillFact {
    pub(super) fn new(input: FillFactInput<'_>) -> Result<Self, StepFatal> {
        let qty = input.qty_before.checked_sub(input.qty_after).filter(|qty| *qty > 0)
            .ok_or_else(|| invariant("fill fact 数量回退或为零"))?;
        let gross = input.value_after.sub(input.value_before)
            .map_err(|error| invariant(&error.to_string()))?;
        if gross <= Money::ZERO { return Err(invariant("fill fact 成交额非正")); }
        Ok(Self { index: input.index, key: input.key.clone(), qty, gross, charged: input.charged })
    }

    pub(super) const fn index(&self) -> ReceiptIndex { self.index }
    pub(super) const fn key(&self) -> &EnvelopeKey { &self.key }
    pub(super) const fn qty(&self) -> u32 { self.qty }
    pub(super) const fn gross(&self) -> Money { self.gross }
    pub(super) const fn charged(&self) -> FeeComponents { self.charged }
}

/// raw审计与typed事实同一对象，贯穿stock/finalizer/market/settlement/经验链。
/// 无serde、无可变访问器；消费侧不能拆出后再构造另一份事实。
#[derive(Clone, Debug, Default)]
pub(super) struct ReceiptBatch {
    raw: Vec<EnvelopeReceipt>,
    fills: Vec<FillFact>,
}

impl ReceiptBatch {
    /// 只由ReceiptAggregation在完整ledger校验成功后调用；这是真正规范化入口。
    pub(super) fn from_aggregation(raw: Vec<EnvelopeReceipt>) -> Result<Self, StepFatal> {
        let fills = derive_batch(&raw)?;
        Ok(Self { raw, fills })
    }

    pub(super) fn raw(&self) -> &[EnvelopeReceipt] { &self.raw }

    /// T5（cfg(test) 克隆分支统一）：测试检视副本的**纯字段克隆**——不经
    /// from_aggregation 再派生（fill 规范化恰一次的钉不受扰动）。
    #[cfg(test)]
    pub(super) fn clone_for_inspect(&self) -> Self {
        Self {
            raw: self.raw.clone(),
            fills: self.fills.clone(),
        }
    }
    pub(super) fn fill_facts(&self) -> FillFactView<'_> { FillFactView::all(&self.fills) }
    pub(super) fn into_raw(self) -> Vec<EnvelopeReceipt> { self.raw }

    pub(super) fn fill_for(&self, receipt: &EnvelopeReceipt) -> Result<&FillFact, StepFatal> {
        let position = self.fills.binary_search_by_key(&ReceiptIndex(receipt.index), FillFact::index)
            .map_err(|_| invariant("Fill收据缺少唯一已验证事实"))?;
        let fact = &self.fills[position];
        if fact.key() != &receipt.envelope { return Err(invariant("Fill事实身份与审计收据不一致")); }
        Ok(fact)
    }

    /// PreSeal只有已应用的Release；借用式合并只搬审计元数据并复用同批Fill事实，
    /// 不重新派生。原批次保持股票批契约（不含前缀）供market/evidence使用。
    pub(super) fn prepend_releases(&self, prefix: &[EnvelopeReceipt]) -> Result<Self, StepFatal> {
        if prefix.iter().any(|row| row.kind != ReceiptKind::Release
            || !matches!(row.local_key.source(), super::ReceiptSource::QuoteExpiry(_))) {
            return Err(invariant("PreSeal前缀只能包含QuoteExpiry Release"));
        }
        let mut raw = Vec::with_capacity(prefix.len().saturating_add(self.raw.len()));
        raw.extend_from_slice(prefix);
        raw.extend_from_slice(&self.raw);
        if raw.windows(2).any(|rows| rows[0].index.checked_add(1) != Some(rows[1].index)) {
            return Err(invariant("合并收据前缀的全局索引不连续唯一"));
        }
        Ok(Self { raw, fills: self.fills.clone() })
    }
}

impl std::ops::Deref for ReceiptBatch {
    type Target = [EnvelopeReceipt];
    fn deref(&self) -> &Self::Target { self.raw() }
}
impl<'a> IntoIterator for &'a ReceiptBatch {
    type Item = &'a EnvelopeReceipt;
    type IntoIter = std::slice::Iter<'a, EnvelopeReceipt>;
    fn into_iter(self) -> Self::IntoIter { self.raw.iter() }
}

/// seen过滤只选择同一批事实的引用，不clone/rederive事实，不重复插入seen树。
#[derive(Clone, Copy)]
pub(super) struct FillFactView<'a> {
    facts: &'a [FillFact],
    selected: Option<&'a BTreeSet<ReceiptIndex>>,
}
impl<'a> FillFactView<'a> {
    pub(super) const fn all(facts: &'a [FillFact]) -> Self { Self { facts, selected: None } }
    pub(super) fn select(self, indices: &'a BTreeSet<ReceiptIndex>) -> Self {
        Self { facts: self.facts, selected: Some(indices) }
    }
    pub(super) fn iter(self) -> impl Iterator<Item = &'a FillFact> {
        self.facts.iter().filter(move |fact| self.selected.is_none_or(|keys| keys.contains(&fact.index())))
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static FILL_FACT_DERIVATION_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn derive_batch(receipts: &[EnvelopeReceipt]) -> Result<Vec<FillFact>, StepFatal> {
    derive_in_order(receipts.iter())
}

/// 旧独立projection测试的fixture产出适配；生产消费者不可调用此入口。
#[cfg(test)]
pub(super) fn derive_refs(receipts: &[&EnvelopeReceipt]) -> Result<Vec<FillFact>, StepFatal> {
    derive_in_order(receipts.iter().copied())
}

fn derive_in_order<'r>(receipts: impl Iterator<Item = &'r EnvelopeReceipt>) -> Result<Vec<FillFact>, StepFatal> {
    #[cfg(test)]
    FILL_FACT_DERIVATION_COUNT.with(|count| count.set(count.get() + 1));
    let mut facts = Vec::new();
    for receipt in receipts {
        if receipt.kind != ReceiptKind::Fill { continue; }
        facts.push(FillFact::new(FillFactInput { index: ReceiptIndex(receipt.index), key: &receipt.envelope,
            qty_before: receipt.qty_before, qty_after: receipt.qty_after,
            value_before: receipt.value_before, value_after: receipt.value_after, charged: receipt.charged })?);
    }
    Ok(facts)
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation { description: description.to_owned(), location: "pipeline::fill_fact".to_owned() }
}
