import { it } from "node:test";
import { runFailingExportCase } from "./rust-test-case-supervisor-export-failure-fixture.mjs";

// POSIX shebang 与 /proc token 清理仅在本测试的 Linux 运行形态中可验证。
it("真实导出 case failure 后先终止孙进程再清理目录", {
  timeout: 10000,
  skip: process.platform !== "linux" ? "该用例依赖 POSIX shebang 与 /proc token 清理" : false,
}, async () => { await runFailingExportCase("failure"); });
