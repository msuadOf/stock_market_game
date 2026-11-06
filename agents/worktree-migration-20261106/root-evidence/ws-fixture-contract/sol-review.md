# WS fixture 契约修复独立复核

2026-10-25；审查者：未参与实施的 `/root/sol61_proc_exit_review`。

**APPROVED：仅批准 `apps/server/tests/ws.rs` 删除三项残留配置的精确补丁。**

- 补丁：ws-fixture.patch（历史路径：`ws-fixture.patch`），SHA-256 `90509836871a83617dda6a19381c5b9a4fdeeb0339b9bb3b8d2e609764a76fa3`。
- 基线文件 SHA-256：`caf057711e10583f29c0a59d52a7002409ff1192af3d554696840ed07bf3690c`。
- 批准文件 SHA-256：`acf6ddb28fe114fd6c135caa99abec623c3555cb7a658168b5536ba92ad86f66`。

已读取完整 diff、原始运行结果、构建记录及相关源码，并重算四份实际新旧 binary SHA；独立 `git apply --check` 退出 0。未实施修复、编译或运行测试。核对记录见 sol-review-binding.json（历史路径：`sol-review-binding.json`）。

## 三项审查结论

1. **符合现行领域语义。** `sample_setup_json` 不携 `company_system`，原有 `dividend_tax_mode`、`flat_withholding_bp`、`par_value_per_share` 分别形成残留位 1、2、16；`validate_company_fields` 明确以 fault1048/context[19] 拒绝。删除这些字段恢复无公司入口的合法 fixture，没有把缺失公司配置变成默认税率或公司状态。已核 ADR-0035、现行 engine 校验与逐字段残留拒绝测试；本批没有改交易制度、A 股单位、撮合、T+1、费用、存档或生产 API，无新增交易所规则判断。
2. **必要且范围最小。** 完整补丁仅删除三行；false 项、`report_frequency`、证券配置、全部 11 个测试定义和 ignore 标记、网关断言、真实 `127.0.0.1:0` 端口与异步 cleanup 均不变。没有修改生产校验或新增镜像 helper 测试。
3. **本次所需边界已覆盖，未发现剩余集成阻断。** 10 个普通 WS case 全部通过；另 2 个 API 合法/非法 setup case 与 2 个 engine 公司字段拒绝 case 通过。已有缺公司入口和逐字段残留负例继续有效；真实 listener/session 的 shutdown 与断言 panic 清理仍由原测试覆盖。手工性能探针 `publisher_modes_report_actual_speed` 保持 ignored、未执行；不能称为 11 个 WS 全过。

## 红绿与源产物绑定

full8 原日志的六个失败均为 `ws.rs:99` 的 1048/[19]。另以冻结原 binary SHA `42e2d0fbc95378e7210ddcd09f475a44e8272771feaed422b59a1e73c3b99eb7` 重跑原 gateway case，真实 exit101、126.867ms，同因失败，满足本次 TDD 红证；见 original-red-result.json（历史路径：`original-red-result.json`）。主区相同源码只作静态预存证据，没有运行 clean-main。

私有构建 `-j128`、进程外 300000ms deadline，实际 102.356s 成功；1694 项构建源前后相同。与 full8 密封清单重叠的 1679 项中仅 `ws.rs` 改变，另外 15 项是补齐的 workspace 原型源。已亲核 Cargo 三个目标均 `fresh=false`；构建采样存在多个 rustc 并行。构建和产物详情见 build-result.json（历史路径：`build-result.json`）、delivery-binding.json（历史路径：`delivery-binding.json`）。

新 WS binary SHA 为 `377b0942a2cabaefd8d8154efbbab80d4132b5c7c24513cda3426cab55598d24`；另外 API/engine 两份 binary 的实际 SHA 也与交付绑定一致。14 个普通 case 各独立进程并发执行、每进程和 case 共用 10000ms 外部进程树门禁，全部输出恰好 1 passed/0 failed，最大 wall 663.072ms。源和三个产物在测试前后无漂移，详见 short-result.json（历史路径：`short-result.json`）。

私有 cache 准备复制未纳入共享外部 deadline，且未采精确 wall，这是已保留的流程证据缺口；不回填、不因此宣称全部准备过程有界。构建终态只覆盖采样见到的 PID，不扩称任意未采样后代均已确认。上述限制不改变三行修复及实际短测结果。

本批准仅供该单文件精确集成；full8 仍是正式失败，后续整树 before 与完整回归须另行验收。
