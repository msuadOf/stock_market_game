use super::{Event, GameSession, SaveSlot};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, thiserror::Error)]
pub enum StepFatal {
    #[error("invariant violation at {location}: {description}")]
    InvariantViolation {
        description: String,
        location: String,
    },
}

impl GameSession {
    pub fn publication_ids(&self) -> Vec<crate::information::PublicationId> {
        self.state.library.publication_ids()
    }
    pub const fn poison_reason(&self) -> Option<&StepFatal> {
        self.poison.as_ref()
    }

    pub(super) fn require_healthy(&self) -> Result<(), StepFatal> {
        match &self.poison {
            Some(fatal) => Err(fatal.clone()),
            None => Ok(()),
        }
    }

    /// 所有市场工作运行在私有 tick shadow；失败候选被丢弃。
    /// 权威状态只接收最终不会失败的 CommitTick 状态交换。
    pub fn step(&mut self) -> Result<Vec<Event>, StepFatal> {
        self.step_inner(false).map(|committed| committed.events)
    }

    /// 执行与 [`Self::step`] 相同的生产权威路径，并返回
    /// 保留成功提交前一刻捕获的不可变执行事实。
    ///
    /// 此验证入口只用于观察；证据不会留在 session、序列化、参与 hash 或后续决策。
    pub fn step_with_commit_evidence(
        &mut self,
    ) -> Result<(Vec<Event>, super::pipeline::TickCommitEvidence), StepFatal> {
        let committed = self.step_inner(true)?;
        Ok((
            committed.events,
            committed
                .evidence
                .expect("commit evidence was requested at the authoritative entry"),
        ))
    }

    pub(super) fn step_inner(
        &mut self,
        capture_commit_evidence: bool,
    ) -> Result<super::pipeline::AuthoritativeTickCommit, StepFatal> {
        self.require_healthy()?;
        if self.civil_clock().phase() == super::CivilPhase::ClosedDay {
            return Err(self.poison_failed_step(StepFatal::InvariantViolation {
                location: "GameSession::step".into(),
                description: "全部交易所休市的自然日不能推进市场tick，请执行自然日日结".into(),
            }));
        }
        #[cfg(test)]
        if self.injected_failure.is_some() {
            if let Err(fatal) = self.run_pre_mutation_hook() {
                return Err(self.poison_failed_step(fatal));
            }
        }
        if self.state.accounts.values().any(|account| {
            account.kind() != crate::AccountKind::Player
                && account.kind() != crate::AccountKind::IssuerRepurchase
                && account.strategy().is_none()
        }) {
            let fatal = StepFatal::InvariantViolation {
                description: "non-player account has no authoritative strategy".to_owned(),
                location: "GameSession::step".to_owned(),
            };
            return Err(self.poison_failed_step(fatal));
        }
        // 回购执行器：窗口内连续竞价阶段以真实委托进入既有订单簿（每日至多一单）。
        // 发行人回购账户不是策略主体，其委托由获批方案确定性产生。
        if self.state.setup.issuer_repurchase_enabled {
            self.place_issuer_repurchase_orders()?;
        }
        // 每个生产交易阶段均进入同一 Escrow 完整 tick 事务。
        // 只有完成不会失败的权威状态交换后才交付结果。
        let result = super::pipeline::execute_authoritative_tick(self, capture_commit_evidence);
        match result {
            Ok(committed) => Ok(committed),
            Err(fatal) => Err(self.poison_failed_step(fatal)),
        }
    }

