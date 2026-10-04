# G20 / G21 独立复核

## 范围与依据

- 本复核者未实施相关产品代码；完整阅读 G20 / G21 总账要求、`reaudit-host.md`、`reaudit-tools.md`、`seed-baseline.md`，并遵守 `AGENTS.md`、`docs/principles.md`、架构与开放问题约束。
- 完整检查 `session-seed.ts`、`session-seed.test.ts`、`price_volume_baseline.rs`（含测试）及量价清单文档的相关 diff；另沿 `App.tsx` → `useSessionHostLifecycle.ts` 检查生产 seed 消费链与 lifecycle 测试。非 seed 的宿主确认与资源清理由 `review_ui_contracts` 复核，避免重复门禁。
- 依据为 ADR-0005 §4 的熵取种 / 固定测试注入，以及量价清单 §7 的 setup-only 输入契约。本改动不改变交易制度，不新增 A 股规则主张；CLI 继续用 `SessionSetup.validate()` 保留 T+1、证券配置、policy 与日期门禁，未要求新增官方规则材料。

## 产品与语义检查

- seed helper 使用真实 Web Crypto `getRandomValues`，两个 uint32 经 bigint 拼成完整 u64，不经过浮点数；每次读取新熵。`0n` 是合法值，不作 truthy/default 替换。能力缺失、调用异常显式抛错并保留 cause，没有墙钟、`Math.random` 或固定 seed 降级。
- 生产 App 不注入 `createSeed`，实际 lifecycle 默认绑定 helper；只在无存档、非 E2E 的新局调用。E2E 明确固定 `DEFAULT_SEED`，读档用原十进制字符串转 bigint 且不消耗熵。初始化 catch 把 helper 错误交给现有显错 UI。
- CLI 改为独立 `SetupProjection`，只对 setup 反序列化与领域校验；合法 JSON 中无关 envelope/runtime/快照/委托/账户错误类型不影响 setup。重复 setup、缺失或非法 setup 拒绝；不是恢复存档，也不消费输入 seed/RNG。量价与 causal 两条计算路径都基于投影 setup 创建新局。
- setup 自身的新 policy/date/company_operations/groups 契约继续归属 `SessionSetup`；其默认值来自明确 serde 契约，不是对非法字段 fallback。其他 CLI 和正式 `SaveSlot` 读档路径未因本 G21 修改而放宽。
- 文档仅补正式 CLI 所需 feature 参数；实现是解决 G20/G21 所必需的最小边界改动，无新增依赖与无关重构。

## 边界发现与处置

- 请求作者补 CLI envelope/runtime 错误类型仍接受、setup policy/date/company_operations/groups 非法输入拒绝的边界；作者已补齐，增量断言静态复核通过。
- 请求 UI 作者补 lifecycle 对 `0n` 的原样传递，以及熵失败时不创建 host、不 ready、显式设置错误的消费链断言；作者已补齐，新增断言与最终生产消费链已复核通过。`review_ui_contracts` 独立运行包含该文件的十文件并行短测，exit 0、10/10 文件通过，shell wall 1.50 秒，记录见 `ui-contracts-review.md`。

## 验证与门禁

- 已查阅实际 seed 红/绿日志与并行短测日志；helper + defaults 短测通过。未自行运行回归、长模拟或真实行情校准。
- 初审时 `baseline-green-build.log` 为其他并行实现导致的六项编译错误，并未视为绿灯。稳定后的同名日志已更新为成功构建（20.47 秒）；作者用外层 300000ms deadline、`flock` 与 `-j16` 分离编译和短测，成功 binary 在锁内复制为独立文件，避免共享 target 后续覆盖。`baseline-green-list.log` 明确列出当前五 case，`baseline-green.log` 为 5/5 通过，0.01 秒；红灯时四 case 不冒充当前五 case。
- 最终增量复核完整读取 setup 内非法 policy/date/company_operations/groups 与顶层 inert envelope/runtime 断言，未发现跨层弱化。复核者独立执行 `timeout -s KILL 10s .tmp/seed-baseline/price-volume-baseline-tests --test-threads=8`，exit 0，5 passed / 0 failed / 0 ignored / 0 filtered，harness 0.01 秒；对应产品和文档 `git diff --check` exit 0。未独立重复 Cargo 编译或执行长模拟。
- **G20：最终 PASS。** helper 的完整 u64/每次熵读取/零值/显错短测，以及真实 lifecycle 的普通新局/E2E/存档/零值/失败显错消费链均已通过；独立审查未留产品或测试发现。此结论允许 root 单独核销 G20，不依赖不相关 G21 Rust 编译。
- **G21：最终 PASS。** setup-only CLI 的完整 diff、大 A 语义、最小必要性及新增边界经独立复核通过；真实构建绿灯与独立五 case 并行短测已核实，无遗留发现。允许 root 单独核销 G21；不代表全量回归、完整宿主验收或真实行情校准完成。
