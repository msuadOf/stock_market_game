import { readFile } from "node:fs/promises";
import init, * as wasm from "../../../apps/web/wasm-pkg/web_wasm.js";
import { normalizePublicReportById, normalizePublicReportPage } from "../../../apps/web/src/host/serde-normalize.ts";

await init(await readFile(new URL("../../../apps/web/wasm-pkg/web_wasm_bg.wasm", import.meta.url)));

const { DEFAULT_SETUP } = await import("../../../apps/web/src/config/defaults.ts");
const setup = DEFAULT_SETUP;

let handle;
try {
  handle = wasm.create_session(setup, 1n);
} catch (error) {
  throw new Error(`2030 session creation failed: ${error instanceof Error ? error.message : String(error)}`);
}

const reports = wasm.public_report_page(handle, { company_id: "C-600101", cursor: null, page_size: 20 });
if (reports.reports.length === 0) throw new Error("2030 session had no published C-600101 report");
const report = reports.reports[0];
for (const dateField of ["period", "approved_date", "published_date"]) {
  console.log(`page ${dateField} type=${typeof report[dateField]} value=${JSON.stringify(report[dateField])}`);
  if (typeof report[dateField] !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(report[dateField])) {
    throw new Error(`page ${dateField} must be a canonical ISO civil date, received ${String(report[dateField])}`);
  }
}
if (typeof report.id !== "string" || typeof report.accounting.total_assets !== "string") {
  throw new Error("public report lost opaque ID or exact decimal string");
}
const reportById = wasm.public_report_by_id(handle, report.id);
for (const dateField of ["period", "approved_date", "published_date"]) {
  console.log(`by-ID ${dateField} type=${typeof reportById[dateField]} value=${JSON.stringify(reportById[dateField])}`);
  if (typeof reportById[dateField] !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(reportById[dateField])) {
    throw new Error(`by-ID ${dateField} must be a canonical ISO civil date, received ${String(reportById[dateField])}`);
  }
}
if (reportById.id !== report.id) throw new Error("report-by-ID did not resolve the page report");
if (!Object.hasOwn(reports, "next_cursor") || reports.next_cursor !== null) {
  throw new Error(`page next_cursor must be own null field, received ${String(reports.next_cursor)}`);
}
console.log(`page next_cursor own=${Object.hasOwn(reports, "next_cursor")} value=${String(reports.next_cursor)}`);
if (!Object.hasOwn(report, "supersedes") || report.supersedes !== null) {
  throw new Error(`page report supersedes must be own null field, received ${String(report.supersedes)}`);
}
console.log(`page supersedes own=${Object.hasOwn(report, "supersedes")} value=${String(report.supersedes)}`);
if (!Object.hasOwn(reportById, "supersedes") || reportById.supersedes !== null) {
  throw new Error(`by-ID report supersedes must be own null field, received ${String(reportById.supersedes)}`);
}
console.log(`by-ID supersedes own=${Object.hasOwn(reportById, "supersedes")} value=${String(reportById.supersedes)}`);
normalizePublicReportPage(reports);
normalizePublicReportById(reportById);
console.log("strict Task-33 public DTO normalizers accepted page and by-ID WASM payloads");
console.log(`public report query resolved ${report.id} with decimal total_assets ${report.accounting.total_assets}`);

let corruptRestoreRejected = false;
try {
  wasm.restore({});
} catch (error) {
  corruptRestoreRejected = true;
  console.log(`corrupt restore rejected: ${error instanceof Error ? error.message : String(error)}`);
}
if (!corruptRestoreRejected) throw new Error("corrupt restore was unexpectedly accepted");

let unknownQueryRejected = false;
try {
  wasm.public_report_page(handle, { company_id: "C-UNKNOWN", cursor: null, page_size: 0 });
} catch (error) {
  unknownQueryRejected = true;
  console.log(`invalid public query rejected: ${error instanceof Error ? error.message : String(error)}`);
}
if (!unknownQueryRejected) throw new Error("invalid public query was unexpectedly accepted");

const save = wasm.save(handle);
const restored = wasm.restore(save);
const before = wasm.snapshot(handle);
const after = wasm.snapshot(restored);
if (before.seq !== after.seq || before.tick !== after.tick) throw new Error("candidate restore snapshot drifted");
wasm.drop_session(handle);
wasm.drop_session(restored);
console.log("candidate-first restore preserved snapshot sequence and tick");