    pub(super) fn prepare_ex_references_for_current_date(&mut self) -> Result<(), StepFatal> {
        let date = self.civil_date();
        let calendar = self.state.civil_clock.calendar().clone();
        #[derive(Default)]
        struct CombinedExEvent {
            exchange: Option<crate::calendar::CalendarExchange>,
            registered_on: Option<crate::calendar::CivilDate>,
            gross_per_share: crate::money::Money,
            ratio_micros: u64,
            cash_plan_ids: Vec<String>,
            stock_event_ids: Vec<String>,
            rights_event_ids: Vec<String>,
            rights_price_per_share: Option<crate::money::Money>,
            rights_ratio_micros: u64,
        }
        let mut combined = std::collections::BTreeMap::<crate::account::StockCode, CombinedExEvent>::new();
        for book in &self.state.corporate_actions.dividends {
            let plan = book.plan();
            if plan.ex_dividend_on != date || book.registration().is_none() {
                continue;
            }
            if plan.formula != crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly {
                return Err(StepFatal::InvariantViolation {
                    description: format!("证券 {} 的除息调整公式不受支持", plan.stock.0),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                });
            }
            let entry = combined.entry(plan.stock.clone()).or_default();
            if entry.exchange.is_some_and(|known| known != plan.exchange)
                || entry.registered_on.is_some_and(|known| known != plan.registered_on)
            {
                return Err(StepFatal::InvariantViolation {
                    description: format!("证券 {} 的同日除权除息事件交易所或登记日不一致", plan.stock.0),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                });
            }
            entry.exchange = Some(plan.exchange);
            entry.registered_on = Some(plan.registered_on);
            entry.gross_per_share = entry.gross_per_share.add(plan.gross_per_share).map_err(|error| StepFatal::InvariantViolation {
                description: error.to_string(),
                location: "GameSession::prepare_ex_references_for_current_date".into(),
            })?;
            entry.cash_plan_ids.push(plan.plan_id.clone());
        }
        for book in &self.state.corporate_actions.stock_distributions {
            let plan = book.plan();
            if plan.ex_rights_on != date || book.registration().is_none() {
                continue;
            }
            let entry = combined.entry(plan.stock.clone()).or_default();
            if entry.exchange.is_some_and(|known| known != plan.exchange)
                || entry.registered_on.is_some_and(|known| known != plan.registered_on)
            {
                return Err(StepFatal::InvariantViolation {
                    description: format!("证券 {} 的同日除权除息事件交易所或登记日不一致", plan.stock.0),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                });
            }
            // 交易所公式对每次除权只有一个变动比例；同日多起送转事件的合并口径
            // （比例相加还是复合相乘）未在官方材料中核实，显式拒绝而不近似。
            if !entry.stock_event_ids.is_empty() {
                return Err(StepFatal::InvariantViolation {
                    description: format!(
                        "证券 {} 的同日多起送转事件合并除权口径未核实，显式拒绝",
                        plan.stock.0
                    ),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                });
            }
            entry.exchange = Some(plan.exchange);
            entry.registered_on = Some(plan.registered_on);
            entry.ratio_micros = plan.shares_per_existing_share_micros;
            entry.stock_event_ids.push(plan.event_id.clone());
        }
        for book in &self.state.corporate_actions.rights_offerings {
            let plan = book.plan();
            if plan.ex_rights_on != date || book.registration().is_none() {
                continue;
            }
            // 配股除权须已关窗（实际认购在 L 日确定；沪 4.3.1 公式的变动比例是
            // 实际比例）。未关窗即到除权日属于日程错误，显式失败。
            if book.status()
                == &crate::company::rights_offering::RightsOfferingStatus::Entitled
            {
                return Err(StepFatal::InvariantViolation {
                    description: format!(
                        "配股事件 {} 除权日 {} 前未关缴款窗口",
                        plan.event_id, date
                    ),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                });
            }
            // 实际认购为零：股份未变动，不产生除权锚（发行失败事实由结算回执
            // 承载）；跳过而不套用除权公式，也不向组合事件插入任何分量。
            let paid = book
                .subscriptions()
                .iter()
                .map(|record| record.paid_shares)
                .sum::<u64>();
            let issued_before = book
                .entitlement()
                .map(|receipt| receipt.issued_shares_before)
                .unwrap_or(0);
            if issued_before == 0 {
                return Err(StepFatal::InvariantViolation {
                    description: format!("配股事件 {} 缺少配股前股数事实", plan.event_id),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                });
            }
            let ratio = u64::try_from((u128::from(paid) * 1_000_000_u128) / u128::from(issued_before))
                .map_err(|_| StepFatal::InvariantViolation {
                    description: format!("配股事件 {} 的实际比例溢出", plan.event_id),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                })?;
            // 整数截位比例为零的微量认购不构成除权组分量：与恢复勾稽共用同一
            // 权威谓词（`forms_ex_rights_component`），两侧不得各自表述。
            if !crate::company::rights_offering::forms_ex_rights_component(paid, issued_before) {
                continue;
            }
            let entry = combined.entry(plan.stock.clone()).or_default();
            if entry.exchange.is_some_and(|known| known != plan.exchange) {
                return Err(StepFatal::InvariantViolation {
                    description: format!("证券 {} 的同日除权除息事件交易所不一致", plan.stock.0),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                });
            }
            // 同日多起配股、或配股与送转同日的合并除权口径（比例相加还是复合）
            // 未在官方材料核实：显式拒绝，不近似。
            if !entry.rights_event_ids.is_empty() || !entry.stock_event_ids.is_empty() {
                return Err(StepFatal::InvariantViolation {
                    description: format!(
                        "证券 {} 的同日多起配股或配股×送转合并除权口径未核实，显式拒绝",
                        plan.stock.0
                    ),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                });
            }
            entry.exchange = Some(plan.exchange);
            entry.rights_price_per_share = Some(plan.price_per_share);
            // 实际配股比例 = 实际认购新增股份 ÷ 配股前股份（百万分之一股，整数截位）。
            entry.rights_ratio_micros = ratio;
            entry.rights_event_ids.push(plan.event_id.clone());
        }
        if combined.is_empty() {
            return Ok(());
        }
        let mut candidate_markets = self.state.markets.clone();
        let mut applied_groups = self.state.corporate_actions.applied_ex_reference_groups.clone();
        for (stock, mut event) in combined {
            let Some(exchange) = event.exchange else {
                return Err(StepFatal::InvariantViolation {
                    description: format!("证券 {} 的除权除息组缺少交易所事实", stock.0),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                });
            };
            // 配股事件的登记日事实可选：其除权锚定缴款截止日 L（见下方公式分支）；
            // 纯现金与送转事件仍要求登记日事实。
            let registered_on = event.registered_on.unwrap_or(date);
            event.cash_plan_ids.sort();
            event.stock_event_ids.sort();
            let market = candidate_markets.get_mut(&stock).ok_or_else(|| StepFatal::InvariantViolation {
                description: format!("除权除息计划引用未知证券 {}", stock.0),
                location: "GameSession::prepare_ex_references_for_current_date".into(),
            })?;
            if let Some(group) = applied_groups.iter().find(|group| group.date == date && group.stock == stock) {
                if group.cash_plan_ids != event.cash_plan_ids
                    || group.stock_event_ids != event.stock_event_ids
                    || group.rights_event_ids != event.rights_event_ids
                    || market.last_cash_ex_reference() != Some(group.reference)
                {
                    return Err(StepFatal::InvariantViolation {
                        description: format!("证券 {} 的已应用除权除息事实与当前登记计划不一致", stock.0),
                        location: "GameSession::prepare_ex_references_for_current_date".into(),
                    });
                }
                continue;
            }
            if market.last_cash_ex_reference().is_some_and(|reference| reference.ex_date == date) {
                return Err(StepFatal::InvariantViolation {
                    description: format!("证券 {} 有无对应计划组的已应用除权除息参考价", stock.0),
                    location: "GameSession::prepare_ex_references_for_current_date".into(),
                });
            }
            let reference = if event.rights_ratio_micros > 0 {
                // 配股除权：缴款截止日 L 的次一交易日；公式含配股价分量与可选
                // 同日现金红利（沪 4.3.1／深 4.4.1 公式形态）。
                let rights_price = event.rights_price_per_share.expect(
                    "rights ratio is set together with the rights price",
                );
                let payment_deadline = calendar
                    .previous_trading_day(exchange, date)
                    .map_err(|error| StepFatal::InvariantViolation {
                        description: error.to_string(),
                        location: "GameSession::prepare_ex_references_for_current_date".into(),
                    })?;
                let formula = match exchange {
                    crate::calendar::CalendarExchange::Sse => crate::company::ex_reference_price::RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange {
                        ratio_micros: event.rights_ratio_micros,
                    },
                    crate::calendar::CalendarExchange::Szse => crate::company::ex_reference_price::RightsOfferingExRightsFormula::ShenzhenRightsShareChange {
                        ratio_micros: event.rights_ratio_micros,
                    },
                };
                crate::company::ex_reference_price::rights_offering_ex_rights_reference_price(
                    &calendar,
                    exchange,
                    formula,
                    payment_deadline,
                    market.last_close(),
                    event.gross_per_share,
                    rights_price,
                )
            } else if event.ratio_micros == 0 {
                crate::company::ex_reference_price::cash_dividend_ex_reference_price(
                    &calendar,
                    exchange,
                    crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
                    registered_on,
                    market.last_close(),
                    event.gross_per_share,
                )
            } else {
                let formula = match exchange {
                    crate::calendar::CalendarExchange::Sse => crate::company::ex_reference_price::StockDistributionExRightsFormula::ShanghaiCirculatingShareChange {
                        ratio_micros: event.ratio_micros,
                    },
                    crate::calendar::CalendarExchange::Szse => crate::company::ex_reference_price::StockDistributionExRightsFormula::ShenzhenShareChange {
                        ratio_micros: event.ratio_micros,
                    },
                };
                crate::company::ex_reference_price::stock_distribution_ex_rights_reference_price(
                    &calendar,
                    exchange,
                    formula,
                    registered_on,
                    market.last_close(),
                    event.gross_per_share,
                )
            }.map_err(|error| StepFatal::InvariantViolation {
                description: error.to_string(),
                location: "GameSession::prepare_ex_references_for_current_date".into(),
            })?;
            market.prepare_ex_date_reference(date, reference).map_err(|error| StepFatal::InvariantViolation {
                description: error.to_string(),
                location: "GameSession::prepare_ex_references_for_current_date".into(),
            })?;
            applied_groups.push(super::AppliedExReferenceGroup {
                date,
                stock,
                cash_plan_ids: event.cash_plan_ids,
                stock_event_ids: event.stock_event_ids,
                rights_event_ids: event.rights_event_ids,
                reference,
            });
        }
        self.state.markets = candidate_markets;
        applied_groups.sort_by(|left, right| (left.date, &left.stock).cmp(&(right.date, &right.stock)));
        self.state.corporate_actions.applied_ex_reference_groups = applied_groups;
        Ok(())
    }

