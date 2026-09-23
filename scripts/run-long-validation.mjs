#!/usr/bin/env node
import path from "node:path";
import { fileURLToPath } from "node:url";

import { LONG_VALIDATION_MAX_MS, parseArgs, runBoundedCommand } from "./run-with-deadline.mjs";

export async function main(argv) {
  const options = parseArgs(argv);
  if (options.timeoutMs > LONG_VALIDATION_MAX_MS) {
    throw new Error(`long validation deadline must be within ${LONG_VALIDATION_MAX_MS}ms; got ${options.timeoutMs}`);
  }
  await runBoundedCommand({ ...options, cleanupReserveMs: 1 });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
