//! Group consumer 的测试专用 Desk 桥；成功来源只由原撮合与 flush 产生。
use super::*;
use crate::runtime::RepurchaseRequestKey;

pub(crate) struct SourceDesk {
    state: DeskState,
}

impl SourceDesk {
    pub(crate) fn new(
        config: Arc<GameConfig>,
        calendar: Arc<super::super::MarketTickContext>,
    ) -> Self {
        let mut state = DeskState::new(
            DeskSetup {
                stock: 3,
                exchange: StockExchange::Shenzhen,
                initial_price: 1000,
                limit_bps: 1000,
                max_order_qty_limit: 1_000_000,
                max_order_qty_market: 150_000,
                ticks_per_day: 100,
                auction_ticks: 0,
                closing_auction_ticks: 0,
                order_ttl_ticks: 10,
                history_len: 8,
                initial_tick: 0,
                initial_orders: Vec::new(),
                initial_recent_prices: Vec::new(),
                initial_daily_bars: Vec::new(),
                initial_traded_days: 0,
                initial_market: None,
                initial_public_cutoff: 0,
                initial_next_serial: 1,
            },
            2,
            config,
        );
        state.begin_tick(1, calendar).unwrap();
        Self { state }
    }

    pub(crate) fn public(&self) -> super::super::protocol::StockPublic {
        self.state.build_public(1).unwrap()
    }

    pub(crate) fn company_buy(
        &mut self,
        request: RepurchaseRequestKey,
        price: i64,
        qty: u32,
        purse: i64,
    ) {
        self.state
            .apply_ops(
                1,
                vec![DeskIn::PlaceIssuerRepurchase {
                    owner: (1, 1),
                    request,
                    price,
                    qty,
                    purse,
                }],
            )
            .unwrap();
    }

    pub(crate) fn sell(&mut self, price: i64, qty: u32) {
        self.state
            .apply_ops(
                self.state.cur_tick,
                vec![DeskIn::Place {
                    owner: (0, 0),
                    side: Side::Sell,
                    price: RoutedPrice::Fixed(price),
                    qty,
                    purse: 0,
                    #[cfg(feature = "simulation-diagnostics")]
                    source: super::super::diagnostics::OperationSource::NonPlayerUnresolved,
                }],
            )
            .unwrap();
    }

    /// 调用原 Desk tick 入口提交前拍；后续成交仍走真实撮合与原 flush。
    pub(crate) fn advance_tick(&mut self, tick: u64, calendar: Arc<super::super::MarketTickContext>) {
        self.state.begin_tick(tick, calendar).unwrap();
    }

    pub(crate) fn cancel_company(&mut self) {
        self.state
            .apply_ops(1, vec![DeskIn::CancelAll { owner: (1, 1) }])
            .unwrap();
    }

    pub(crate) fn flush_company(&mut self) -> DeskBatch {
        self.flush_company_round(0)
    }

    pub(crate) fn flush_company_round(&mut self, round: u32) -> DeskBatch {
        let (tx0, _rx0) = std::sync::mpsc::channel();
        let (tx1, rx1) = std::sync::mpsc::channel();
        self.state.flush_batches(&[tx0, tx1], self.state.cur_tick, round).unwrap();
        let super::super::protocol::GroupIn::Batch(batch) =
            rx1.try_recv().expect("真实公司 Batch 必须已发送")
        else {
            panic!("公司来源必须沿原 Batch 交付");
        };
        assert!(rx1.try_recv().is_err(), "一次 flush 不能重复发送公司 Batch");
        batch
    }
}
