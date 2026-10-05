# 本人拒单反馈独立复核

复核对象为 `apps/web/src/host/protocol/effects.ts`、`protocol/reduce.ts`、`protocol-effects.test.ts`、`protocol-reducer.test.ts` 的本轮完整工作区 diff；复核者未参与实现。

## 结论

通过本批次独立复核，未发现必须修复的问题。该结论只覆盖本人拒单反馈，不代表全部多人功能、所有 UI 或 checklist 已完成。

## 领域语义与必要性

- 本轮只修正 `IntentRejected` 的通知归属，不改变 A 股委托受理、撮合、资金或股份规则，拒单原因的原有文本和类型穷尽检查仍保留。NPC 的业务拒单保留原始事实，不冒充玩家操作反馈；系统 `SettlementError` 的显式通知不被该筛选隐藏。
- 原实现以数字 `0` 判断玩家，既不满足当前规范十进制字符串 `AccountId` 契约，也无法覆盖 Remote 非零玩家。现实现保留完整 u64 字符串比较，不经过 `Number`，修复属于本轮多人账户契约的必要改动。
- `scopedBaselineAccount` 只接受当前基线的唯一账户；空账户和多账户不猜测本人，直接 effects 调用同样默认没有经济账户。这不是把管理员控制身份偷换为经济账户。

## 跨层与边界核对

- 实际追读 `apps/server/src/actor.rs` 的 `EngineUpdate::for_member_account` 和 `MemberRequest::Baseline`：Remote 依据 membership 过滤快照、增量和事件，缺席控制者的账户为零个，本人账户为一个；别人的私有事件变为 `PrivateEventOmitted`。
- 实际追读 `packages/engine/src/session/snapshot.rs`：公开 snapshot 不包含 NPC；本地 WASM 和 Desktop 的普通玩家意图入口固定 `AccountId(0)`。本批次未把无 owner 的通用多账户快照当作玩家零。
- `parseProtocolSnapshot` 对账户对象键逐个调用规范 u64 校验；effects 的显式 owner 也调用同一校验器，不新增宽松解析或身份兼容路径。
- `ProtocolCoordinator.installBaseline` 在交付 baseline 回调前安装 generation 和账户；Remote 先校验同 generation context 再交付 baseline。reducer 先拒绝代际不匹配，后处理 exact retry，再生成 effects；通知归属读取更新前同代基线，不由新包临时带入另一账户反向决定。
- 非零／MAX u64 本人、另一零账户、缺席／多账户、非法身份、重复包、旧 generation、原始事件保留均有短断言。排序不变性测试补真实账户零基线，仍保留原来的 notice 和状态不变性断言，并未弱化预期。

## 简单单元测试

独立重跑以下四个套件，case timeout 为 10000 ms，外部整命令 deadline 为 10000 ms，并发为 4：

```sh
node scripts/run-with-deadline.mjs 10000 -- node --experimental-strip-types --test --test-timeout=10000 --test-concurrency=4 apps/web/src/host/protocol-effects.test.ts apps/web/src/host/protocol-reducer.test.ts apps/web/src/host/protocol-public-events.test.ts apps/web/src/host/protocol-coordinator.test.ts
```

结果为 29 / 29 通过，0 失败、0 跳过，测试报告耗时 290.564463 ms。未运行 Cargo、网络验证、复杂回归或实机矩阵。
