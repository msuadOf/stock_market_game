# F3 冻结交接

- 独占 worktree：`.worktree/company-system-inputs`；branch：`feat/web-company-system-inputs`。
- 基线：`3984e14167202dc645ec340fbd2a73e661c71c8a`，不重复 I-D。源码冻结 `e7850b5c`，精确文件 SHA 见 `source-manifest.json`。
- 三个实现提交：`8d5d9fb4` 机械导出当前严格 parser；`fc81fa5f` 纯表单；`e7850b5c` 显式期间 preset 与 seed 草稿。无 push。
- 不涉及 App/defaults/host/protocol/store 或新依赖；本地依赖仅 symlink。没有执行 Cargo/build/full/all 或浏览器。

## 行为与语义

- numeric CompanyId 回调，不使用旧 C-code 拼接。Money 为精确 i64 分字符串；会计金额为 i128 两位小数元字符串。
- 偏好输入保留非法 raw 并显示错误，合法后回写；外部配置变化时无非法编辑则回填，有非法编辑则保留并显示冲突提示。送转原本 null 时开启为空字段，明确完整输入后才回写，不注入未经批准的旧送转参数。现金分红启用参数只使用计划批准的 3000bp／100000000分／1周期。
- preset 仅接受调用者显式模板与每公司完整期间金额区间；只采样营收、固定开支和 Growth 变动开支。无价格、股本或年度金额换算输入。源合同 r2 SHA `a6233ec61ab4c1ef0e799489c5365d5923812357b89b5595643383dafaa1e08a` 已由独立 Sol 核准设计；不是实现验收。
- seed 编辑不重建 JSON，custom 周期切换显式拒绝，preset 切换必须提供新周期范围。显式 regenerate 才抽一次熵；创建解析使用展示 seed。JSON/seed/范围非法在创建预检前失败，输入失败不抽熵、不改原草稿。
- Simple 仅账面、虚拟、可编辑、seed 可复现；Simulation 显式拒绝。四种会计 kind 与 opening_lines 原样保留，不制造默认现金。结构 parser 不替代 Rust 创建领域门禁，不宣称开账/发行人等已通过真实创建。
- 不宣称公司日终自动提案已交付。未接 App；仍需真实 Rust 创建旅程后由入口 writer 接线。

## TDD 与验证边界

各 `red-*` 目录保留红源 snapshot、source SHA、原始 stdout/stderr 与 receipt；初版旧控件已能执行，业务红分别证明 i64 越界、非法 raw 被吞、送转注入旧数字、外部配置吞本地 raw、seed/Simulation 及显式操作缺行为。preset 使用可运行的返回模板 stub，红为金额未采样/范围未拒绝，不是缺模块或语法红。缺包首次环境失败另留在 red-preferences 中，未冒充业务红。

历史红/早期绿实际 Node v25.8.2；按协调要求最终绿显式改用根 `.tmp/node24-env/node` v24.18.0，没有回写历史版本。最终运行：

```sh
/data1/baiyifan/workplace/stock_market_game/.tmp/node24-env/node scripts/run-with-deadline.mjs 10000 -- /data1/baiyifan/workplace/stock_market_game/.tmp/node24-env/node --test --test-timeout=10000 --test-concurrency=4 apps/web/src/config/seed-draft.test.ts apps/web/src/config/company-initial-preset.test.ts apps/web/src/components/company/CompanyPreferencesInput.test.ts apps/web/src/components/company/CompanySystemInput.test.ts apps/web/src/save/schema/setup-company-options.test.ts
```

`green-node24.stdout.log`：28 tests 通过，0 fail，约1.29秒，file concurrency=4；case 与整进程树均10000ms。范围 lint exit 0；窄 TypeScript --strict/--noUnusedLocals/--noUnusedParameters exit 0，见 `lint-node24.*` 与 `types-node24-final.*`。两个早期窄 TS 环境调用错误（根目录找不到 @types/node、TS6 需要 --ignoreConfig）如实保留，后续改到 apps/web 并显式 --ignoreConfig 通过。

组件 SSR 与真实 React 元素事件/rerender 均执行。交互使用既有 `memoryHook` dispatcher，只证明 state/真实事件与回调，不模拟 DOM、浏览器 commit 或端到端创建。未宣称浏览器验证。

## 待门禁

由未实施 Sol 只读复核完整 `3984e141..e7850b5c` diff：大 A 语义及依据、最小必要性、边界与跨层一致性；发现修复后重审。未通过此门禁前不宣称完成。

TDD 纪律限制：`changeCompanyPreferencesDraft` 的现行 parser 复用胶水与对应身份/保值断言在同一编辑操作加入，没有单独留存该 helper 的先红现场；不能把相邻 seed 业务红冒称为此 helper 独立红证。其余上述专门业务红均有实际先红 snapshot。`git diff --check` 对原始 Node 红日志会报告原生 AssertionError 空白行的尾空格；日志未裁剪，源码 diff 检查通过。
