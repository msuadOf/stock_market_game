//! 报表更正的共同命令 DTO；不等同模型私有的披露 proof 或已安装结果。
//!
//! 字段恢复自既有 CompanyReportCorrection 生产合同。ReportCorrectionEpoch
//! 是会话内 Weak 身份，不进入 JSON；本模块不创建排队、取消或提交入口。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
    LedgerAccountId, PostingSide,
};
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::CompanyId;
use crate::information::PublicationId;
use std::collections::BTreeMap;

/// operation_id 是更正操作身份，不是交易 PlanId；reason 是用户领域内容。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyReportCorrection {
    pub operation_id: String,
    pub company: CompanyId,
    pub supersedes: PublicationId,
    #[serde(deserialize_with = "strict_entries")]
    pub entries: Vec<JournalEntry>,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletedReportCorrection {
    pub request: CompanyReportCorrection,
    pub publications: Vec<PublicationId>,
    #[serde(deserialize_with = "strict_committed_at")]
    pub committed_at: CivilInstant,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportCorrectionStatus {
    pub pending: Vec<CompanyReportCorrection>,
    #[serde(deserialize_with = "strict_completed")]
    pub completed: BTreeMap<String, CompletedReportCorrection>,
}

fn strict_committed_at<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<CivilInstant, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StrictInstant {
        date: CivilDate,
        second_of_day: u32,
    }
    let instant = <StrictInstant as serde::Deserialize>::deserialize(deserializer)?;
    CivilInstant::new(instant.date, instant.second_of_day).map_err(serde::de::Error::custom)
}

pub(super) fn strict_completed<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, CompletedReportCorrection>, D::Error> {
    struct CompletedVisitor;
    impl<'de> serde::de::Visitor<'de> for CompletedVisitor {
        type Value = BTreeMap<String, CompletedReportCorrection>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("无重复操作身份的已完成更正表")
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut completed = BTreeMap::new();
            while let Some((key, value)) = map.next_entry()? {
                if completed.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("已完成更正包含重复操作身份"));
                }
            }
            Ok(completed)
        }
    }
    deserializer.deserialize_map(CompletedVisitor)
}

// JournalEntry 的一般 serde 不拒绝未知字段；命令边界补结构严格性，业务
// 不变量仍交真实 JournalEntry::validate_invariants，不复制会计算法。
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictJournalEntry {
    source: BusinessEventId,
    date: CivilDate,
    kind: BusinessKind,
    cash_flow: CashFlowClass,
    lines: Vec<StrictJournalLine>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictJournalLine {
    account: LedgerAccountId,
    side: PostingSide,
    amount: AccountingAmount,
}

fn strict_entries<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<JournalEntry>, D::Error> {
    use serde::Deserialize;
    Ok(Vec::<StrictJournalEntry>::deserialize(deserializer)?
        .into_iter()
        .map(|entry| JournalEntry {
            source: entry.source,
            date: entry.date,
            kind: entry.kind,
            cash_flow: entry.cash_flow,
            lines: entry
                .lines
                .into_iter()
                .map(|line| JournalLine {
                    account: line.account,
                    side: line.side,
                    amount: line.amount,
                })
                .collect(),
        })
        .collect())
}
