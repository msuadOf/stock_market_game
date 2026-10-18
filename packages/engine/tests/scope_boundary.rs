//! 负向范围与领域边界锚定（R30/R33，trading-rules.md「尚未模拟」+「快捷委托
//! 与定时自动」）：
//!
//! - **R30（定时自动，2026-10-05）**：定时自动是会话内 Web 功能，不是交易所
//!   原生委托类型，也不写入日终存档。引擎侧锚定=委托词表（`Intent`）不含
//!   任何定时/条件触发变体——存档侧因此**没有可写入的域**（SaveSlot 契约由
//!   save_contract 测试族以 deny_unknown_fields+obsolete 拒收锚定，形状演
//!   进须过该门禁）。Web 会话功能面不在 engine 范围（by design）。
//! - **R33（尚未模拟）**：科创板/北交所差异、停复牌/盘中临停不在范围——
//!   引擎侧锚定=证券类别词表（`SecurityCategory`）无科创板/北交所变体、
//!   交易相位词表（`TradingPhase`）无停牌/临停变体；融资融券/大宗交易
//!   不是委托类型（`Intent` 词表无对应变体）。
//!
//! 锚定方法：穷举 match helper（编译期强制——新增变体须修改本文件才能编译，
//! 即显式登记门）+ serde 序列化标签集断言（运行期词表快照）。

use engine::strategy::{Intent, LimitPrice};
use engine::{Money, SecurityCategory, StockId, TradingPhase};

/// `Intent` 变体名穷举（编译期门：新增变体 ⇒ 本函数非穷举 ⇒ 编译失败 ⇒
/// 须在此显式登记并对照 R30/R33 裁决是否属引擎范围）。
fn intent_variant_names(intent: &Intent) -> &'static str {
    match intent {
        Intent::PlaceLimit { .. } => "PlaceLimit",
        Intent::PlaceMarket { .. } => "PlaceMarket",
        Intent::Cancel { .. } => "Cancel",
    }
}

/// `SecurityCategory` 变体名穷举（同上编译期门）。
fn security_category_name(c: &SecurityCategory) -> &'static str {
    match c {
        SecurityCategory::MainBoard => "MainBoard",
        SecurityCategory::StMainBoard => "StMainBoard",
        SecurityCategory::ChiNext => "ChiNext",
    }
}

/// `TradingPhase` 变体名穷举（同上编译期门——无 Halted/Suspended 变体）。
fn trading_phase_name(p: &TradingPhase) -> &'static str {
    match p {
        TradingPhase::CallAuction => "CallAuction",
        TradingPhase::PreOpen => "PreOpen",
        TradingPhase::ClosingAuction => "ClosingAuction",
        TradingPhase::Continuous => "Continuous",
    }
}

/// R30/R33：委托词表=限价/市价/撤单三种，无定时委托（R30）、无融资融券/
/// 大宗交易委托（R33）。serde 标签集与穷举 helper 双重一致。
#[test]
fn intent_vocabulary_has_no_scheduled_margin_or_block_variants() {
    let intents = vec![
        Intent::PlaceLimit {
            code: StockId(0),
            side: engine::Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(1000)),
            qty: 100,
        },
        Intent::PlaceLimit {
            code: StockId(0),
            side: engine::Side::Buy,
            price: LimitPrice::Highest,
            qty: 100,
        },
        Intent::PlaceLimit {
            code: StockId(0),
            side: engine::Side::Sell,
            price: LimitPrice::Lowest,
            qty: 100,
        },
        Intent::PlaceMarket {
            code: StockId(0),
            side: engine::Side::Buy,
            qty: 100,
        },
        Intent::Cancel {
            code: StockId(0),
            id: engine::OrderId(1),
        },
    ];
    // 穷举 helper 名集（编译期门覆盖）。
    let names: std::collections::BTreeSet<&str> = intents.iter().map(intent_variant_names).collect();
    assert_eq!(
        names,
        ["Cancel", "PlaceLimit", "PlaceMarket"].into_iter().collect(),
        "委托词表=限价/市价/撤单（R30：定时自动非委托类型；R33：两融/大宗非委托类型）"
    );
    // serde 标签快照（运行期词表与序列化契约一致——Web/存档契约面）。
    let mut tags = std::collections::BTreeSet::new();
    for i in &intents {
        let v = serde_json::to_value(i).expect("Intent 可序列化");
        let tag = v
            .as_object()
            .and_then(|m| m.keys().next().cloned())
            .expect("外部标签枚举形态");
        tags.insert(tag.to_string());
    }
    assert_eq!(
        tags,
        ["Cancel", "PlaceLimit", "PlaceMarket"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        "serde 标签集与词表一致（新增变体=契约面显式变更）"
    );
}

/// R33：证券类别=沪深主板/风险警示主板/创业板，无科创板/北交所变体。
#[test]
fn security_category_vocabulary_excludes_star_and_bse() {
    let categories = [
        SecurityCategory::MainBoard,
        SecurityCategory::StMainBoard,
        SecurityCategory::ChiNext,
    ];
    let names: std::collections::BTreeSet<&str> =
        categories.iter().map(security_category_name).collect();
    assert_eq!(
        names,
        ["ChiNext", "MainBoard", "StMainBoard"].into_iter().collect(),
        "类别词表无科创板/北交所（R33 尚未模拟——新增须显式登记涨跌幅/单笔上限等全套规则）"
    );
}

/// R33：交易相位=开盘竞价/盘前/连续/收盘竞价，无停牌/盘中临停相位。
#[test]
fn trading_phase_vocabulary_excludes_halt_and_suspension() {
    let phases = [
        TradingPhase::CallAuction,
        TradingPhase::PreOpen,
        TradingPhase::ClosingAuction,
        TradingPhase::Continuous,
    ];
    let names: std::collections::BTreeSet<&str> = phases.iter().map(trading_phase_name).collect();
    assert_eq!(
        names,
        ["CallAuction", "ClosingAuction", "Continuous", "PreOpen"]
            .into_iter()
            .collect(),
        "相位词表无停复牌/盘中临停（R33 尚未模拟）"
    );
}
