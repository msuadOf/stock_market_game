//! 个体混合分析能力档案（K5）：账户身份与主导风格之外的五维分析权重 + 基本面方法链接。
//!
//! 权重以基点（bp）表示，五个非负权重总和恰为 10000；零权重表示该分析维度对本
//! 实例彻底关闭（不运行、无方法）。银行/保险行业的基本面方法按规则恒为权益 ROE
//! 法，不逐实例抽样、不存入档案（见 [`AnalysisProfile::method_for_company_kind`]）；
//! 档案保存的方法槽位只承载工商/地产的盈利倍数/现金流选择。

use crate::company::CompanyKind;

/// 基本面估值方法路径（K5a：三条实质不同的路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundamentalMethod {
    /// 可持续盈利倍数法（工商/地产 DeepValue/Defensive；奇偶选项之一）。
    EarningsMultiple,
    /// 个人现金流折现法（Growth；奇偶选项之一）。
    CashFlow,
    /// 账面权益 × 可持续 ROE / 资本成本（仅银行/保险，按规则固定）。
    EquityRoe,
}

/// 分析档案构造/恢复失败。绝不静默吞掉（铁律二）：非法权重/方法映射一律类型化拒绝。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AnalysisProfileError {
    #[error("negative analysis weight {field}: {value} bp")]
    NegativeWeight { field: &'static str, value: i64 },
    #[error("all analysis weights are zero")]
    AllZeroWeights,
    #[error("analysis weights must sum to 10000 bp, got {actual}")]
    WeightSumMismatch { actual: i64 },
    #[error(
        "fundamental method {method:?} is illegal in the profile slot (equity ROE is bank/insurance-only by rule)"
    )]
    IllegalMethodKindMapping { method: FundamentalMethod },
    #[error("fundamental method {method:?} conflicts with zero fundamental weight")]
    MethodWithoutFundamentalWeight { method: FundamentalMethod },
    #[error("nonzero fundamental weight requires a fundamental method")]
    FundamentalWeightWithoutMethod,
    #[error("unknown fundamental method id: {id}")]
    UnknownFundamentalMethod { id: String },
    #[error("normalization input must be nonnegative with positive total, got total {total}")]
    InvalidNormalizationInput { total: i64 },
}

/// 五维分析权重（bp）：基本面/趋势/量价/技术/成本经历。
///
/// 声明顺序即最大余数法归一的破同分顺序（见 `factory_profiles`）。合法实例只能经
/// [`AnalysisWeights::new`] 或恢复边界构造：五个非负、总和恰为 [`AnalysisWeights::TOTAL_BP`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisWeights {
    fundamental_bp: u32,
    trend_bp: u32,
    price_volume_bp: u32,
    technical_bp: u32,
    experience_cost_bp: u32,
}

impl AnalysisWeights {
    /// 合法权重总和（1bp = 0.01%）。
    pub const TOTAL_BP: u32 = 10_000;

    /// 边界构造：输入取 i64（负值可表示），负权重/全零/总和不符全部类型化拒绝。
    pub fn new(
        fundamental_bp: i64,
        trend_bp: i64,
        price_volume_bp: i64,
        technical_bp: i64,
        experience_cost_bp: i64,
    ) -> Result<Self, AnalysisProfileError> {
        let fields = [
            ("fundamental_bp", fundamental_bp),
            ("trend_bp", trend_bp),
            ("price_volume_bp", price_volume_bp),
            ("technical_bp", technical_bp),
            ("experience_cost_bp", experience_cost_bp),
        ];
        if let Some(&(field, value)) = fields.iter().find(|&(_, value)| *value < 0) {
            return Err(AnalysisProfileError::NegativeWeight { field, value });
        }
        let total: i64 = fields.iter().map(|&(_, value)| value).sum();
        if total == 0 {
            return Err(AnalysisProfileError::AllZeroWeights);
        }
        if total != i64::from(Self::TOTAL_BP) {
            return Err(AnalysisProfileError::WeightSumMismatch { actual: total });
        }
        let [fundamental, trend, price_volume, technical, experience_cost] =
            fields.map(|(_, value)| value as u32);
        Ok(Self {
            fundamental_bp: fundamental,
            trend_bp: trend,
            price_volume_bp: price_volume,
            technical_bp: technical,
            experience_cost_bp: experience_cost,
        })
    }

    pub fn fundamental_bp(&self) -> u32 {
        self.fundamental_bp
    }

    pub fn trend_bp(&self) -> u32 {
        self.trend_bp
    }

    pub fn price_volume_bp(&self) -> u32 {
        self.price_volume_bp
    }

    pub fn technical_bp(&self) -> u32 {
        self.technical_bp
    }

    pub fn experience_cost_bp(&self) -> u32 {
        self.experience_cost_bp
    }

    pub fn total_bp(&self) -> u32 {
        self.fundamental_bp
            + self.trend_bp
            + self.price_volume_bp
            + self.technical_bp
            + self.experience_cost_bp
    }

    /// 声明顺序数组（与归一破同分顺序一致）。
    pub fn as_array(&self) -> [u32; 5] {
        [
            self.fundamental_bp,
            self.trend_bp,
            self.price_volume_bp,
            self.technical_bp,
            self.experience_cost_bp,
        ]
    }
}

