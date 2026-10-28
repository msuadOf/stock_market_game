//! 玩家提案合同；依 ADR-0037 作为持仓即可发起的游戏化简化。
//!
//! 这里只承载输入与类别，不赋予审批权限、不表示已受理，也不分配交易 PlanId。

use crate::Money;
use crate::company::CompanyId;
use crate::company::issuer_repurchase::RepurchasePurpose;
use crate::company::share_split::ShareSplitDirection;
use crate::company::stock_distribution::StockDistributionKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlayerProposalKind {
    CashDividend,
    StockDistribution,
    RightsOffering,
    SecondaryOffering,
    IssuerRepurchase,
    ShareSplit,
}

/// 账户由宿主身份绑定，输入不接受客户端指定发起账户或批准日期。
/// Money 使用分字符串；股数、整数比例和百万分比使用规范 u64 字符串。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PlayerCompanyProposal {
    CashDividend {
        company: CompanyId,
        gross_per_share: Money,
    },
    StockDistribution {
        company: CompanyId,
        kind: StockDistributionKind,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        shares_per_existing_share_micros: u64,
    },
    RightsOffering {
        company: CompanyId,
        price_per_share: Money,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        shares_per_existing_share_micros: u64,
        payment_days: u32,
    },
    /// 定向给提案玩家本人；缴款期沿已决最小合同为 1 个交易日，不设锁定期。
    SecondaryOffering {
        company: CompanyId,
        price_per_share: Money,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        shares: u64,
    },
    IssuerRepurchase {
        company: CompanyId,
        price_cap_per_share: Money,
        total_budget: Money,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        max_shares: u64,
        window_trading_days: u32,
        purpose: RepurchasePurpose,
    },
    ShareSplit {
        company: CompanyId,
        direction: ShareSplitDirection,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        ratio: u64,
    },
}

impl PlayerCompanyProposal {
    pub fn kind(&self) -> PlayerProposalKind {
        match self {
            Self::CashDividend { .. } => PlayerProposalKind::CashDividend,
            Self::StockDistribution { .. } => PlayerProposalKind::StockDistribution,
            Self::RightsOffering { .. } => PlayerProposalKind::RightsOffering,
            Self::SecondaryOffering { .. } => PlayerProposalKind::SecondaryOffering,
            Self::IssuerRepurchase { .. } => PlayerProposalKind::IssuerRepurchase,
            Self::ShareSplit { .. } => PlayerProposalKind::ShareSplit,
        }
    }

    pub fn company(&self) -> CompanyId {
        match self {
            Self::CashDividend { company, .. }
            | Self::StockDistribution { company, .. }
            | Self::RightsOffering { company, .. }
            | Self::SecondaryOffering { company, .. }
            | Self::IssuerRepurchase { company, .. }
            | Self::ShareSplit { company, .. } => *company,
        }
    }
}
