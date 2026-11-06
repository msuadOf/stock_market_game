# WS fixture 契约修复

日期：2026-10-25。工作树：`.worktree/ws-fixture-fix`。本批未提交、未 push，未改集成树，未跑完整回归。

## 定位与必要性

`apps/server/tests/ws.rs::sample_setup_json` 未提供 `company_system`，却携带 `dividend_tax_mode`、`flat_withholding_bp` 和 `par_value_per_share`。`packages/engine/src/session.rs::validate_company_fields` 在公司入口缺席时以 bit0、bit1、bit4 标识三项残留，故 `1 + 2 + 16 = 19`，返回 `InvalidSetup`、fault 1048、context `[19]`。

该错误不是证券数量、`StockId`、公司 ID 或工具权限错误。`api_contract.rs` 的合法最小 setup 本来不含三项字段。主工作区与本批修复前 WS 源码逐字一致，仅作为静态归属证据；本批没有构建、运行 clean main，不能把本次红测称作 clean main 动态复现。

依据为 ADR-0035 的显式 CompanySystem 共同入口与未实现配置显式拒绝、ADR-0041 的结构化 fault 契约，以及当前 `validate_company_fields` 与 `session_company_foundation` 契约用例。本批没有实现或修改交易制度，不改变沪深 A 股证券、交易所、板块、价格/数量单位、撮合、费用或 T+1 语义，无需引入新的交易规则依据。

## 最小改动

仅删除 `apps/server/tests/ws.rs` fixture 中上述三项非空残留字段。合法 false 项、`report_frequency`、证券配置保持原样。11 个既有 WS 用例、全部网关断言、真实 `127.0.0.1:0` 端口、失败路径与异步 shutdown 清理均保留；不修改生产校验，不新增镜像 helper 测试。

精确 patch：`ws-fixture.patch`，SHA256 `90509836871a83617dda6a19381c5b9a4fdeeb0339b9bb3b8d2e609764a76fa3`。

## 证据与验证

- 原 full8 源清单 1679 项精确复制并核 SHA，清单见 `source-baseline-full8.json`。其不覆盖的 `std-thread-proto` workspace 源另外复制并列 SHA，见 `additional-workspace-source.json`。该 crate 不属于 engine 依赖。
- 原 full8 WS 二进制冻结于根 `.tmp/ws-fixture-contract/full8-ws-frozen`，SHA256 `42e2d0fbc95378e7210ddcd09f475a44e8272771feaed422b59a1e73c3b99eb7`。在独立外进程 10000ms deadline 内运行原网关 case，实际 exit101，明确复现 1048/[19]，wall 126.867ms；见 `original-binding.json` 与 `original-red-result.json`。
- 私有 cache 构建显式 `cargo -j 128`，进程外 `run-long-validation.mjs 300000` 监督，构建和源码/产物绑定记录总 wall 102.356s；实际 Cargo 显示 1m40s。采样峰值 15 个进程、91 个线程；终态采样所见 PID 全部不存在。见 `build-start.json`、`build-cpu.jsonl`、`build-terminal.json`、`build-result.json`。编译有既有 warnings，未处理无关告警。
- 构建源码前后完全一致；与 full8 清单重叠部分仅 WS fixture 改变。新 WS 实际二进制 SHA256 `377b0942a2cabaefd8d8154efbbab80d4132b5c7c24513cda3426cab55598d24`。另外两份实际 API/engine 产物 SHA 与完整源绑定见 `delivery-binding.json`。
- 实际 listing 共 11 个 WS case；其中 10 个普通 case 全过，既有 `publisher_modes_report_actual_speed` 保持 ignored，未运行性能探针，不能写成 11 passed。另跑合法 API 创建、非法 API setup 拒绝、缺公司入口配置拒绝、逐字段残留隔离，共 4 个短契约，全过。
- 14 个 case 独立进程并发，`--test-threads=1`，各自临时目录在根 `.tmp/ws-fixture-contract/cases/`，每个进程独立外部 10000ms deadline，包含终止清理；最大 case wall 663.072ms。测试前后源码及三个实际二进制 SHA 均不漂移。完整结果见 `short-result.json`，清单见 `ws-listings.json`。

## 已知证据边界

私有 cache 准备使用 `cp -a --reflink=always` 从正式 cache 复制 Btrfs CoW 元数据，逻辑大小 49G。该准备在单独命令中已经完成，未放进外部共享 300000ms deadline，也未记录精确 wall；不回填时限，不把它算作受监督构建的组成部分。本批未重新复制 cache 补证，正式 cache 未被写入。后续可能耗时的 cache 复制必须先纳入外部有界监督。

构建终态核对范围是 CPU 采样捕获的 PID；不宣称有限采样证明任意未采样后代不存在。WS fixture 的 shutdown 与断言 panic 清理通过原有两个真实 case 验证。完整回归尚未重跑，后续阶段状态没有因这些短证改变。

本批交未实施者独立审查完整三行 diff、A 股语义、必要性、边界与实际证据后，再交主 agent 集成。
