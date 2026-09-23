# 测试策略 (Testing)

> TDD 是本项目的铁律。本文档说明**怎么做** TDD，以及不同层级的测试边界。
> 配套：[`principles.md`](principles.md) 原则 1、[`architecture.md`](architecture.md)。

---

## 1. TDD 工作循环

```
┌─────────────────────────────────────────────────┐
│  红 (Red)                                        │
│  写一个测试，描述目标行为。运行它，确认它失败。   │
│  关键：要确认它因"正确的原因"失败（而非编译错）。 │
└─────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────┐
│  绿 (Green)                                      │
│  写最少的实现代码，让测试通过。不多做、不预判。   │
└─────────────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────┐
│  重构 (Refactor)                                 │
│  在测试保护下清理代码。测试必须保持全绿。         │
└─────────────────────────────────────────────────┘
                      │
                      ▼
        一个红绿循环 = 一个 git 提交
```

### 红灯要问自己的三个问题

1. 它失败了吗？（如果直接通过，说明测试没意义或实现已存在。）
2. 它因**正确的原因**失败吗？（断言没过，而非编译错误 / import 错误。）
3. 这个测试真的描述了**期望行为**，还是在描述"我刚好写的实现"？

## 2. 测试金字塔

```
            ┌──────────┐
            │   E2E    │   少量：真实用户流程（开浏览器跑完整游戏流程）
            └──────────┘
          ┌──────────────┐
          │ Integration  │   适中：多个模块协作（engine + 存储层）
          └──────────────┘
       ┌────────────────────┐
       │      Unit          │   大量：纯函数、核心逻辑（撮合、价格模型）
       └────────────────────┘
```

**比例目标：** 单元 ≫ 集成 > E2E。核心 engine 层应有接近 100% 的单元覆盖。

## 3. 按层级的测试约定

| 层 | 测试重点 | 工具（建议） | 备注 |
|----|----------|--------------|------|
| `engine`（核心逻辑） | 规则正确性、边界、不变量 | Rust `cargo test` | **重中之重**，纯逻辑与状态机测试 |
| `web`（前端） | 输入规则、宿主契约、状态流转 | Node test runner + TypeScript | 测行为与协议，不用源码快照替代行为测试 |
| `web`（浏览器 E2E） | 桌面/移动真实交互、可访问性、存读档 | Playwright + Chromium | production preview，少量关键旅程 |
| `server`（后端，可选） | API 契约、并发、错误响应 | Rust 集成测试 + Tokio | router、actor 与真实 WebSocket 链路 |
| `desktop`（Tauri） | 壳与核心的集成 | Rust 测试 + 前端宿主契约测试 | 薄壳，重点测桥接与存档原子性 |

完整回归从仓库根目录运行：

```bash
bash scripts/corepack-pnpm.sh test
bash scripts/corepack-pnpm.sh lint
bash scripts/corepack-pnpm.sh build
```

Rust 单独验证可用 `cargo test --workspace`；前端单独验证可用
`bash scripts/corepack-pnpm.sh --filter web test`、
`bash scripts/corepack-pnpm.sh --filter web lint` 和
`bash scripts/corepack-pnpm.sh --filter web build`。该帮助脚本只调用仓库 `packageManager`
固定版本的 Corepack pnpm；若 Node 或 Corepack 缺失，会明确说明需要修复的前置条件，绝不回退到
其他包管理器。普通开发命令统一使用跨平台的 `corepack pnpm`；若本机 Node 版本不明，可使用该
POSIX 帮助脚本进行 `.nvmrc` 诊断。CI 或隔离环境可将 `NODE_BIN` 设置为 Node 安装目录或其
`node` 可执行文件的绝对路径。Rust serde 边界变化后运行 `bash scripts/corepack-pnpm.sh types:generate`；
`bash scripts/corepack-pnpm.sh types:check` 会生成并检查
`apps/web/src/types/generated/` 是否与仓库一致。
浏览器主链路使用 `bash scripts/corepack-pnpm.sh test:e2e`；命令会构建前端并启动隔离头完整的 Vite preview。

### 运行时限与并行约束

- 普通自动化测试的单命令、单 case 硬上限都是 10 秒；超过时必须缩小代表性 fixture、拆分测试或去掉重复准备工作。
- Node 普通测试同时使用 `--test-timeout=10000` 的 case 门禁和
  `scripts/run-with-deadline.mjs 10000 -- <command>` 的整命令进程树门禁；不允许通过参数把这一上限调大。
- Web 普通测试统一由 `scripts/run-web-tests.mjs` 直接启动当前满足 `>=24.18.0` 的 Node，
  不经过 Corepack/pnpm 的动态导入启动链。runner 递归发现 `apps/web/src/**/*.test.ts(x)`，拒绝零测试或重复路径，按 CPU 预算最多拆成 8 个真实 Node shard；整个批次和每个 child 均不超过 10000ms，任一 shard 失败会终止在途 siblings。`apps/web` 的 package `test` script 与完整回归共用该入口。