    pub(super) fn poison_failed_step(&mut self, fatal: StepFatal) -> StepFatal {
        self.poison = Some(fatal.clone());
        fatal
    }

    pub fn save(&self) -> Result<SaveSlot, StepFatal> {
        let mut save = self.save_committed_projection()?;
        self.project_shared_ingress_save(&mut save)?;
        Ok(save)
    }

    pub(in crate::session) fn save_committed_projection(&self) -> Result<SaveSlot, StepFatal> {
        self.require_healthy()?;
        let runtime_state = super::persistence::capture_runtime_state(self)?;
        if !self.state.pending_report_corrections.is_empty() {
            return Err(StepFatal::InvariantViolation {
                description: "日内待处理报表更正不得写入日终存档".into(),
                location: "GameSession::save_committed_projection".into(),
            });
        }
        Ok(self.save_projection(runtime_state))
    }

    #[cfg(test)]
    pub(super) fn inject_step_failure(&mut self, fatal: StepFatal) {
        self.injected_failure = Some(fatal);
    }

    #[cfg(test)]
    pub(crate) fn inject_post_shadow_failure(&mut self, fatal: StepFatal) {
        self.post_shadow_failure = Some(fatal);
    }

    #[cfg(test)]
    fn run_pre_mutation_hook(&mut self) -> Result<(), StepFatal> {
        match self.injected_failure.take() {
            Some(fatal) => Err(fatal),
            None => Ok(()),
        }
    }

    #[cfg(test)]
    pub(super) fn run_post_shadow_hook(&mut self) -> Result<(), StepFatal> {
        match self.post_shadow_failure.take() {
            Some(fatal) => Err(fatal),
            None => Ok(()),
        }
    }
}
