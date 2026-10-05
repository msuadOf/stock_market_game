# 旧 Session 年末断言参考

迁移前 `packages/engine/tests/company_scenarios/lifecycle.rs` 中的原始 Session 耦合断言。字段已从当前 `SaveSlot` 移除，本文仅保留迁移证据，不作为现行契约或生产 API：

```rust
let closing_before = year_end
    .save()
    .expect("healthy save")
    .closing_registry
    .versions(&scope, annual_period, ReportKind::Annual)
    .len();

assert!(
    !year_end
        .save()
        .expect("healthy save")
        .company_operations
        .scheduler()
        .pending()
        .is_empty(),
    "year-end must retain future operating obligations"
);

let after_close = year_end.save().expect("healthy save");
assert!(
    after_close
        .closing_registry
        .versions(&scope, annual_period, ReportKind::Annual)
        .len()
        > closing_before
);
```

现行 Simple Session 只由 `CompanySystem` 持有基本面与报告事实。ClosingEngine 年报版本数在场景中从真实 `SaveSlot.company_system` 的序列化值提取并反序列化为 `SimpleFinanceState` 后核对；经营调度义务断言移至 `company_operations` 低层 fixture，不表示当前 Session 持有这类经营状态。
