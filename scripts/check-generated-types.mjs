import { execFileSync } from "node:child_process";

const generatedDirectory = "apps/web/src/types/generated";
const status = execFileSync(
  "git",
  ["status", "--porcelain", "--untracked-files=all", "--", generatedDirectory],
  { encoding: "utf8" },
).trim();

if (status) {
  console.error(
    `Rust → TypeScript bindings are stale or untracked:\n${status}\n` +
      "Run `pnpm types:generate` and commit the generated files.",
  );
  process.exitCode = 1;
}
