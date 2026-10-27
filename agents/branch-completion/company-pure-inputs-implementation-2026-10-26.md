# B4f：TaxModeInput / ParValueInput 纯组件恢复

- 实施日期：2026-10-25 开始，2026-10-26 完成冻结验证（Asia/Shanghai）。
- 基线：`b172c1ab`；来源：`18725d07` 的同名四文件。
- 工作树：`.worktree/company-pure-inputs`；分支：`feat/web-company-pure-inputs`。
- 范围：恢复两项受控输入组件及测试；不接入 App，不改 defaults、host、save parser、generated 类型或依赖。旧组件没有 `disabled` prop，本批不新增；将来由实际宿主接线决定控制门禁。

## 契约与必要修复

- TaxModeInput 遵循 ADR-0040：简税、个人差别化、不扣税三选一。简税明确为游戏简化；机构/企业差别计税未实现；不扣税免卖出印花税但佣金/过户费照付。没有修改实际计税管线。
- 百分比先校验十进制原文的 0–100% 范围，再按 1bp 四舍五入。保留旧科学记数形式；拒绝 `0x10`、`0b10`、`0o10` 等非十进制。修复 `1.005%` 浮点误舍入，并防止 `100.00000000000000001%` 先被近似成 100%。非法文本保留并显示当前已设比例。
- ParValueInput 按元录入、按 Money 规范分字符串回调；只有失焦或 Enter 才提交。非法失焦/Enter 保留原文并持续显示错误，修复旧实现回填草稿而隐藏错误的问题。正数与 i64 上限检查保持当前 Money 契约，不推断依赖总股本的 setup 条件。
- 两组件使用 `type="text"` / `inputMode="decimal"`，让非法编辑原文可保留，并允许百分比按既定 bp 规则舍入。
- 面值默认 1 元/股与注册资本=面值×总股本仅描述 N2a 游戏装配约定，不声称所有现实 A 股证券均为 1 元面值。依据 ADR-0035、ADR-0040、`agents/company-system/auto-registry.md`，以及历史 `18725d07:docs/trading-rules.md` 的原有范围登记；本批没有重实现交易制度，也没有把历史引擎覆盖宣称为当前整体验收。

## TDD 与冻结验证

- 先恢复旧组件基线，再加入交互/边界测试；`red.log` 中 Par 非法失焦实际回填 `1.00`，测试因行为断言失败。`red-boundary.log` 继续固定 Money 越界和正数门禁。
- 独立预审发现 Tax 文本输入的非十进制入口及半 bp 浮点误差后，先补测试；`red-review-tax.log` 复现 `0x10 → 1600bp`、`1.005% → 100bp`、精细越界被接受。随后改为精确十进制解析。
- `red-review-dom-settled.log` 是真实 Chromium 的越界原文被替换成 `100` 的红灯。dispatcher 测试只证明真实 handler 与 effect 的状态流转；实际 DOM 由本目录 `company-pure-inputs-dom.test.mjs` 独立验证。
- DOM fixture 只对两组件构建内存 bundle，两个独立页面并发执行实际 input、Enter、焦点离开、radio 切换和外部 prop 更新；断言已设金额/比例、回调次数、保留文本及 role=alert。它不证明 App、生产构建或完整游戏 E2E。
- 曾发生 DOM 断言通过但整命令超时；`green-final-dom.log` 等旧日志仍保留且判失败。阶段诊断发现 Chromium graceful `browser.close` 可花约 6 秒，最终改为 `launchServer` 后在唯一 test 的 finally 中等待 `browserServer.kill`、关闭 client/HTTP 连接。没有放宽十秒门禁或强制将进程退出伪装成通过。
- 最终四条独立命令并发执行；unit 文件并发 2、DOM 页面并发 2、lint threads=2。所有 Node case timeout=10000ms，整命令均经进程外 `run-with-deadline.mjs 10000`。tsc 使用工具自身的单进程能力，与其它独立检查并行。

- `unit`：exit 0，整命令 1.664s；日志 `frozen-unit-2026-10-26.log`。
- `dom`：exit 0，整命令 2.695s；日志 `frozen-dom-2026-10-26.log`。
- `lint`：exit 0，整命令 0.254s；日志 `frozen-lint-2026-10-26.log`。
- `tsc`：exit 0，整命令 1.436s；日志 `frozen-tsc-2026-10-26.log`。

- unit：14/14；DOM：1/1 case、两个真实页面并发。局部 TypeScript 检查和四文件 oxlint 通过；没有执行全量测试、完整构建或实际宿主接线验收。
- 原件目录：`/data1/baiyifan/workplace/stock_market_game/.tmp/branch-completion/company-pure-inputs`（会话期 `.tmp` 证据，不声称已入库）。
- 收据：`frozen-validation-2026-10-26.json`，SHA-256 `233d7a14d169c23ba6cc3dd514c08c0e16a92c7bb591d1af00b976fea97e47f0`；收据绑定完整命令、前后源码 SHA、实际日志 SHA 和 exit code。前后源码内容一致。

## 冻结源码 SHA-256

- `agents/branch-completion/company-pure-inputs-dom.test.mjs`：`6e15e94b33223c1b04dfeebc99164aed252590e38f2598d64887f200edf557f6`。
- `apps/web/src/components/ParValueInput.test.ts`：`833833c034132d4e2f9bc3e5fed21404515e2a265f8e4e040c8cea80c2b5ba2a`。
- `apps/web/src/components/ParValueInput.tsx`：`4dd97227f6254bc896baac09ceb1610e6e8123f8ea9e0de9fe5a3ad5fd3b43e3`。
- `apps/web/src/components/TaxModeInput.test.ts`：`7a75a737d18bfa21ab0eca925cc8480641400b515450ebfcc7fd6d1de7b55076`。
- `apps/web/src/components/TaxModeInput.tsx`：`1a527b4aed707922cac559da27b1ea43e7e31328b47e28c814fb539a322b3ce7`。

## 独立复核状态

已交由未实施者 `sol61_review_legacy` 复核完整 diff；其预审有效发现已加入红绿测试修复。终审结论由独立复核者另行记录，本实施记录不自行宣称门禁通过。
