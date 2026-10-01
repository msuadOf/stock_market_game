#!/usr/bin/env node
import path from "node:path";
import { fileURLToPath } from "node:url";

import { COMMAND_CLEANUP_RESERVE_MAX_MS, LONG_VALIDATION_MAX_MS, parseArgs, runBoundedCommand } from "./run-with-deadline.mjs";

export async function main(argv, { run = runBoundedCommand } = {}) {
  const options = parseArgs(argv);
  if (options.timeoutMs > LONG_VALIDATION_MAX_MS) {
    throw new Error(`long validation deadline must be within ${LONG_VALIDATION_MAX_MS}ms; got ${options.timeoutMs}`);
  }
  await run({ ...options, cleanupReserveMs: Math.min(COMMAND_CLEANUP_RESERVE_MAX_MS, Math.max(1, Math.floor(options.timeoutMs / 5))) });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
