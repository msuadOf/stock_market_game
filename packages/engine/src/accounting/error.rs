//! 会计域统一错误（K2）。绝不静默吞掉（铁律二）：每个变体携带定位与数值上下文。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::journal::{BusinessEventId, PostingSide};
use crate::accounting::ledger::LedgerAccountId;
use crate::accounting::period::AccountingPeriod;
use crate::calendar::CivilDate;
use thiserror::Error;

/// 会计操作失败。批次内失败统一包成 [`AccountingError::BatchAborted`]，
/// `cause` 内是首条被拒原因（可 `matches!` 精确匹配）。
#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum AccountingError {
    /// i128 分运算溢出（加/减/乘/基点缩放）。数值游戏不应触达，触达即显式失败。
    #[error("amount overflow in {op}: {detail}")]
    AmountOverflow { op: &'static str, detail: String },

    /// 十进制字符串解析失败（非法精度 >2 位 / 非数字 / 空 / 多小数点 / 超 i128）。
    #[error("decimal parse failed: input {input:?}: {reason}")]
    DecimalParse { input: String, reason: String },

    /// 与市场域 `Money`（i64 分）互转时超出 i64 值域。
    #[error("amount {cents} cents exceeds Money (i64) range")]
    MoneyRangeExceeded { cents: i128 },

    /// 分录借贷总额不等（复式记账基本恒等式被破坏）。
    #[error("unbalanced entry for event {event:?} on {date}: debits {debit_total}, credits {credit_total}")]
    Unbalanced {
        event: BusinessEventId,
        date: CivilDate,
        debit_total: AccountingAmount,
        credit_total: AccountingAmount,
    },

    /// 行金额必须为正：方向由借贷表示，负数来自对方科目，不来自行符号。
    #[error(
        "non-positive line amount {amount:?} on {side:?} of account {account} (event {event:?})"
    )]
    NonPositiveLine {
        event: BusinessEventId,
        account: LedgerAccountId,
        side: PostingSide,
        amount: AccountingAmount,
    },

    /// 只有借方或只有贷方（含空行）：不构成复式分录。
    #[error("entry {event:?} lacks {missing} side lines")]
    MissingSide {
        event: BusinessEventId,
        missing: &'static str,
    },

    /// 分录无任何行。
    #[error("entry {event:?} has no lines")]
    EmptyLines { event: BusinessEventId },

    /// 科目不在当前（版本化）科目表内：不得悄悄建账。
    #[error("unknown account {account} (event {event:?}); not in chart")]
    UnknownAccount {
        event: BusinessEventId,
        account: LedgerAccountId,
    },

    /// NonCash 分录含现金科目行：非现金标识与现金行互斥。
    #[error("non-cash entry {event:?} touches cash account {account}")]
    NonCashTouchesCash {
        event: BusinessEventId,
        account: LedgerAccountId,
    },

    /// 同一 `BusinessEventId` 至多入账一次：重复入账显式拒绝（K2 来源唯一性）。
    #[error("duplicate posting for event {event:?}: first posted on {first_date}")]
    DuplicatePosting {
        event: BusinessEventId,
        first_date: CivilDate,
    },

    /// 记账将把现金科目打成负数：类型化拒绝，绝不 clamp（K2 负现金禁令）。
    #[error("posting would drive cash account {account} to {projected}; negative cash prohibited")]
    NegativeCashProhibited {
        account: LedgerAccountId,
        projected: AccountingAmount,
    },

    /// 已封期间拒绝新入账（结账机制本体在任务 13；本模块只有状态 + 守卫）。
    #[error("period {period} is closed; posting for event {event:?} rejected")]
    ClosedPeriod {
        period: AccountingPeriod,
        event: BusinessEventId,
    },

    /// 重复封账。
    #[error("period {period} already closed")]
    PeriodAlreadyClosed { period: AccountingPeriod },

    /// 期间构造非法（月份 ∉ 1..=12 或年份超出公历算法窗 1900..=2199）。
    #[error("invalid accounting period {year}-{month:02}: {reason}")]
    InvalidPeriod {
        year: i32,
        month: u8,
        reason: &'static str,
    },

    /// 期间字符串解析失败（非 `YYYY-MM`）。
    #[error("period parse failed: input {input:?}: {reason}")]
    PeriodParse { input: String, reason: String },

    /// 科目表构造非法（重复科目 / 空代码 / 空名称）。
    #[error("invalid chart: {detail}")]
    ChartInvalid { detail: String },

    /// 整批原子性：验证期任一失败都中止整批，账套零改动。
    /// `failed_index = None` 表示批级裁决（现金下限），无单条罪魁。
    #[error("batch of {batch_len} entries aborted at {:?}: {cause}", failed_index)]
    BatchAborted {
        batch_len: usize,
        failed_index: Option<usize>,
        cause: Box<AccountingError>,
    },
}
