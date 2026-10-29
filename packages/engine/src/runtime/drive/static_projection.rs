//! 静点完整读面只持短命值；不消费 sparse、事件、同步序号或待安装表。
use super::*;

pub(super) struct InitialStaticSeed {
    pub revision: u64,
    pub market_views: Vec<(i64, u64)>,
}

#[derive(Clone)]
pub(crate) struct PreparedStaticRead {
    pub source: crate::runtime::source::WorldMirrorSource,
    pub session_revision: u64,
    pub attention: Vec<(AccountId, u64, u64)>,
    pub resting: Vec<std::sync::Arc<Vec<super::super::protocol::RestingFact>>>,
    pub serials: Vec<u64>,
    pub market_views: Vec<(i64, u64)>,
    pub refresh_market: bool,
}

impl RuntimeDriver {
    /// 本片不重编号旧簿身份；不能无损迁入 desk 域时，在 spawn 前保旧权威显式拒绝。
    pub(crate) fn validate_static_adoption_identity(session: &GameSession) -> Result<(), String> {
        for (stock, orders) in session.runtime_resting_orders_snapshot() {
            let cutoff = session.runtime_public_cutoff_for(&stock);
            let mut prior = std::collections::BTreeMap::new();
            for order in orders {
                let serial = super::super::group::desk_order_serial(order.id.0);
                let expected =
                    crate::OrderId(super::super::group::desk_order_id(stock.0 as _, serial));
                if order.id != expected {
                    return Err(format!("初始静点接管不能保真重编号：stock={stock:?} order={:?} expected={expected:?}", order.id));
                }
                // 同价 FIFO 在现场及 save/restore 后都必须保持；当前持久化 seq 使用 serial。
                let key = (
                    u8::from(matches!(order.side, crate::Side::Sell)),
                    order.price.cents(),
                );
                if prior
                    .insert(key, serial)
                    .is_some_and(|previous| previous >= serial)
                {
                    return Err(format!(
                        "初始静点接管不能保真同价 FIFO：stock={stock:?} order={:?}",
                        order.id
                    ));
                }
                if cutoff > 0 && (order.seq < cutoff) != (serial < cutoff) {
                    return Err(format!("初始静点接管不能保真公开截点：stock={stock:?} order={:?} seq={} serial={serial} cutoff={cutoff}", order.id, order.seq));
                }
            }
        }
        session.runtime_legacy_order_id_floor()?;
        Ok(())
    }

    pub(crate) fn prepare_static_driver_projection(
        &mut self,
        session: &GameSession,
    ) -> Result<PreparedDriverStep, String> {
        self.prepare_static_projection_inner(session, true)
    }

    pub(crate) fn prepare_flush_driver_projection(
        &mut self,
        session: &GameSession,
    ) -> Result<PreparedDriverStep, String> {
        self.prepare_static_projection_inner(session, false)
    }

