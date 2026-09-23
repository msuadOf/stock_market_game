//! 银行账套装配输入与开局守卫（K3 银行，任务 9）：配置值类型 + 构造期
//! 守卫（ECL 政策校验先行；开局行不得给银行子账科目种子——经营前史由
//! 任务 14 用同一处理器生成，不从存档倒推，诚实边界）。

use crate::accounting::reports::bank::codes;
use crate::accounting::JournalLine;
use crate::accounting::LedgerAccountId;
use crate::calendar::CivilDate;
use crate::company::bank::ecl::EclPolicy;
use crate::company::bank::BankError;
use crate::company::ExternalCounterparty;

/// 银行账套装配输入。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct BankConfig {
    pub chart: crate::accounting::AccountChart,
    pub as_of: CivilDate,
    /// 显式平衡的开局行（经 `post_batch` 验证路径，任务 7 语义）。
    /// 只允许现金 + 权益侧科目；触碰银行子账科目 → 构造期拒绝。
    pub opening_lines: Vec<JournalLine>,
    pub counterparties: Vec<ExternalCounterparty>,
    /// 版本化 ECL 政策（无默认构造——PD/LGD 属显式游戏假设，不从市场推导）。
    pub ecl_policy: EclPolicy,
}

/// 开局行子账种子守卫：触碰任何银行子账/损益科目（贷款/存款/准备/应收
/// 应付利息/利息收支/手续费/减值损失）→ `OpeningBankBooksSeeded`。
/// 现金（1003）与权益（4001/4103）是开局唯一合法落点。
pub(super) fn check_opening_lines(lines: &[JournalLine]) -> Result<(), BankError> {
    const SEEDED_ACCOUNTS: [&str; 10] = [
        codes::LOAN_INT_RCV,
        codes::LOAN_PRINCIPAL,
        codes::LOAN_ALLOWANCE,
        codes::ST_DEPOSIT,
        codes::LT_DEPOSIT,
        codes::DEP_INT_PAYABLE,
        codes::INTEREST_INCOME,
        codes::FEE_INCOME,
        codes::INTEREST_EXPENSE,
        codes::CREDIT_IMPAIR,
    ];
    for line in lines {
        if SEEDED_ACCOUNTS.contains(&line.account.0.as_str()) {
            return Err(BankError::OpeningBankBooksSeeded {
                account: LedgerAccountId(line.account.0.clone()),
            });
        }
    }
    Ok(())
}
