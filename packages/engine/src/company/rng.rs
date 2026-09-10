//! 公司经营 RNG 分流（K4，任务 14）。
//!
//! 每公司独立经营流 + 市场/行业冲击独立流 + 初始化专用前史流：全部由
//! `seed + 流种类 + 稳定 id` 派生，**状态随存档持久化**（serde），绝不与
//! 策略/会话 RNG 共享（`crate::session::SplitMix64` 的同算法孪生副本，
//! 交叉引用注释见 `accounting::amount` 的 rhe_div 先例——session 依赖方向
//! 是 session→company，不能反向 import）。
//!
//! 派生函数：FNV-1a 哈希流标签 + 稳定 id → 与 seed 混合后过一次 SplitMix64
//! 终结器。同 seed 同 id 必得同流；不同流种类/不同 id 的流互相独立。

/// RNG 流种类（派生鉴别符；决定与哪些流隔离）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum RngStream {
    /// 单公司经营流（seed + CompanyId）。
    CompanyOperating,
    /// 市场冲击流（全局一条）。
    MarketShock,
    /// 行业冲击流（seed + IndustryId）。
    IndustryShock,
    /// 初始化专用前史流（仅前史生成期间使用；开局后切换到经营流）。
    InitHistory,
}

impl RngStream {
    fn tag(self) -> &'static str {
        match self {
            RngStream::CompanyOperating => "company-operating",
            RngStream::MarketShock => "market-shock",
            RngStream::IndustryShock => "industry-shock",
            RngStream::InitHistory => "init-history",
        }
    }
}

/// 公司域确定性 PRNG（SplitMix64，与 session.rs 同算法孪生）。状态可持久化。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct OperatingRng {
    state: u64,
}

impl OperatingRng {
    pub fn from_state(state: u64) -> Self {
        Self { state }
    }

    pub fn state(&self) -> u64 {
        self.state
    }

    /// 由 (seed, 流种类, 稳定 id) 派生：FNV-1a(标签 + id) 与 seed 混合后过
    /// SplitMix64 终结器（ avalanche）。
    pub fn derive(seed: u64, stream: RngStream, stable_id: &str) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in stream.tag().bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for byte in stable_id.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self::from_state(split_mix_finalize(seed ^ hash))
    }

    /// 标准 SplitMix64 步进（常量与 session.rs 一致，确定性依赖）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 均匀取 [0, bound)；bound = 0 → 0（调用方保证 bound > 0）。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound <= 1 {
            return 0;
        }
        self.next_u64() % bound
    }

    /// 候选概率判定（整数基点；0 = 永不、10000 = 每次必中）。
    pub fn chance_bp(&mut self, prob_bp: i32) -> bool {
        if prob_bp <= 0 {
            return false;
        }
        self.below(10_000) < u64::try_from(prob_bp).expect("prob_bp within u64")
    }

    /// 均匀取有符号闭区间 [lo, hi]（lo ≤ hi 由调用方参数校验保证）。
    pub fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        let width = u64::try_from(hi - lo + 1).expect("non-empty bounded range");
        lo + i64::try_from(self.below(width)).expect("range within i64")
    }
}

/// SplitMix64 输出终结器（派生混合用；不推进内部状态）。
fn split_mix_finalize(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
