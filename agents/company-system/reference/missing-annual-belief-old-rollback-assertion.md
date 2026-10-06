# 缺少年报日终回滚断言历史

本文件逐字保留原 `packages/engine/src/session/notices.rs::information_revaluation_failure_rolls_back_entire_day_end` 的核心故障注入与断言，供修复后迁移到“公共库缺失已被 NPC acquisition 引用材料”的真实 corruption fixture。它是历史证据，不作为合法资料不足必须 fatal 的现行预期。迁移时须保留完整状态 hash/save 相等、失败后恢复材料并同日重试、结算与税凭证/月报只发生一次等全部强断言。

原始测试片段（从当前 HEAD 原样摘录；不含后续并行作者追加的新增月结校验）：

```rust
    #[test]
    fn information_revaluation_failure_rolls_back_entire_day_end() {
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        setup.start_date = crate::CivilDate::from_ymd(2032, 1, 31).unwrap();
        let mut session = GameSession::new(setup, 42).unwrap();
        assert_eq!(session.civil_clock().phase(), CivilPhase::ClosedDay);
        let id = AccountId(1);
        let code = session.state.setup.stocks[0].code.clone();
        let company = session
            .state
            .company_system
            .issuers()
            .issuer_of(&code)
            .unwrap()
            .clone();
        let settled = session.civil_date();
        let period = crate::accounting::AccountingPeriod::of_date(settled);
        let scope = crate::accounting::consolidation::ScopeId::Standalone(
            crate::accounting::consolidation::MemberId(company.0.clone()),
        );
        let finance = session.state.company_system.finance(&company).unwrap();
        assert!(
            !finance
                .books()
                .journal()
                .entries()
                .any(|entry| entry.date == settled)
        );
        assert!(
            finance
                .closing()
                .versions(
                    &scope,
                    period,
                    crate::accounting::reports::ReportKind::Monthly
                )
                .is_empty()
        );
        session
            .state
            .belief_participants
            .get_mut(&id)
            .unwrap()
            .watchlist_mut()
            .record_attention(&code, 0, 0)
            .unwrap();
        let original_library = session.state.library.clone();
        let mut public = original_library.save();
        public
            .reports
            .retain(|report| report.reports.kind != crate::accounting::reports::ReportKind::Annual);
        session.state.library = std::sync::Arc::new(PublicLibrary::from_parts(public).unwrap());
        let before = session.business_state_hash().unwrap();
        let save = serde_json::to_value(session.save().unwrap()).unwrap();
        let error = session.end_civil_day().unwrap_err();
        assert!(
            matches!(&error, SessionError::InvalidSave(message)
                if message.contains("公告重估失败") && message.contains("no own-known annual material")),
            "{error}"
        );
        assert_eq!(session.business_state_hash().unwrap(), before);
        assert_eq!(serde_json::to_value(session.save().unwrap()).unwrap(), save);
        session.state.library = original_library;
        let completed = session.end_civil_day().unwrap();
        assert_eq!(completed.settled_date, settled);
```

原测试后半段及当前同步作者新增的同日收入摘要、税计提、月报版本与财务结果断言，继续由源码保留；修复时不得删减或放宽。
