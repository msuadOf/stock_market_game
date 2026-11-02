import { readFileSync } from "node:fs";

export function readProcessBirth(pid) {
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    if (fields.length < 20 || !/^\d+$/.test(fields[19])) throw new Error(`cannot confirm process ${pid}: malformed stat identity`);
    return fields[19];
  } catch (error) {
    if (error.code === "ENOENT" || error.code === "ESRCH") return undefined;
    throw error;
  }
}

export function recordProcessIdentity(pid) {
  const birth = readProcessBirth(pid);
  if (birth === undefined) throw new Error(`cannot record process identity for missing PID ${pid}`);
  return { pid, birth };
}

export function killRecordedProcesses(records) {
  const results = [];
  for (const record of records) {
    if (!Number.isSafeInteger(record?.pid) || record.pid <= 0 || typeof record.birth !== "string" || !/^\d+$/.test(record.birth)) {
      results.push({ ...record, status: "invalid" });
      continue;
    }
    const currentBirth = readProcessBirth(record.pid);
    if (currentBirth === undefined) { results.push({ ...record, status: "gone" }); continue; }
    if (currentBirth !== record.birth) { results.push({ ...record, status: "mismatch", currentBirth }); continue; }
    try { process.kill(record.pid, "SIGKILL"); results.push({ ...record, status: "killed" }); }
    catch (error) {
      if (error.code === "ESRCH") results.push({ ...record, status: "gone" });
      else throw error;
    }
  }
  return results;
}