- 确有必要的长测试，其每个 child 进程和整个批次 wall-clock 均设 300000ms 硬上限；批次截止时间覆盖结果校验、manifest 发布、进程树终止与临时文件清理，不能只给 child 设置 timeout。K7 固定把前 299000ms 用于执行/校验/发布，最后 1000ms 只用于 kill、等待 close 和删除 staged 文件。runner 内部第二截止负责异步清理收敛；正式命令同时由进程外 deadline supervisor 约束总 wall，不能只依赖被测 Node 进程自己的事件循环计时器。
- 可并行测试必须使用真实多进程或多线程，资源预算需显式记录并避免超卖；长测试不得单核串跑。
- 构建耗时与测试执行耗时分开记录。冷编译超过 10 秒不应伪装成测试耗时；必要构建同样使用多核并受 5 分钟硬上限约束。
- 根目录完整回归是明确的两阶段长验收。`scripts/run-full-regression.mjs build --inventory <workspace-.tmp-path>`
  在独立 300000ms 硬期限内完成多核冷构建，从 Cargo JSON 取得 test executables，并把源码指纹、产物相对路径、大小与 SHA-256 原子发布到 workspace `.tmp` 下的密封 inventory；构建前后源码指纹不同则拒绝发布。
- `scripts/run-full-regression.mjs execute --inventory <workspace-.tmp-path>` 另起独立 300000ms
  硬期限；先验证 inventory 自校验、当前源码指纹和每个二进制哈希，再按总 CPU 预算最多并发 8 个预构建 Rust test binaries（128 CPU 时每 binary 为 12 个 harness threads + 4 个 Rayon threads，总预算 128）。任一 binary 失败即终止在途 siblings；同一执行期限随后覆盖 workspace doctests 和上述多进程 Web 普通测试入口，其中 Web 批次及其 child 仍受 10000ms 门禁。执行结束再次验证源码未漂移。
- 无参数的 `scripts/run-full-regression.mjs` 只负责依次启动上述两个进程外阶段；构建耗时不挤占执行阶段，但任何一个长阶段都不得超过 5 分钟。doctest 保留 rustdoc 固有的 snippet compilation，不冒充预构建 binary 执行。
- K7 的 `before` 语料已密封，只保留可测的解析兼容层；CLI 明确拒绝重跑，不将它当作当前长验收。
- K7 当前验收先在独立 300000ms 构建 deadline 内构建一次 fixture，再由 after/sensitivity 在各自的 300000ms 总 deadline 内直接执行预构建二进制；每个 K7 批次遵循 299000ms 执行/发布 + 1000ms 收尾预留。二进制哈希和编译时嵌入的源指纹必须与当前密封源一致，否则在启动矩阵前失败。
- 正式长验收的单阶段进程外门禁统一使用 `scripts/run-long-validation.mjs 300000 -- <command>` 或等价的 runner 内进程外门禁；普通测试不得借此放宽 10 秒门禁。

## 4. 什么必须有测试

- ✅ 任何公共函数 / API
- ✅ 核心业务规则：撮合、价格波动、组合盈亏、手续费、熔断
- ✅ 边界情况：资金为 0、持仓为空、价格为负（应被拒绝）、整数溢出
- ✅ 错误路径：存档损坏、网络失败、非法输入
- ✅ 不变量：资产守恒（钱不会凭空产生/消失，除非按规则）

## 5. 什么可以不写测试（或少写）

- 纯展示组件的无逻辑样式（视觉回归可另用工具）
- 第三方库的包装（除非包装里有逻辑）
- 配置文件、类型定义

> 但要警惕"这不用测"成为逃避测试的借口。当犹豫时，**写测试**。

## 6. 测试代码本身的质量

- **Arrange-Act-Assert** 结构清晰。
- 一个测试只断言一件事（一个概念）。
- 测试名描述行为：`it('拒绝负数的买入数量', ...)` 而非 `it('test1', ...)`。
- 避免测试间共享可变状态。
- **测试里的魔法数字要有名字**：`const INITIAL_CASH = 100_000` 而非裸 `100000`。

## 7. 测试与"诚实反馈"原则

- 测试失败 = 诚实信号。**绝不为让它通过而弱化断言、删测试、或改测试去迁就错误实现。**
- 如果发现需求本身错了（测试写错了），明确说明并修正测试——但要说明 **why**。
- CI 必须要求测试全绿才能合并。

## 8. 后续质量工作

- [ ] 覆盖率门槛（建议 engine ≥ 90%，整体 ≥ 70%，仅作信号不作强约束）
- [x] 引入浏览器 E2E，覆盖桌面/移动关键交互和本地 WASM 存读档
- [ ] 扩展浏览器 E2E 到远程/Tauri 宿主与需要跨交易日的撤单、恢复旅程
- [ ] 将仍依赖源码文本匹配的移动端视觉契约测试迁移为组件行为测试
