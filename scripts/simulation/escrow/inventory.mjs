import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { sha256 } from "./closure.mjs";

export function verifyInventory(expected, actual) {
  if (JSON.stringify(expected) !== JSON.stringify(actual)) throw new Error("inventory assertion occurrence/anchor mismatch");
}

const affected = new Map([
  ["sell_order_is_rejected_when_cash_cannot_cover_fee_shortfall", "E9-B_SELL_ACCEPTANCE_FLIP"],
  ["sell_order_reserves_fees_for_a_possible_small_partial_fill", "E9-B_SELL_ACCEPTANCE_FLIP"],
  ["buy_and_sell_orders_share_one_cash_reservation_budget", "E9-A_SELL_CASH_RESERVATION"],
  ["planned_sell_fee_is_reserved_before_a_later_buy", "E9-A_SELL_CASH_RESERVATION"],
]);
const union = /reserved_cash|sell_order_fee_reservation|available_cash|fee_reserve|InsufficientCash|OrderAccepted|IntentRejected|SettlementError|charged|commission|decision_chain|plans_debug|plan_summary|accepted|rejected|rejection|\.plans|diagnostics/;

export function inventoryCandidates(path, source) {
  const matches = [...source.matchAll(/#\[test\]\s*(?:#\[[^\]]+\]\s*)*(?:pub\s+)?fn\s+(\w+)\s*\([^)]*\)[^{]*\{/g)];
  const candidates = [];
  for (const match of matches) {
    let end = match.index + match[0].length;
    let depth = 1;
    let quoted = false;
    let lineComment = false;
    let blockComment = 0;
    let escaped = false;
    while (depth > 0 && end < source.length) {
      const char = source[end++];
      const next = source[end];
      if (lineComment) { if (char === "\n") lineComment = false; continue; }
      if (blockComment) {
        if (char === "/" && next === "*") { blockComment++; end++; }
        else if (char === "*" && next === "/") { blockComment--; end++; }
        continue;
      }
      if (quoted) { if (char === '"' && !escaped) quoted = false; escaped = char === "\\" && !escaped; }
      else if (char === "/" && next === "/") { lineComment = true; end++; }
      else if (char === "/" && next === "*") { blockComment = 1; end++; }
      else if (char === '"') quoted = true;
      else if (char === "{") depth++;
      else if (char === "}") depth--;
    }
    if (depth !== 0) throw new Error(`unbalanced test: ${path}:${match[1]}`);
    const body = source.slice(match.index, end);
    if (!union.test(body) && !/decision_chain|company_decision_session/.test(path)) continue;
    const line = source.slice(0, match.index).split("\n").length;
    const effect = affected.get(match[1]);
    const soft = path.endsWith("company_scenarios/constraints.rs") && /allocated_cash/.test(body) && /fee_reserve/.test(source);
    const assertions = [...body.matchAll(/assert(?:_eq|_ne)?!\s*\(/g)].map((assertion, index) => ({
      identity: `${match[1]}:assertion:${index}`,
      line: line + body.slice(0, assertion.index).split("\n").length - 1,
      hunk_anchor: sha256(body.slice(assertion.index, body.indexOf(";", assertion.index) + 1)),
    }));
    candidates.push({ file: path, symbol: match[1], line, source_sha256: sha256(source), test_sha256: sha256(body), assertions,
      classification: effect && !soft ? "b" : "excluded", effect_id: effect ?? null,
      allowed_transformation: effect ? (effect.startsWith("E9-B") ? "Only sell InsufficientCash rejection and associated absent-order assertions become acceptance; preserve owned shares/T+1/nominal fees" : "Only sell cash reservation and dependent available-cash/capped-intent output changes; buy reservation formula unchanged") : "none; preserve assertion bytes",
      reason: soft ? "soft-budget fee_reserve request input / allocated_cash, not order reservation; sell limit 1100 cents has zero old cash reservation" : effect ? "existing low-price sell fee reservation controls acceptance or subsequent cash budget" : "candidate union hit; no verified #9 affected assertion, preserve unchanged",
      candidate_terms: [...new Set(body.match(new RegExp(union.source, "g")))].sort(),
    });
  }
  return candidates;
}

export async function buildInventory(root, closure) {
  const candidates = [];
  for (const entry of closure.filter((entry) => /^packages\/engine\/(src|tests)\/.*\.rs$/.test(entry.path))) {
    candidates.push(...inventoryCandidates(entry.path, await readFile(join(root, entry.path), "utf8")));
  }
  const included = candidates.filter((entry) => entry.classification === "b");
  for (const symbol of affected.keys()) if (!included.some((entry) => entry.symbol === symbol)) throw new Error(`missing mandatory affected test: ${symbol}`);
  const soft = candidates.filter((entry) => entry.file.endsWith("company_scenarios/constraints.rs") && /soft-budget/.test(entry.reason));
  if (soft.length === 0) throw new Error("missing soft-budget exclusion anchor");
  return { candidates, b_test_inventory: included, explicit_exclusion: { file: "packages/engine/tests/company_scenarios/constraints.rs", lines: [90, 94], entries: soft.map((entry) => entry.symbol) },
    direct_sell_cash_assertion_anchors: [], direct_sell_cash_anchor_verification: "Candidate source review: no accepted resting sell order directly asserts its cash reservation; sell share reservation and rejection-only assertions are not such anchors", enumeration: "All #[test] functions in closure src/tests; balanced-body extraction; reservation/available-cash/fees/acceptance/rejection/decision-output union; each assertion hashed" };
}
