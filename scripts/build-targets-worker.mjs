import { executeBuild, parseBuildArgs } from "./build-targets.mjs";

async function run() {
  if (process.env.STOCK_TARGET_BUILD_ID === undefined) throw new Error("use build.sh/build.bat or build-targets.mjs; the internal worker requires the process-tree deadline supervisor.");
  await executeBuild(parseBuildArgs(process.argv.slice(2)), { buildId: process.env.STOCK_TARGET_BUILD_ID, prepared: true });
}

run().catch((error) => {
  process.stderr.write(`[build worker] ${error.message}\n`);
  process.exitCode = 1;
});
