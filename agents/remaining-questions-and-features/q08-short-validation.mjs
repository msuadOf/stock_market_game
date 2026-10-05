import { readFile, writeFile, mkdir } from "node:fs/promises";
import { basename } from "node:path";
import { runBoundedCommand } from "../../scripts/run-with-deadline.mjs";

const buildPath = process.argv[2];
if (buildPath === undefined) throw new Error("必须指定实际Cargo JSONL构建日志");
const mode = process.argv[3];
if (mode !== "list" && mode !== "run") throw new Error("模式必须为list或run");
const outputDirectory = `.tmp/checklist-wave4/q08-short-validation/${basename(buildPath, ".jsonl")}`;
await mkdir(outputDirectory, { recursive: true });
const artifacts = (await readFile(buildPath, "utf8")).trim().split("\n").map((line) => JSON.parse(line))
  .filter((artifact) => artifact.reason === "compiler-artifact" && artifact.profile.test && artifact.executable !== null
    && ["engine", "server", "stock_market_game_lib", "web_wasm"].includes(artifact.target.name));
const env = { ...process.env, RAYON_NUM_THREADS: "8" };
const policy = { processConcurrency: 4, rayonThreads: 8, rustTestThreads: 8, commandDeadlineMs: 10000, buildPath };
console.log(JSON.stringify(policy));
await writeFile(`${outputDirectory}/policy.json`, `${JSON.stringify(policy, null, 2)}\n`);
const listed = await Promise.all(artifacts.map(async (artifact) => {
  const listing = await runBoundedCommand({ command: artifact.executable, args: ["--list"], timeoutMs: 10000, captureOutput: true, env });
  await writeFile(`${outputDirectory}/${artifact.target.name}-list.log`, listing.stdout + listing.stderr);
  return { target: artifact.target.name, executable: artifact.executable, tests: listing.stdout.split("\n").filter((line) => line.endsWith(": test")).map((line) => line.slice(0, -6)) };
}));
await writeFile(`${outputDirectory}/artifacts.json`, `${JSON.stringify(listed, null, 2)}\n`);
if (mode === "list") {
  for (const artifact of listed) console.log(artifact.target, artifact.tests.filter((name) => /intraday|personal_trade|single_trade|crossing_buy|query|capabilit|parse|save|protocol/.test(name)).join("\n"));
} else {
  const selection = listed.flatMap((artifact) => artifact.tests.filter((name) =>
    name.startsWith("intraday_average::tests::")
    || name.includes("::personal_trade_tests::")
    || name.includes("continuous_trade_acceptance_tests::crossing_buy_commits_exact_trade")
    || name.endsWith("intraday_average_and_personal_trade_history_are_authenticated_current_generation_queries")
  ).map((name) => ({ target: artifact.target, executable: artifact.executable, name })));
  if (selection.length !== 10) throw new Error(`Q08 exact核心短测必须实际列出10项，当前${selection.length}`);
  const results = [];
  let nextIndex = 0;
  await Promise.all(Array.from({ length: Math.min(4, selection.length) }, async () => {
    for (;;) {
      const index = nextIndex++;
      if (index >= selection.length) return;
      const selected = selection[index];
      const startedAt = performance.now();
      let stdout = "";
      let stderr = "";
      let error = null;
      try {
        await runBoundedCommand({ command: selected.executable, args: ["--exact", selected.name, "--nocapture", "--test-threads=8"], timeoutMs: 10000, env, captureOutput: true, onStdout: (chunk) => { stdout += chunk; }, onStderr: (chunk) => { stderr += chunk; } });
        if (!/test result: ok\. 1 passed; 0 failed;/.test(stdout)) throw new Error("exact短测未证明恰好执行1个成功case");
      } catch (failure) {
        error = failure instanceof Error ? failure.message : String(failure);
      }
      const result = { ...selected, wallMs: performance.now() - startedAt, error };
      await writeFile(`${outputDirectory}/${index}-${selected.target}.log`, stdout + stderr);
      results.push(result);
      console.log(JSON.stringify(result));
    }
  }));
  await writeFile(`${outputDirectory}/results.json`, `${JSON.stringify(results, null, 2)}\n`);
  if (results.some((result) => result.error !== null)) process.exitCode = 1;
}
