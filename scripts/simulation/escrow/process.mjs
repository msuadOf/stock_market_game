import { spawn } from "node:child_process";

export function run(program, args, options = {}) {
  const { cwd, signal, timeout = 120_000 } = options;
  return new Promise((resolve, reject) => {
    const child = spawn(program, args, {
      cwd, detached: true, stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, GIT_MASTER: "1", RAYON_NUM_THREADS: "1", CARGO_TERM_COLOR: "never" },
    });
    let stdout = "";
    let stderr = "";
    let failure;
    const terminate = (reason) => {
      failure = new Error(`${program}: ${reason}`);
      if (child.pid) {
        try { process.kill(-child.pid, "SIGKILL"); }
        catch (error) { if (error.code !== "ESRCH") failure = error; }
      }
    };
    const abort = () => terminate("interrupt/abort");
    const timer = setTimeout(() => terminate(`timeout ${timeout}ms`), timeout);
    signal?.addEventListener("abort", abort, { once: true });
    if (signal?.aborted) abort();
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", (error) => { failure = error; });
    child.on("close", (code, killed) => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", abort);
      if (failure) reject(failure);
      else if (code !== 0) reject(new Error(`${program} exit ${code} signal ${killed}: ${stderr}\n${stdout}`));
      else resolve({ stdout, stderr, code, command: [program, ...args] });
    });
  });
}

export async function git(root, args) {
  return (await run("git", args, { cwd: root })).stdout;
}