/// 个体混合分析能力档案：权重 + 工商/地产的基本面方法。
///
/// 档案由 `factory_profiles` 在新局按（身份档案, 稳定账户 id, profile RNG）确定性
/// 派生一次；主导风格是输入而非每次抽样。serde 经 [`PersistedAnalysisProfile`]
/// 走恢复边界，非法/冲突数据显式拒绝。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    into = "PersistedAnalysisProfile",
    try_from = "PersistedAnalysisProfile"
)]
pub struct AnalysisProfile {
    weights: AnalysisWeights,
    fundamental_method: Option<FundamentalMethod>,
}

impl AnalysisProfile {
    /// 构造并验证方法链接：
    /// - 方法为 `Some` ⇔ 基本面权重 > 0（零权重不运行基本面分析，不给方法）；
    /// - 方法槽位只接受盈利倍数/现金流；权益 ROE 属银行/保险规则，存入即非法映射。
    pub fn new(
        weights: AnalysisWeights,
        fundamental_method: Option<FundamentalMethod>,
    ) -> Result<Self, AnalysisProfileError> {
        match fundamental_method {
            Some(method @ FundamentalMethod::EquityRoe) => {
                Err(AnalysisProfileError::IllegalMethodKindMapping { method })
            }
            Some(method) if weights.fundamental_bp() == 0 => {
                Err(AnalysisProfileError::MethodWithoutFundamentalWeight { method })
            }
            None if weights.fundamental_bp() > 0 => {
                Err(AnalysisProfileError::FundamentalWeightWithoutMethod)
            }
            method => Ok(Self {
                weights,
                fundamental_method: method,
            }),
        }
    }

    /// 行业 → 基本面方法链接：零基本权重恒 `None`（不因行业身份获得能力）；
    /// 银行/保险恒 [`FundamentalMethod::EquityRoe`]（规则，非抽样）；
    /// 工商/地产返回档案保存的方法。
    pub fn method_for_company_kind(&self, kind: CompanyKind) -> Option<FundamentalMethod> {
        if self.weights.fundamental_bp == 0 {
            return None;
        }
        match kind {
            CompanyKind::Bank | CompanyKind::Insurance => Some(FundamentalMethod::EquityRoe),
            CompanyKind::Industrial | CompanyKind::RealEstate => self.fundamental_method,
        }
    }

    pub fn weights(&self) -> AnalysisWeights {
        self.weights
    }

    /// 工商/地产方法槽位（`None` ⇔ 零基本权重）。
    pub fn fundamental_method(&self) -> Option<FundamentalMethod> {
        self.fundamental_method
    }

    /// 存档恢复入口：一切校验集中在此边界，与配置冲突显式拒绝。
    pub fn from_persisted(
        persisted: PersistedAnalysisProfile,
    ) -> Result<Self, AnalysisProfileError> {
        Self::try_from(persisted)
    }

    pub fn to_persisted(&self) -> PersistedAnalysisProfile {
        (*self).clone().into()
    }
}

/// 存档/恢复 DTO：宽松类型（i64 权重 + 字符串方法 id），校验集中在恢复边界。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedAnalysisProfile {
    pub fundamental_bp: i64,
    pub trend_bp: i64,
    pub price_volume_bp: i64,
    pub technical_bp: i64,
    pub experience_cost_bp: i64,
    /// `earnings_multiple` | `cash_flow` | `equity_roe`；`None` 表示零基本权重。
    pub fundamental_method: Option<String>,
}

impl From<AnalysisProfile> for PersistedAnalysisProfile {
    fn from(profile: AnalysisProfile) -> Self {
        Self {
            fundamental_bp: i64::from(profile.weights.fundamental_bp),
            trend_bp: i64::from(profile.weights.trend_bp),
            price_volume_bp: i64::from(profile.weights.price_volume_bp),
            technical_bp: i64::from(profile.weights.technical_bp),
            experience_cost_bp: i64::from(profile.weights.experience_cost_bp),
            fundamental_method: profile
                .fundamental_method
                .map(method_id)
                .map(str::to_string),
        }
    }
}

impl TryFrom<PersistedAnalysisProfile> for AnalysisProfile {
    type Error = AnalysisProfileError;

    fn try_from(persisted: PersistedAnalysisProfile) -> Result<Self, Self::Error> {
        let method = persisted
            .fundamental_method
            .as_deref()
            .map(parse_method_id)
            .transpose()?;
        let weights = AnalysisWeights::new(
            persisted.fundamental_bp,
            persisted.trend_bp,
            persisted.price_volume_bp,
            persisted.technical_bp,
            persisted.experience_cost_bp,
        )?;
        AnalysisProfile::new(weights, method)
    }
}

fn method_id(method: FundamentalMethod) -> &'static str {
    match method {
        FundamentalMethod::EarningsMultiple => "earnings_multiple",
        FundamentalMethod::CashFlow => "cash_flow",
        FundamentalMethod::EquityRoe => "equity_roe",
    }
}

fn parse_method_id(id: &str) -> Result<FundamentalMethod, AnalysisProfileError> {
    match id {
        "earnings_multiple" => Ok(FundamentalMethod::EarningsMultiple),
        "cash_flow" => Ok(FundamentalMethod::CashFlow),
        "equity_roe" => Ok(FundamentalMethod::EquityRoe),
        other => Err(AnalysisProfileError::UnknownFundamentalMethod {
            id: other.to_string(),
        }),
    }
}