    fn prepare_static_projection_inner(
        &mut self,
        session: &GameSession,
        initial_adoption: bool,
    ) -> Result<PreparedDriverStep, String> {
        let (calendar_day, _) = session
            .prepare_market_calendar_projection()
            .map_err(|error| error.to_string())?;
        let source = self.mirror_source();
        // generation 也会因闭市 retained 重发布前进，不能用它证明 desk 已提交真实行情。
        let has_committed_market_tick = !self.world.report_log.is_empty();
        let (market_views, session_revision) = if initial_adoption {
            let initial = self
                .initial_static_seed
                .as_ref()
                .ok_or_else(|| "初始静点种子已随安装退役".to_owned())?;
            if initial.revision != session.external_revision() || source.generation != 0 {
                return Err("初始静点种子与当前 Session 或 World 来源不一致".to_owned());
            }
            (initial.market_views.clone(), initial.revision)
        } else {
            if source.civil_date != session.civil_date() {
                return Err("flush 静点 World 与 Session civil date 不一致".to_owned());
            }
            let views = if !has_committed_market_tick {
                self.initial_static_seed
                    .as_ref()
                    .ok_or_else(|| "flush 初始发布缺原始行情种子".to_owned())?
                    .market_views
                    .clone()
            } else {
                self.world_public_views()
                    .iter()
                    .map(|view| (view.last_price, view.public_cutoff_serial))
                    .collect()
            };
            (views, session.external_revision())
        };
        if source.tick != session.tick() {
            return Err("静点 Full 不能替代尚未交付的市场 tick".to_owned());
        }
        if !self.pending_retail.is_empty()
            || !self.pending_retail_rejections.is_empty()
            || !self.world.drained_retail.is_empty()
            || !self.drained_events.is_empty()
        {
            return Err("静点 Full 不能消费尚未交付的业务事实".to_owned());
        }
        let full_accounts = initial_adoption
            || self.force_full_accounts
            || self.sync_inputs.account_source()
                == crate::runtime::driver_sync::AccountSource::Full
            || self.pending_mirror_domains().accounts_full;
        let accounts = if full_accounts {
            let groups = self
                .world
                .snapshot_accounts()
                .map_err(|error| error.to_string())?;
            if groups.len() != self.world.n_groups()
                || groups.iter().enumerate().any(|(index, (group, rows))| {
                    usize::from(*group) != index || rows.len() != self.accounts_per_group
                })
            {
                return Err("静点 Full 账户组集合或名册不完整".to_owned());
            }
            groups
                .into_iter()
                .map(|(_, rows)| {
                    Arc::new(
                        rows.into_iter()
                            .enumerate()
                            .map(|(local, value)| (local as _, Arc::new(value)))
                            .collect(),
                    )
                })
                .collect()
        } else {
            (0..self.world.n_groups())
                .map(|group| {
                    Arc::new(
                        self.sync_inputs
                            .accounts()
                            .iter()
                            .filter(|(owner, _)| usize::from(owner.group) == group)
                            .map(|(owner, row)| (owner.local, Arc::clone(&row.value)))
                            .collect(),
                    )
                })
                .collect()
        };
        let full = self
            .world
            .collect_personal_full_for_prepared()
            .map_err(|error| error.to_string())?;
        if full.source != source {
            return Err("静点 Full 个人态来源改变".to_owned());
        }
        let personals = full
            .groups
            .into_iter()
            .flat_map(|(group, rows)| {
                let per = self.accounts_per_group;
                rows.into_iter().filter_map(move |(local, id, value)| {
                    value.map(|value| (usize::from(group) * per + local as usize, id, value))
                })
            })
            .collect();
        let plans = super::super::plan_ownership::merge(
            self.world
                .collect_plans()
                .map_err(|error| error.to_string())?,
            &self.id_map,
            self.world.n_groups(),
            self.accounts_per_group,
            self.plan_policy,
        )
        .map_err(|error| error.to_string())?;
        let mut attention = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (group, rows) in self
            .world
            .collect_group_attention()
            .map_err(|error| error.to_string())?
        {
            for (local, due, seed) in rows {
                let id = self
                    .account_by_owner((group, local))
                    .filter(|_| (local as usize) < self.accounts_per_group)
                    .ok_or_else(|| format!("静点 Full 调度属主不在真实名册：{group}/{local}"))?;
                if !seen.insert(id) {
                    return Err(format!("静点 Full 调度属主重复：{id:?}"));
                }
                attention.push((id, due, seed));
            }
        }
        if seen.len() != self.id_map.len() {
            return Err("静点 Full 调度缺少真实名册成员".to_owned());
        }
        // 直接读取 World 回执，失败前不改 driver 游标缓存。
        let (resting, serials) = self
            .world
            .collect_desk_resting()
            .map_err(|error| error.to_string())?;
        if resting.len() != session.stock_count_for_runtime() || serials.len() != resting.len() {
            return Err("静点 Full 在途簿或 serial 维度错误".to_owned());
        }
        if self.mirror_source() != source {
            return Err("静点 Full 读取期间 World 来源改变".to_owned());
        }
        Ok(PreparedDriverStep {
            calendar: PreparedDriverCalendar::Static(calendar_day),
            accounts,
            buy_vwap: Default::default(),
            last_prices: self
                .world_public_views()
                .iter()
                .map(|view| view.last_price)
                .collect(),
            personals,
            plans,
            touched: Vec::new(),
            events: Vec::new(),
            day_end: None,
            emit_price_tick: false,
            static_read: Some(PreparedStaticRead {
                source,
                session_revision,
                attention,
                resting: resting.into_iter().map(std::sync::Arc::new).collect(),
                serials,
                market_views,
                refresh_market: !initial_adoption && has_committed_market_tick,
            }),
        })
    }

    pub(crate) fn stop_prepared_without_drain(self) -> Result<(), String> {
        self.world
            .stop_without_drain()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}
