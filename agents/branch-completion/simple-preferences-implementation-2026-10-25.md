# PREF-1：Simple 局内偏好配置候选

- 基线：`b172c1ab`；分支：`feat/company-simple-preferences-session`。
- 合同：Leader 冻结的 PREF-1；规划主树文件
  `agents/branch-completion/company-commands-plan-2026-10-25.md` §3。
- 依据：[ADR-0037](../../docs/decisions/0037-company-action-initiator.md)、
  [ADR-0039](../../docs/decisions/0039-rights-placement-split-scope-and-toggle.md)。
  本批没有新增或修改真实交易制度；只恢复游戏化策略参数的 Rust 接缝。

## 实施范围

getter/setter 均先 `require_healthy`，独立非 wire `SimplePreferencesError` 保留
`StepFatal` 与 `CompanySystemError`。setter 只 clone `CompanySystem` 与配置候选，
调用既有域校验、完整配置勾稽后才标记一次 external revision 并安装两侧配置。
不 clone 整个 Session、不触碰账户和常驻 runtime。两侧配置不一致直接拒绝，禁止
选一侧覆盖另一侧。公共 save 验证增加完整配置一致性勾稽；复用现有
`SessionError::Step(StepFatal::InvariantViolation)`，稳定 location 为
`validate_save_slot.company_system_config`，该验证不会毒化运行中的会话。

Protocol 显式转发，不改变 `save` 返回上次已发布日终档的职责。只有后续自然日日终
发布才把新配置带入新的公共存档。自动评估、公司行为日终执行、其他偏好参数与
WASM/server/desktop wire 适配不在本批，已同步正式交易规则文档。

## 红绿与验证

所有产物放主树 `.tmp/simple-preferences-*`，未纳入 Git；独占 target 目录
`.tmp/simple-preferences-target`。构建均 `jobs=16`、进程外 `300000ms` deadline；
最终只运行本主题 8 个短 case，`--test-threads=8`，整命令进程外 `10000ms` deadline。

1. 先添加调用尚不存在 API 的测试，构建失败：缺少 getter/setter/typed error；
   同次发现测试使用私有 `civil` 路径，随后改为正式 Protocol reexport。
   日志：`.tmp/simple-preferences-red.log`。
2. 最终构建：
   `CARGO_TARGET_DIR=<主树>/.tmp/simple-preferences-target CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 node scripts/run-long-validation.mjs 300000 -- cargo test -p engine --lib --no-run -j 16`。
   日志 `.tmp/simple-preferences-complete-build.log`，通过，23.04 秒；
   存量 warnings 如实保留，未宣称 lint/Clippy 通过。
3. 最终短测：
   `node scripts/run-with-deadline.mjs 10000 -- <主树>/.tmp/simple-preferences-target/debug/deps/engine-3eb2aa70a85e274e session::simple_preferences_tests --test-threads=8 --nocapture`。
   日志 `.tmp/simple-preferences-complete.log`，8 passed，0 failed，0.54 秒。
4. `git diff --check` 通过。未运行全量验收、未 merge/push。

短测覆盖两项全部字段的合法上下边界/非法值、None、读取无副作用、重复提交与 revision、
未知/缺席/poison/配置撕裂的失败原子性、内存及 JSON 的单侧配置编辑拒绝、编辑后
save→restore→save 深度一致、Protocol 失败与已发布档保持、checkpoint rollback。
真实 runtime 在编辑前启动，编辑后两个真实 step；用 `publication()` 的
`Arc::ptr_eq` 核对同一 world 发布槽，避免只比较内联字段地址的弱断言。

## 独立审查

Leader 派未实施者 `sol61_review_d1a_design` 完整复核；预审未发现生产阻断，
有效测试发现（checkpoint、后续两 step、真实 driver identity）已修复。
最终提交仍须该 reviewer 终审后才可宣称本批完成。
