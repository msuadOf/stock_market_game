use engine::{session::protocol::ProtocolSession, Event, SessionSetup};
use serde_json::Value;
use std::error::Error;
use std::fs;

fn next_public_frame(
    session: &mut ProtocolSession,
) -> Result<engine::session::protocol::TickFrame, Box<dyn Error>> {
    while session.civil_day_ready()? {
        session.end_civil_day_update()?;
    }
    Ok(session.step_frame()?)
}

fn has_npc_acceptance(frame: &engine::session::protocol::TickFrame, npc_count: u64) -> bool {
    frame.events.iter().any(|event| match event {
        Event::OrderAccepted { account, .. } => account.0 > 0 && account.0 <= npc_count,
        _ => false,
    })
}

fn ensure_receipt_cursors_do_not_regress(
    before: &engine::IngressReceiptCursors,
    after: &engine::IngressReceiptCursors,
) -> Result<(), Box<dyn Error>> {
    for (account, ordinal) in &before.next_account_ordinal {
        if after
            .next_account_ordinal
            .get(account)
            .copied()
            .unwrap_or(0)
            < *ordinal
        {
            return Err(format!("账户 {account:?} 的 receipt cursor 回退").into());
        }
    }
    for (code, ordinal) in &before.next_stock_ordinal {
        if after.next_stock_ordinal.get(code).copied().unwrap_or(0) < *ordinal {
            return Err(format!("证券 {code:?} 的 receipt cursor 回退").into());
        }
    }
    Ok(())
}

