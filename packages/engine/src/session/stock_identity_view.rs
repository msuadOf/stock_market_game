//! 本局实际证券目录的最小公开身份投影。

use super::GameSession;
use crate::StockId;

/// StockId 为本局目录原 key；code 仅用于边界展示。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct StockIdentityEntry {
    pub id: StockId,
    pub code: String,
}

impl GameSession {
    /// 只读身份事实，不重排、不从 setup 重新分配 StockId。
    pub fn stock_catalog_view(&self) -> Vec<StockIdentityEntry> {
        self.stock_catalog()
            .iter()
            .map(|(id, meta)| StockIdentityEntry {
                id,
                code: meta.code.clone(),
            })
            .collect()
    }
}
