# 控制授权与经济账户分离独立审查

- 审查范围：当前工作区中 `MarketMembership`、`SaveSlot`、Server actor/routes、Native SQLite 身份与控制授权表、Remote UI 相关实现及其改动。
- 依据：根 `AGENTS.md`、`docs/principles.md`、ADR-0030、ADR-0033；用户确认加载存档保留当前授权，不从档案授撤权；缺席控制者仍可控制和查看公开市场，交易前须确认加入且只发放一次资金。
- 审查方式：只读检查源代码、完整相关差异和测试定义；未运行测试或编译。统一编译在途，按要求未启动 Cargo。

## 结论

**就控制授权与存档经济账户分离的实现审查，未发现阻断问题。** 控制权限不在 Engine `SaveSlot`/`MarketMembership` 中，而由 Native SQLite `market_control_grants` 独立保存；恢复存档只替换 `ProtocolSession` 与 ingress，不替换 actor 当前的 `controllers`。归档内的成员关系只映射经济账户，不会赋予控制权。

Engine 的 `MarketMembershipState::validate` 将成员、经济账户和 NPC 账户集合做精确校验。加载后缺席主体的 `Context` 与 `Baseline` 请求不要求账户成员关系，返回公开市场数据并按主体过滤账户；受控接口仍由 `require_market_control` 校验。交易 ingress 通过主体到成员账户的映射派生账户，未成员主体得到明确的重新加入要求。`join_market` 对缺席主体要求确认；成功后使用当时的 `starting_cash` 建立空仓账户，重复加入返回既有成员，不重复入金。

Remote UI 对无账户但有控制权的身份展示控制权限状态，并保留市场控制能力；账户为空时显示重新加入入口，交易命令路径也检查账户缺失。市场控制和入场资金设置文案清楚说明作用范围。该改动没有改变 A 股撮合、费用、股份或 T+1 规则；本审查涉及的是身份、授权与资金入场边界，不需要引入新的交易所规则依据。

## 已有边界覆盖

- `packages/engine/src/session/memberships_tests.rs` 覆盖首次入金、重复加入不重复发放、恢复后缺席主体确认、成员账户校验，以及多人共享簿、账户隔离、委托所有权和 T+1。
- `apps/server/src/actor/fatal_tests.rs` 中 `loading_economic_members_keeps_current_control_without_granting_archive_members` 覆盖控制主体加载不含自己的经济档、仍可控制/查看公共市场、档案成员不获控制权、过期 generation/未确认重入被拒、确认后按新设置入金且重入不重复发放。
- `apps/server/tests/actor.rs` 覆盖成员快照账户隔离、普通成员不能执行控制操作及本人交易路径。
- `packages/native-store/tests/archives.rs` 覆盖身份/控制授权独立于市场档案并验证无效主体不能获得授权。
- `apps/web/src/app/remote-login-behavior.test.ts` 覆盖缺席控制者进入公共市场并要求确认重入；Remote auth 测试覆盖未确认重入拒绝。
- `apps/server/tests/shared_market_restart.rs` 覆盖真实 SQLite 文件跨 `SessionManager` 重启：原 controller 的身份和 `shared-market` grant 在重开数据库后恢复；加载日终档后该主体仍可查看公共市场和控制市场，但没有自动经济成员账户；档案成员保留经济账户但不获得控制权；缺席 controller 未确认不能加入，确认后按当前入场资金建立空仓账户，重复加入不重复发放。

## 复核建议与验证状态

- 上述跨生命周期组合缺口已由 `apps/server/tests/shared_market_restart.rs` 补齐。生产路径中 `SessionManager::resume_session` 从指定槽恢复经济状态，`register_game` 另从 `NativeDatabase::market_controllers("shared-market")` 装载授权；`market_control_grants` 以 `(market_id, subject_id)` 为主键独立于 `archive_slots`，档案加载/删除不修改授权表。
- 已读取 `.tmp/checklist-wave4/host19-reopened_sqlite_keeps_control_grants_separate_from_loaded_economic_members.log`：fresh binary 的该测试结果为 1 passed，耗时 2.28s。此结论只覆盖日志中的单个测试，不代表其他测试或完整编译通过。本轮没有启动 Cargo。

## 必要性与范围

把市场控制权限从 SaveSlot 成员能力中移出是 ADR-0030/0033 和用户确认规则的直接要求。各层需要分别处理认证身份、控制能力、经济成员账户和公开数据投影，改动方向必要且各职责边界清楚。当前未发现该分离本身引入不必要的 A 股领域复杂度。