fn validate_corporate_action_and_finance_state(encoded: &Value) -> Result<(), Box<dyn Error>> {
    let actions = encoded
        .get("corporate_actions")
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 corporate_actions 对象")?;
    for field in [
        "registries",
        "dividends",
        "account_gross_receipts",
        "external_receipts",
        "applied_ex_dividend_groups",
    ] {
        if actions
            .get(field)
            .and_then(Value::as_array)
            .is_none_or(|rows| !rows.is_empty())
        {
            return Err(
                format!("新局 corporate_actions.{field} 必须是 Engine 生成的空数组").into(),
            );
        }
    }

    let markets = encoded
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("markets"))
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 snapshot.markets")?;
    for (code, market) in markets {
        for field in ["cash_ex_reference_pending_trade", "day_market_activity"] {
            if market.get(field).and_then(Value::as_bool).is_none() {
                return Err(
                    format!("snapshot.markets.{code}.{field} 缺少 Engine 生成的布尔状态").into(),
                );
            }
        }
        if !market
            .get("last_cash_ex_reference")
            .is_some_and(Value::is_null)
        {
            return Err(format!(
                "新局 snapshot.markets.{code}.last_cash_ex_reference 必须为 Engine 生成的 null"
            )
            .into());
        }
    }

    let companies = encoded
        .get("company_system")
        .and_then(|system| system.get("implementation"))
        .and_then(|implementation| implementation.get("state"))
        .and_then(|state| state.get("companies"))
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 Simple companies")?;
    for (company, state) in companies {
        let finance = state
            .get("finance")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("Simple company {company} 缺少 finance"))?;
        if !finance.get("legal_facts").is_some_and(Value::is_null)
            || finance
                .get("dividends")
                .and_then(Value::as_object)
                .is_none_or(|rows| !rows.is_empty())
        {
            return Err(format!(
                "新局 Simple company {company} 的 legal_facts 必须为 null 且 dividends 必须为空对象"
            )
            .into());
        }
        let batches = finance
            .get("books")
            .and_then(|books| books.get("journal"))
            .and_then(|journal| journal.get("batches"))
            .and_then(Value::as_array)
            .ok_or_else(|| {
                format!("Simple company {company} 缺少 Engine 生成的 books.journal.batches sources")
            })?;
        let mut sources = std::collections::BTreeSet::new();
        for batch in batches {
            for entry in batch
                .as_array()
                .ok_or_else(|| format!("Simple company {company} 的 journal batch 不是数组"))?
            {
                let source = entry.get("source").and_then(Value::as_u64).ok_or_else(|| {
                    format!("Simple company {company} 的 journal entry 缺少整数 source")
                })?;
                if !sources.insert(source) {
                    return Err(format!("Simple company {company} 的 journal source 重复").into());
                }
            }
        }
        let next_source = finance
            .get("next_event_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("Simple company {company} 缺少十进制字符串 next_event_id"))?
            .parse::<u64>()?;
        if sources.is_empty()
            || sources.iter().next() != Some(&1)
            || sources
                .iter()
                .next_back()
                .is_none_or(|source| next_source <= *source)
        {
            return Err(format!(
                "Simple company {company} 的 journal source 与 next_event_id 不一致"
            )
            .into());
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let input_path = arguments.next().ok_or("缺少旧 fixture 路径")?;
    let output_path = arguments.next().ok_or("缺少临时输出路径")?;
    if arguments.next().is_some() {
        return Err("参数过多".into());
    }

    let old_fixture: Value = serde_json::from_str(&fs::read_to_string(input_path)?)?;
    let mut setup_value = old_fixture
        .get("setup")
        .ok_or("旧 fixture 缺少 setup")?
        .clone();
    let seed_text = old_fixture
        .get("seed")
        .and_then(Value::as_str)
        .ok_or("旧 fixture 缺少十进制字符串 seed")?;
    let seed = seed_text.parse::<u64>()?;

    let setup_object = setup_value
        .as_object_mut()
        .ok_or("旧 fixture 的 setup 不是对象")?;
    setup_object.remove("company_operations");
    setup_object.remove("groups");
    let companies = setup_object
        .get("stocks")
        .and_then(Value::as_array)
        .ok_or("测试场景缺少 stocks 数组")?
        .iter()
        .map(|stock| {
            let code = stock.get("code").and_then(Value::as_str)
                .ok_or("测试场景缺少证券代码")?;
            Ok(serde_json::json!({
                "company": format!("C-{code}"),
                "kind": "Industrial",
                "generation": {
                    "initial_revenue": "1000000000",
                    "initial_fixed_expense": "900000000",
                    "revenue_trend": { "kind": "Fixed", "annual_growth_bp": 0 },
                    "fixed_expense_trend": { "kind": "Fixed", "annual_growth_bp": 0 },
                    "revenue_noise": { "monthly_bp": 0, "quarterly_bp": 0, "half_year_bp": 0, "annual_bp": 0 },
                    "fixed_expense_noise": { "monthly_bp": 0, "quarterly_bp": 0, "half_year_bp": 0, "annual_bp": 0 },
                    "demand_sensitivity_bp": 0,
                    "variable_expense": {
                        "rule": "RevenueRatio",
                        "ratio_bp": 0,
                        "noise": { "monthly_bp": 0, "quarterly_bp": 0, "half_year_bp": 0, "annual_bp": 0 }
                    }
                },
                "finance": {
                    "opening_lines": [
                        { "account": "1122", "side": "Debit", "amount": "500000000" },
                        { "account": "4001", "side": "Credit", "amount": "500000000" }
                    ],
                    "tax_policy": {
                        "version": 1,
                        "vat": { "output_rate_bp": 1300, "input_rate_bp": 1300, "deductible_share_bp": 10000 },
                        "income_tax": { "rate_bp": 2500, "loss_carryforward_years": 5 }
                    },
                    "summary_rule": "ReceivableRevenuePayableExpenses"
                }
            }))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    setup_object.insert("company_system".to_owned(), serde_json::json!({
        "mode": "Simple",
        "config": {
            "environment": {
                "initial_change_bp": 0,
                "persistence_bp": 0,
                "noise": { "monthly_bp": 0, "quarterly_bp": 0, "half_year_bp": 0, "annual_bp": 0 }
            },
            "companies": companies,
            "prehistory_periods": 24,
            "settlement_cycle": "Monthly"
        }
    }));
    setup_object.insert(
        "report_frequency".to_owned(),
        serde_json::json!("Quarterly"),
    );
    setup_object.insert(
        "float_allocation".to_owned(),
        serde_json::json!({
            "between_kinds": { "Percentage": { "retail": 0.4, "inst": 0.5, "hot": 0.1 } },
            "within_kind": "Random"
        }),
    );
    let setup: SessionSetup = serde_json::from_value(setup_value)?;

    if seed != 666_959_854
        || setup.stocks.len() != 5
        || setup.npcs.retail_count + setup.npcs.inst_count + setup.npcs.hot_count != 26
        || setup.ticks_per_day != 60
        || setup.report_frequency != engine::information::ReportFrequency::Quarterly
    {
        return Err("旧 fixture 的 setup/seed 不符合要求".into());
    }

    let ticks_per_day = setup.ticks_per_day;
    let mut session = ProtocolSession::new(setup, seed)?;
    for _ in 0..2 {
        for _ in 0..ticks_per_day {
            session.step_frame()?;
        }
        session.end_civil_day_update()?;
    }

    let save = session.save()?;
    if save.snapshot.tick != 120 || save.civil_clock.settled_through.is_none() {
        return Err("生成结果未完成两个交易日的日终结算".into());
    }
    let encoded = serde_json::to_value(&save)?;
    validate_corporate_action_and_finance_state(&encoded)?;
    let memberships = encoded
        .get("market_memberships")
        .and_then(|value| value.get("members"))
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 market_memberships.members")?;
    let local_owner = memberships
        .get("local-owner")
        .ok_or("Engine 未生成 local-owner 成员关系")?;
    if local_owner.get("account_id").and_then(Value::as_str) != Some("0")
        || local_owner
            .get("admission_funding")
            .and_then(|value| value.get("external_cash"))
            != encoded
                .get("setup")
                .and_then(|value| value.get("config"))
                .and_then(|value| value.get("starting_cash"))
    {
        return Err("Engine local-owner 成员身份或入场资金与当前 Setup 不一致".into());
    }
    let saved_accounts = encoded
        .get("snapshot")
        .and_then(|value| value.get("accounts"))
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 snapshot.accounts")?;
    for (subject, membership) in memberships {
        let account_id = membership
            .get("account_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("market_memberships.members.{subject}.account_id 不是字符串"))?;
        if account_id.is_empty()
            || account_id.bytes().any(|byte| !byte.is_ascii_digit())
            || (account_id.len() > 1 && account_id.starts_with('0'))
            || !saved_accounts.contains_key(account_id)
        {
            return Err(format!(
                "market_memberships.members.{subject}.account_id 不是已存在账户的规范十进制字符串"
            )
            .into());
        }
    }
    encoded
        .get("report_correction_operations")
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 report_correction_operations 对象")?;
    encoded
        .get("runtime_state")
        .and_then(|value| value.get("personal_trade_confirmations"))
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 personal_trade_confirmations 对象")?;
    let system = encoded
        .get("company_system")
        .ok_or("Engine 保存结果缺少 company_system")?;
    if system["implementation"]["mode"] != "Simple" {
        return Err("测试场景未保存所选 Simple 模式".into());
    }
    for field in [
        "company_operations",
        "closing_registry",
        "ops_wiring",
        "groups",
    ] {
        if encoded.get(field).is_some() {
            return Err(format!("Simple 测试存档仍携带旧仿真字段 {field}").into());
        }
    }
    let companies = system
        .get("implementation")
        .and_then(|value| value.get("state"))
        .and_then(|value| value.get("companies"))
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 Simple companies")?;
    if companies.len() != save.setup.stocks.len() {
        return Err("Simple 测试存档的公司数量与场景证券数量不一致".into());
    }
    let history_reads = encoded
        .get("history_reads")
        .ok_or("Engine 保存结果缺少 history_reads")?;
    let history_accounts = history_reads
        .as_object()
        .ok_or("Engine 保存结果的 history_reads 不是对象")?;
    let snapshot_accounts = encoded
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("accounts"))
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 snapshot.accounts")?;
    let history_account_keys = history_accounts
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let snapshot_account_keys = snapshot_accounts
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if history_account_keys != snapshot_account_keys {
        return Err("history_reads account keys 与 snapshot.accounts 不一致".into());
    }

    let price_memories = encoded
        .get("price_memories")
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 price_memories")?;
    for (account_id, memory) in price_memories {
        let stocks = memory
            .get("stocks")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("price_memories.{account_id}.stocks 不是对象"))?;
        for (code, stock_memory) in stocks {
            for old_field in [
                "public_history_read_count",
                "last_public_history_read_minute",
            ] {
                if stock_memory.get(old_field).is_some() {
                    return Err(format!(
                        "price_memories.{account_id}.stocks.{code} 含已删除字段 {old_field}"
                    )
                    .into());
                }
            }
        }
    }

    let restored = ProtocolSession::restore(&save)?;
    if serde_json::to_value(restored.save()?)? != encoded {
        return Err("ProtocolSession::restore 后重新保存与原保存不一致".into());
    }
    let npc_count = u64::from(save.setup.npcs.retail_count)
        + u64::from(save.setup.npcs.inst_count)
        + u64::from(save.setup.npcs.hot_count);
    let mut uninterrupted = session;
    let mut resumed = restored;
    let mut last_tick = save.snapshot.tick;
    let mut uninterrupted_seq = save.snapshot.seq;
    let mut resumed_seq = save.snapshot.seq;
    let mut uninterrupted_accepted_npc = false;
    let mut resumed_accepted_npc = false;
    for _ in 0..3 {
        let original_frame = next_public_frame(&mut uninterrupted)?;
        let resumed_frame = next_public_frame(&mut resumed)?;
        let expected_tick = last_tick + 1;
        if original_frame.tick != expected_tick
            || resumed_frame.tick != expected_tick
            || original_frame.seq_from != uninterrupted_seq
            || resumed_frame.seq_from != resumed_seq
            || original_frame.seq_to < original_frame.seq_from
            || resumed_frame.seq_to < resumed_frame.seq_from
        {
            return Err("日终原会话或恢复会话的后续 tick/sequence 不连续".into());
        }
        uninterrupted_accepted_npc |= has_npc_acceptance(&original_frame, npc_count);
        resumed_accepted_npc |= has_npc_acceptance(&resumed_frame, npc_count);
        last_tick = expected_tick;
        uninterrupted_seq = original_frame.seq_to;
        resumed_seq = resumed_frame.seq_to;
    }
    if !uninterrupted_accepted_npc || !resumed_accepted_npc {
        return Err("日终后连续推进没有观察到原会话与恢复会话的真实 NPC OrderAccepted".into());
    }
    ensure_receipt_cursors_do_not_regress(
        &save.ingress_receipt_cursors,
        &uninterrupted.game().save()?.ingress_receipt_cursors,
    )?;
    ensure_receipt_cursors_do_not_regress(
        &save.ingress_receipt_cursors,
        &resumed.game().save()?.ingress_receipt_cursors,
    )?;
    fs::write(output_path, serde_json::to_string(&save)? + "\n")?;
    Ok(())
}
