#!/usr/bin/env node
import path from "node:path";
import { fileURLToPath } from "node:url";
import { performance } from "node:perf_hooks";

import { COMMAND_CLEANUP_RESERVE_MAX_MS, LONG_VALIDATION_MAX_MS, parseArgs, runBoundedCommand } from "./run-with-deadline.mjs";

export async function main(argv, { run = runBoundedCommand } = {}) {
  const options = parseArgs(argv);
  if (options.timeoutMs > LONG_VALIDATION_MAX_MS) {
    throw new Error(`long validation deadline must be within ${LONG_VALIDATION_MAX_MS}ms; got ${options.timeoutMs}`);
  }
  await run({ ...options, cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, Math.max(1, Math.floor(options.timeoutMs / 5))) });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const requested = process.argv.slice(2);
  try {
    const options = parseArgs(requested);
    if (options.timeoutMs > LONG_VALIDATION_MAX_MS) throw new Error(`long validation deadline must be within ${LONG_VALIDATION_MAX_MS}ms; got ${options.timeoutMs}`);
    // CLI 的 Node 启动已消耗预算；导入的 main 注入参数契约保持原请求值。
    const remaining = Math.floor(options.timeoutMs - performance.now());
    if (remaining < 2) throw new Error("long validation deadline expired before command spawn");
    requested[0] = String(remaining);
    main(requested).catch((error) => {
      // hard rejection 后真实 child/stdio 可能仍保活；错误写入与强退都使用原预留窗。
      const finalExit = setTimeout(() => process.exit(1), 5);
      process.stderr.write(`${error.message}\n`, () => { clearTimeout(finalExit); process.exit(1); });
    });
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exit(1);
  }
}
