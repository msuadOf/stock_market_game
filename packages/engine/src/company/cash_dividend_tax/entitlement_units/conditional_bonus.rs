//! Bonus 原权益的条件事实；不代表真实发股已安装或生产税务来源已闭合。
use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalBonusTaxFacts {
    pub events: Vec<ConditionalBonusTaxEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalBonusTaxEvent {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    pub after_book_operation: u64,
    pub day: CivilDate,
    pub action: ConditionalBonusTaxAction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ConditionalBonusTaxAction {
    RegisterBonus {
        event_id: String,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        shares_per_existing_share_micros: u64,
        par_value_per_share: Money,
    },
    RealizeBonus {
        event_id: String,
        actual_credit: ConditionalBonusCredit,
        #[serde(deserialize_with = "required_tax_acquisition")]
        tax_acquisition: Option<u64>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalBonusCredit {
    pub event_id: String,
    pub account: AccountId,
    pub stock: StockCode,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub credited_qty: u64,
}
fn required_tax_acquisition<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Option<u64>,D::Error> {
    Option::<u64>::deserialize(decoder)
}

impl CashDividendTaxBook {
    /// 仅在明确条件来源下评估原R Bonus；未接生产付款、收缴或存档。
    pub fn assessment_with_conditional_bonus(
        &self,
        lineage: &TaxUnitLineage,
        _bonus: &ConditionalBonusTaxFacts,
    ) -> Result<MappedTaxAssessment, TaxLineageError> {
        self.assessment_with_unit_lineage(lineage)
    }
}
