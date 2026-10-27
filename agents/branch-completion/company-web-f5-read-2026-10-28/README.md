# F5 公司只读 App 与宿主接线（2026-10-28）

## 范围与当前边界

本片基于 `c10cf187c9f498dbabde9defc33042147a79aefe`，只接已有六项公司查询。`EngineHost` 的六个方法不接受账户参数；公司代码只经当前实际 `CatalogResolver` 转成 `CompanyId`。三宿主的成功结果消费实际身份 `owner`，按当前 Rust 的 `kind` 校验原领域视图，再复用已存在的严格 parser。Worker/Tauri 固定玩家为 `0`，Remote 由服务端当前身份决定本人，不用股票编号或默认账户推断。

App 的桌面与移动公司页面均通过 `ConnectedCompanyPanel` 挂载现有 `CompanyContractPanel` 和 `DividendTaxPanel`。查询上下文包含宿主实例、membership owner、snapshot/company generation 与自然日；membership 未就绪、需要重新加入或 generation 不一致时不查询本人。切换上下文会撤销旧成功和错误。界面不增加提案、批准、认购或结算按钮，不改变 `action_readiness.ready`。

本片的 `SOURCE_ONLY` 审查不表示 Rust 查询注册已编译、六项真实非空 producer 已验、Remote membership 生产广播已完成或 browser 已通过。真实 Rust 注册由 `/root/sol_company_queries_28` 在独占树负责，实际 Rust fixture 必须随后绑定并独立复核。R4、税务真实结算与公司命令的未交范围继续保留；不能据只读 UI 展示推断已付款或已划税。

## TDD 与失败原件

| 原件 | 真实含义 |
| --- | --- |
| `red-v1/` | 内存 DOM 不支持公司选择器，两个基础设施错误；不算业务红。 |
| `red-v2/` | 修正测试输入后，两项缺失契约读面的断言分别真实失败。 |
| `transport-red-v1/` | 新宿主适配模块的显式未实现 stub；四项合同测试失败，不冒已有 Rust 行为红。 |
| `host-red-v1/` | 三个真实 TypeScript host 各三项查询因方法尚未注册失败；transport 使用受控测试端。 |
| `local-owner-red/` | Worker/Tauri 接受 `owner=9` 且回执账户也为 `9` 的两个真实缺口。 |
| `tauri-error-red/` | Tauri 对象拒绝不是 `Error`，界面会失去结构化详情。 |
| `issuer-red/` | 首个发行人不匹配断言失败；同 case 后续能力摘要断言未执行。 |
| `issuer-red-v2/` | 权益明细和能力摘要拆成独立 case，二者均因接受跨公司权益而真实失败。 |

修复没有削弱既有断言。发行关系修复还检查合法配对可读，不以全部拒绝冒严格校验。

## 验证与限制

- `green-v1/`、`transport-green-v1/`、`green-v2/`、`issuer-green/` 分别保存对应阶段日志，不把重复运行数量相加当全量分母。新增核心闭包为 decoder 6 项、三 host 15 项、React 读面与既有上下文 16 项、实际 Worker listener 1 项；另有原 SSR 面板短门。
- 普通 Node 命令均使用真实 main 的绝对 `run-with-deadline.mjs 10000`，并设置 `--test-timeout=10000`。独立 suite 最多六进程并行；没有运行 Cargo 或创建新 target。
- `static-v2/receipt.json` 记录十四项产品指纹、四个完整 strict app 根文件分片，以及新范围 lint。发行关系修复后的十四项指纹位于 `issuer-green/receipt.json`；相对前份仅两个 decoder 源/测试变化。
- 本片新增 lint 问题均已修复。完整范围 lint 仍有 `App.tsx` 三条继承的 `react-hooks/exhaustive-deps` warning；`lint-v2/App-current.stdout` 与 `App-head.stdout` 用同配置复现同三处，不能宣称完整范围 lint 退出 0。
- W1 冻结 `9a1cf4ea` 不含本片。`worker-query-narrow-v1.patch` 已对该树实际执行只读 `git apply --check` 并退出 0；没有写入 W1 delivery，也不据此宣称 W1 构建或 browser 已通过。

独立完整审查及发现修复记录见 [app-read-independent-review.md](app-read-independent-review.md)。产品提交须等待未实施者对最终指纹及发行关系修复复核。

## 真实 Rust wire 消费追加批次

`e2b831fd` 之后追加测试原始 Rust JSON 的消费，生产实现未变化。三类 TypeScript host 消费 18 份真实结果，Tauri 消费实际拒绝；真实 React/Redux 的 ConnectedCompanyPanel 经 Remote host 展示同局六种 JSON。来源、失败原件、最终检查与准确限制见 [real-wire-v1/README.md](real-wire-v1/README.md)。本批 native fixture 消费不改变原 `SOURCE_ONLY` 审查的历史范围，也不代表 WASM JavaScript export、browser、F6 或真实投资者结算完成。
