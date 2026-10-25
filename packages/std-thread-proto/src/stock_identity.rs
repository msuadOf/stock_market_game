use crate::ProtoError;
use engine::{StockCatalog, StockId, StockMeta};

// 策略使用代码序 ID；持仓掩码与交易部仍按原配置槽索引。
pub(crate) struct StockIdentity {
    ids: Vec<StockId>,
    original_indices: Vec<usize>,
}

impl StockIdentity {
    pub(crate) fn from_codes<'a>(
        codes: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, ProtoError> {
        let codes: Vec<_> = codes.into_iter().collect();
        let catalog =
            StockCatalog::from_metas(codes.iter().map(|code| StockMeta::new(*code)).collect())
                .map_err(|error| {
                    ProtoError::InvalidConfig(format!("股票目录构造失败：{error:?}"))
                })?;
        let ids: Vec<_> = codes
            .iter()
            .map(|code| catalog.id(code).expect("目录包含全部配置股票"))
            .collect();
        let mut original_indices = vec![0; ids.len()];
        for (index, id) in ids.iter().enumerate() {
            original_indices[id.0 as usize] = index;
        }
        Ok(Self {
            ids,
            original_indices,
        })
    }

    pub(crate) fn id(&self, original_index: usize) -> StockId {
        self.ids[original_index]
    }

    pub(crate) fn original_index(&self, id: StockId) -> Result<usize, ProtoError> {
        self.original_indices
            .get(id.0 as usize)
            .copied()
            .ok_or_else(|| ProtoError::InvalidConfig(format!("意图指向未知股票 ID：{}", id.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::strategy::{MarketView, PositionView, SelfView, StockView};
    use engine::Money;
    use std::collections::BTreeMap;

    #[test]
    fn unsorted_codes_use_catalog_identity_not_configuration_position() {
        let identity = StockIdentity::from_codes(["Z-stock", "A-stock", "M-stock"]).unwrap();
        assert_eq!(
            (identity.id(0), identity.id(1), identity.id(2)),
            (StockId(2), StockId(0), StockId(1))
        );
    }

    #[test]
    fn intent_identity_routes_back_to_original_stock_position() {
        let identity = StockIdentity::from_codes(["Z-stock", "A-stock", "M-stock"]).unwrap();
        assert_eq!(identity.original_index(StockId(0)).unwrap(), 1);
        assert_eq!(identity.original_index(StockId(1)).unwrap(), 2);
        assert_eq!(identity.original_index(StockId(2)).unwrap(), 0);
    }

    #[test]
    fn unknown_identity_is_rejected() {
        let identity = StockIdentity::from_codes(["600001", "000001"]).unwrap();
        assert!(matches!(
            identity.original_index(StockId(2)),
            Err(ProtoError::InvalidConfig(_))
        ));
    }

    #[test]
    fn duplicate_codes_are_rejected() {
        assert!(matches!(
            StockIdentity::from_codes(["600001", "600001"]),
            Err(ProtoError::InvalidConfig(_))
        ));
    }

    #[test]
    fn market_and_positions_share_catalog_keys_under_original_position_mask() {
        let identity = StockIdentity::from_codes(["Z-stock", "A-stock", "M-stock"]).unwrap();
        let mut stocks = BTreeMap::new();
        let mut positions = BTreeMap::new();
        for index in [0, 2] {
            let id = identity.id(index);
            stocks.insert(
                id,
                StockView {
                    best_bid: None,
                    best_ask: None,
                    last_price: Money::from_cents((index as i64 + 1) * 100),
                    max_buy_price: Money::from_cents(1_000),
                    daily_upper_limit: Money::from_cents(1_000),
                    min_sell_price: Money::from_cents(1),
                    recent_prices: vec![],
                    recent_market_minute_prices: vec![],
                    relative_volume: 1.0,
                    order_book_imbalance: 0.0,
                },
            );
            positions.insert(
                id,
                PositionView {
                    qty: (index as u32 + 1) * 100,
                    sellable_qty: 100,
                    cost_price: None,
                },
            );
        }
        let market = MarketView {
            stocks,
            tick: 1,
            market_minute: 1,
        };
        let own = SelfView {
            cash: Money::ZERO,
            positions,
        };
        assert_eq!(
            market.stocks.keys().copied().collect::<Vec<_>>(),
            vec![StockId(1), StockId(2)]
        );
        assert_eq!(market.stocks[&StockId(2)].last_price.cents(), 100);
        assert_eq!(own.positions[&StockId(2)].qty, 100);
        assert_eq!(market.stocks[&StockId(1)].last_price.cents(), 300);
        assert_eq!(own.positions[&StockId(1)].qty, 300);
        assert!(!market.stocks.contains_key(&StockId(0)));
    }
}
