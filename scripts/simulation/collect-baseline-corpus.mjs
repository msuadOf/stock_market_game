import { collect } from "./escrow/collector.mjs";
import { parseArgs } from "./escrow/options.mjs";

try {
  const options = parseArgs(process.argv.slice(2));
  if (options.mode === "help") console.log("usage: node scripts/simulation/collect-baseline-corpus.mjs <collect|verify> <new-evidence-directory>");
  else console.log(JSON.stringify(await collect(process.cwd(), options)));
} catch (error) {
  console.error(error instanceof Error ? error.stack : String(error));
  process.exitCode = 1;
}
