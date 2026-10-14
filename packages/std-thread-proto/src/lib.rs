//! std::thread 并发模型原型（用户设计）：一股一持久 StockWorker 线程，
//! 每 tick worker 内 spawn NPC 决策线程 → join → 本股撮合 → 结果回传
//! coordinator 单线程收束安装。纯 Rust 标准库（std::thread + std::sync）。
//!
//! 设计文档：`.tmp/std-thread-proto/design.md`。
//! 决策逻辑直接复用 engine crate 的 `strategy::decide_data` 纯函数；
//! 撮合为本原型实现的等价负载（价格优先/同价 FIFO/涨跌停带/T+1/费用），
//! 结算为简化版（不含 engine 的 SealAllocation/receipt/ledger/守恒证据链）。

pub mod book;
pub mod npc;
pub mod run;
pub mod setup;
pub mod worker;

pub use run::{run_benchmark, AuditReport, RunMode, RunReport, StockAudit};
pub use setup::{ProtoConfig, StockSpec};

/// 原型错误。绝不静默吞（铁律二）：所有失败显式上抛给调用方。
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("money 定点运算失败：{0}")]
    Money(String),
    #[error("现金守恒破坏：initial={initial} final={final_}")]
    CashConservation { initial: i64, final_: i64 },
    #[error("股票 {code} 守恒破坏：held={held} remainder={remainder} total={total}")]
    ShareConservation {
        code: String,
        held: u64,
        remainder: u64,
        total: u64,
    },
    #[error("股票 {code} 持仓 T+1 结构破坏：qty={qty} sellable+reserved+frozen={parts}")]
    T1Structure {
        code: String,
        qty: u32,
        parts: u32,
    },
    #[error("股票 {code} 账户 {npc} 现金为负：{cash}")]
    NegativeCash {
        code: String,
        npc: u32,
        cash: i64,
    },
    #[error("整数溢出：{op} {operand}")]
    Overflow { op: &'static str, operand: String },
    #[error("非法配置：{0}")]
    InvalidConfig(String),
    #[error("worker 通道关闭：{0}")]
    ChannelClosed(String),
}
