# wasm 线程战役 P0 前置批——Pages SW 隔离链恢复台账

- 分支：`feat/wasm-p0-web-recovery`（基 main@a6911c52），worktree
  `.worktree/wasm-p0`
- 依据：`.tmp/wasm-thread-abstraction-design-input.md` §8.5 恢复清单（7 项）
  ——Sol 四审实证批 9 A10 "已实现已 smoke" 失实后的独立前置批
- 恢复基线：`c18c08c8^`（main 上的 A10 删除前置提交；与清单所引
  `39d0737a^`/`b478457f^` 同 blob，已核验三处 hash 一致）

## 提交与清单映射（6 提交）

| 提交 | 清单项 | 内容 |
|---|---|---|
| e27b24ea | 1/2/3 | pages-isolation.js（SW 本体）+ pages-isolation.ts（引导，显式失败面四类）+ main.tsx pages 模式引导块 + pages-isolation.test.mjs（4 用例） |
| c93137e4 | 4 | package-static-web 产物要求 + fixture 断言 |
| 4ee396be | 6 | vite COOP/COEP 插件（dev+preview）+ startup-policy crossOriginIsolated/SAB 门禁 + 两测试文件 |
| 836eb7d3 | 7 | check-web-release-wasm 并入串行 glue 契约断言；删 package.json 悬空 wasm:check-threading 别名 |
| 7a94c401 | 5 | smoke-pages 三断言 + worker 隔离断言 + wasmThreads/isolationHeadersAtOrigin 日志字段 + :89 尾日志勘正 |
| e6ff5ca2 | — | docs/tech-stack.md 与现状同步勘正 |

## 红证记录（TDD）

- 清单 1-3：恢复测试先行 4 用例全红（ENOENT 缺实现文件）→ 恢复后 4/4 绿
- 清单 4：仅恢复产物要求红（ENOENT 缺 fixture 文件）→ fixture 恢复 2/2 绿
- 清单 6：恢复两测试先行门禁用例红（缺 crossOriginIsolated/sharedArrayBuffer
  字段）→ 实现恢复 6/6 绿
- 清单 7：4 个 glue 用例先行 3 拒绝用例红 → 实现 9/9 绿；真实新产物串行 glue 过检
- 清单 5：dist 去 SW 副本红（SW 注册失败→引导显式失败→heading 超时）→
  完整包两入口 bootstrap ready+隔离/SAB/标记/worker 隔离断言全过

## 断言口径裁定（当前串行 wasm 无 SAB 下的确定行为）

1. `crossOriginIsolated && SAB` / worker 内 `crossOriginIsolated`：测的是
   浏览器隔离环境（SW/vite 插件注 COOP/COEP），与引擎线程面无关——串行下
   依然可绿，口径确定（smoke 实测 true）。
2. `wasmThreads` 日志字段：串行=1（单 WASM Worker）如实登记；Rayon 池数
   待真线程批。**清单未定义无线程时降级值，本批裁定 1 并上报。**
3. Rayon 池计数（workers+1）与 init `threads` 注入**不复刻**：池架构已退役
   （未来=自研线程门面），真线程批按新池形态重写。
4. 串行 glue 契约=纯否定断言（实测新产物 glue 0 处 `WebAssembly.Memory`/
   shared/线程栈）：不得 JS 侧建 Memory、不得 shared:true/thread_stack_size。
   老脚本的存在性片段（`new WebAssembly.Memory({` 等）是**线程时代**契约，
   串行产物本无这些片段（初版误设，已修正）。dist 中 glue 被 vite 打进
   chunk 无独立文件——断言作用于 wasm-pack 产物目录调用点（frontend-build-
   plan 既有两处调用之一），目录无 glue 文件则不检（作用面如实登记）。
   §7.3.2 时间 import 白名单属未来时钟门面，本批不实施。
5. startup-policy 门禁恢复=本地 wasm 启动要求隔离环境；三条伺服路径全覆盖
  （dev/preview=vite 插件注头实测 COOP/COEP 在；Pages 静态包=SW 注头，
   smoke 实测 crossOriginIsolated:true；桌面 tauri 不读该探测）。

## 验证矩阵（2026-10-20，机器负载 ~240）

| 项 | 结果 |
|---|---|
| wasm32 check（RUSTFLAGS="" 稳定链，1.96.1） | 绿（web-wasm Finished） |
| web 构建（vite build pages/e2e/default） | 绿（tsc -b 除外，见下） |
| scripts 测试 34 文件逐一并行 | 32 绿；build-targets=worktree 路径含 "wasm" 伪影（干净路径 worktree 22/22 绿）；escrow-performance-harness=main 既有红（纯净 main 对照 10/10 同败） |
| web 测试批（193 文件/8 shard） | 全绿（需本机已生成 apps/desktop/src-tauri/gen/schemas——gitignore 产物，裸 worktree 需拷入） |
| smoke-pages | 恢复断言面全绿；整体败于 .app-root（见下，main 既有） |
| oxlint | 本批文件零告警（全 app 27 条既有告警不变） |
| tsc -b | main 既有 31-32 错（company/report 域，W3 合并遗留）；本批文件零错、总数 32→31 |

## 本批发现的 main 既有断裂（如实上报，本批未修）

1. **smoke-pages .app-root 败（main 既有）**：`wasm-worker.ts:133`
   `postIngressBridge` 把非共享 `exports.memory` postMessage 回主线程 →
   DataCloneError "#<Memory> could not be cloned"（非 shared Memory 不可
   结构化克隆）。ingress 桥为 W3 批新面（d803b937/e24c349d），与串行 wasm
   不兼容。**纯净 main 对照（同 wasm-pkg、main 版 smoke）同败**，证据闭环。
   属宿主域，留主实施批处置（真线程批恢复 shared memory 后该面自然回归，
   但串行期本地 wasm 起局即崩——影响所有本地会话启动，需主批/宿主Owner裁决）。
2. tsc -b 红（company/report 域 31-32 错）→ `pnpm build`/e2e webServer 的
   tsc 段在 main 即红；本批验证用 vite build 直跑绕开，未新增错误。
3. scripts 批 escrow-performance-harness 红（纯净 main 同败）。
4. 悬空 `wasm:check-threading` 别名（已随 836eb7d3 清理）。

## 环境备注

- worktree 目录名含 "wasm" 触发 build-targets 计划纯度正则误命中（该测试
  断言计划 JSON 不含 /pnpm|wasm|tauri|web-ui/，绝对路径进 JSON）——复跑
  需干净路径（.worktree/p0-checkout 22/22 绿）。
- 裸 worktree 跑 scripts/web 批需预置：pnpm install、apps/web/wasm-pkg
  （build-matrix 前置）、apps/desktop/src-tauri/gen/schemas（file-target-
  permissions 前置）。
- btrfs+高负载下 git 检出极慢：`git -c checkout.workers=16 worktree add`
  由 2h+ 降到 ~3min。
