# W1-1 验证计划

W0 最终可信已审输入到位后执行；此文不是完成证据。

1. 在初始 W0 桩上增加真实行为红测；编译或缺 API 不是有效红证。若需要新绑定 API，
   先给最薄 fixture 接口、保留五 import 桩，确认业务断言实际失败，再补最小实现。
2. 运行 `facade-thread-env.test.ts`、`facade-thread-binding.test.ts` 与 W0 直接协议短测，
   Node case 10000ms、整命令复用 `scripts/run-with-deadline.mjs 10000`，显式并发 4。
3. 对 env/binding/测试窄范围执行 TypeScript 与 oxlint；与短测独立的检查并发执行。
4. 记录 W0 基础 commit、完整差异、红绿原日志、实际命令和并发参数；不跑 all/build。
5. 由未实施者复核完整 diff，回答 A 股语义、必要性、边界与跨层一致性；有效发现修复后复查。

独立 ABI 小测已证明 i64/高位 i32 的原生导入行为；它不等于 W1-1 合同行为通过。
W1-1 的 Node Atomics 测试也不等于真实浏览器、glue 接线或 W1-4 通过。
