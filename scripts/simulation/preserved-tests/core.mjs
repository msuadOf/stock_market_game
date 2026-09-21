import { createHash } from "node:crypto";

export const sha256 = (text) => createHash("sha256").update(text).digest("hex");
export const issue = (code, path, symbol, detail) => ({ code, path, symbol, detail });

const isRecord = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
const strings = (value) => Array.isArray(value) && value.every((item) => typeof item === "string");
const requiredStrings = (entry, fields) => isRecord(entry) && fields.every((field) => typeof entry[field] === "string" && entry[field].length > 0);
const hash = (value) => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
const safeRelative = (value) => typeof value === "string" && value.length > 0 && !value.includes("\\") && !value.split("/").some((part) => !part || part === "." || part === "..");

// This is deliberately independent of Rust-source parsing. The inventory is
// policy metadata, not a Rust test file: feeding Markdown/JSON to rustTokens
// makes the verifier's language boundary depend on prose syntax.
export function verifyInventorySchema(inventory) {
  if (!isRecord(inventory)) return [issue("RECLASSIFIED", "preserved-test-inventory.json", null, "metadata root")];
  if (!/^[0-9a-f]{40}$/.test(inventory.baseline ?? "")) return [issue("RECLASSIFIED", "preserved-test-inventory.json", null, "metadata baseline")];
  if (!strings(inventory.forbidden_tokens) || !Array.isArray(inventory.class_a) || !Array.isArray(inventory.class_b) || !Array.isArray(inventory.class_b_changes) || !isRecord(inventory.class_c) || !Array.isArray(inventory.class_c.exact) || !Array.isArray(inventory.class_c.additive)) return [issue("RECLASSIFIED", "preserved-test-inventory.json", null, "metadata top-level schema")];
  const issues = [];
  for (const entry of inventory.class_a) if (!requiredStrings(entry, ["path", "symbol", "baseline_hash", "anchor"]) || !safeRelative(entry.path) || !entry.path.endsWith(".rs")) issues.push(issue("RECLASSIFIED", "preserved-test-inventory.json", null, "metadata class_a entry"));
  for (const entry of inventory.class_b) if (!requiredStrings(entry, ["path", "symbol", "effect_id", "allowed_transformation", "reason"]) || !safeRelative(entry.path) || !entry.path.endsWith(".rs") || !strings(entry.candidate_terms) || !Array.isArray(entry.assertions)) issues.push(issue("RECLASSIFIED", "preserved-test-inventory.json", null, "metadata class_b entry"));
  for (const entry of inventory.class_b_changes) if (!requiredStrings(entry, ["file", "symbol", "effect_id", "allowed_transformation"]) || !safeRelative(entry.file) || !entry.file.endsWith(".rs") || !Array.isArray(entry.assertions)) issues.push(issue("RECLASSIFIED", "preserved-test-inventory.json", null, "metadata class_b_changes entry"));
  for (const entry of inventory.class_c.exact) if (!requiredStrings(entry, ["path", "symbol", "rationale"]) || !safeRelative(entry.path) || !entry.path.endsWith(".rs") || !Array.isArray(entry.hunk_hashes) || !strings(entry.forbidden_fields)) issues.push(issue("RECLASSIFIED", "preserved-test-inventory.json", null, "metadata class_c.exact entry"));
  for (const entry of inventory.class_c.additive) if (!requiredStrings(entry, ["path", "sha256", "purpose"]) || !safeRelative(entry.path) || !entry.path.endsWith(".rs")) issues.push(issue("RECLASSIFIED", "preserved-test-inventory.json", null, "metadata class_c.additive entry"));
  return issues;
}

/** Validate sealed evidence as JSON metadata before it can influence Rust
 * classification. No evidence/Markdown bytes enter the Rust lexer. */
export function verifySealedEvidence({ index, indexBytes, manifest, manifestBytes, seal, sealBytes, closure, closureBytes, overlay, overlayBytes, sealed, sealedBytes }) {
  const issues = [];
  if (!isRecord(index) || index.schema !== 2 || index.kind !== "sealed-corpus-index"
    || index.artifact_directory !== "attempt-12" || index.manifest !== "attempt-12/manifest.json"
    || !hash(index.manifest_sha256) || !hash(index.corpus_digest)
    || index.historical_attempts_are_not_current !== true) {
    return [issue("RECLASSIFIED", "../manifest.json", null, "sealed corpus index schema")];
  }
  if (!isRecord(manifest) || manifest.schema !== 2 || manifest.status !== "sealed" || !/^[0-9a-f]{40}$/.test(manifest.preserved_test_baseline_sha)
    || !/^[0-9a-f]{40}$/.test(manifest.head_tree) || !isRecord(manifest.identity) || !hash(manifest.identity.composite)
    || !hash(manifest.closure_manifest_digest) || !hash(manifest.overlay_archive_digest) || !hash(manifest.corpus_digest)
    || !Array.isArray(manifest.artifacts) || !Array.isArray(manifest.closure)) {
    return [issue("RECLASSIFIED", "manifest.json", null, "sealed manifest schema")];
  }
  if (sha256(manifestBytes) !== index.manifest_sha256 || manifest.corpus_digest !== index.corpus_digest) {
    issues.push(issue("RECLASSIFIED", "../manifest.json", null, "corpus index manifest/corpus digest"));
  }
  if (!isRecord(seal) || !hash(seal.manifest_sha256) || !hash(seal.corpus_digest)
    || !Number.isSafeInteger(seal.run_count) || seal.run_count < 0 || !hash(seal.cleanup_sha256)
    || seal.manifest_sha256 !== index.manifest_sha256 || seal.manifest_sha256 !== sha256(manifestBytes)
    || seal.corpus_digest !== index.corpus_digest || seal.corpus_digest !== manifest.corpus_digest) {
    issues.push(issue("RECLASSIFIED", "seal.json", null, "attempt seal manifest/corpus digest"));
  }
  if (!isRecord(closure) || closure.commit !== manifest.preserved_test_baseline_sha || closure.tree !== manifest.head_tree
    || closure.closure_digest !== manifest.closure_manifest_digest || closure.overlay_digest !== manifest.overlay_archive_digest
    || !Array.isArray(closure.closure) || JSON.stringify(closure.closure) !== JSON.stringify(manifest.closure)
    || sha256(JSON.stringify(closure)) !== manifest.identity.composite) {
    issues.push(issue("RECLASSIFIED", "closure.json", null, "closure source fingerprint schema/digest"));
  }
  else for (const entry of closure.closure) {
    if (!isRecord(entry) || !safeRelative(entry.path) || entry.entry_type !== "file" || typeof entry.mode !== "string"
      || !/^[0-7]{6}$/.test(entry.mode) || !Number.isSafeInteger(entry.byte_length) || entry.byte_length < 0
      || !hash(entry.sha256) || !["head", "tracked_modified", "overlay"].includes(entry.origin)) {
      issues.push(issue("RECLASSIFIED", "closure.json", null, "closure entry schema"));
    }
  }
  if (!isRecord(overlay) || !Array.isArray(overlay.entries) || !isRecord(overlay.blobs)) issues.push(issue("RECLASSIFIED", "overlay.json", null, "overlay schema"));
  else for (const entry of overlay.entries) {
    if (!isRecord(entry) || !safeRelative(entry.path) || !hash(entry.sha256) || !Number.isSafeInteger(entry.byte_length) || entry.byte_length < 0 || typeof overlay.blobs[entry.sha256] !== "string") {
      issues.push(issue("RECLASSIFIED", "overlay.json", null, "overlay entry schema")); continue;
    }
    const bytes = Buffer.from(overlay.blobs[entry.sha256], "base64");
    if (bytes.length !== entry.byte_length || sha256(bytes) !== entry.sha256) issues.push(issue("RECLASSIFIED", "overlay.json", null, `overlay blob ${entry.path}`));
  }
  if (!isRecord(sealed) || !Array.isArray(sealed.b_test_inventory) || !Array.isArray(sealed.candidates)
    || !isRecord(sealed.explicit_exclusion) || !Array.isArray(sealed.direct_sell_cash_assertion_anchors)) issues.push(issue("RECLASSIFIED", "b-test-inventory.json", null, "sealed B inventory schema"));
  else for (const entry of sealed.b_test_inventory) if (!requiredStrings(entry, ["file", "symbol", "effect_id", "allowed_transformation", "reason"]) || !strings(entry.candidate_terms) || !Array.isArray(entry.assertions)) issues.push(issue("RECLASSIFIED", "b-test-inventory.json", null, "sealed B entry schema"));
  const declared = new Map(manifest.artifacts.map((artifact) => [artifact?.file, artifact?.sha256]));
  for (const [name, bytes] of [["closure.json", closureBytes], ["overlay.json", overlayBytes], ["b-test-inventory.json", sealedBytes]]) if (!hash(declared.get(name)) || sha256(bytes) !== declared.get(name)) issues.push(issue("RECLASSIFIED", name, null, "manifest artifact hash"));
  if (manifest.inventory_file !== "b-test-inventory.json") issues.push(issue("RECLASSIFIED", "manifest.json", null, "sealed inventory filename"));
  return issues;
}

/**
 * Validate the human-readable inventory as metadata, independently of the
 * Rust lexer. Markdown is evidence policy, not a test source file: parsing it
 * as Rust would let prose braces/keywords alter the preservation decision.
 */
export function verifyInventoryMarkdown(markdown, inventory) {
  const issues = [];
  if (typeof markdown !== "string" || markdown.length === 0) {
    return [issue("RECLASSIFIED", "preserved-test-inventory.md", null, "metadata markdown is empty")];
  }
  const required = [
    "# 保留测试台账（Todo 3）",
    "## (a) 未改动保护",
    "## (b) 冻结 #9，严格四项",
    "## (c) 精确 Todo 2 合同改写",
    "preserved-test-inventory.json",
    "scripts/simulation/verify-preserved-tests.mjs",
  ];
  for (const marker of required) {
    if (!markdown.includes(marker)) issues.push(issue("RECLASSIFIED", "preserved-test-inventory.md", null, `metadata markdown missing ${marker}`));
  }
  if (!inventory || typeof inventory.baseline !== "string" || !markdown.includes(`commit \`${inventory.baseline}\``)) {
    issues.push(issue("RECLASSIFIED", "preserved-test-inventory.md", null, "metadata baseline link"));
  }
  if (/\\u0000/.test(markdown)) issues.push(issue("RECLASSIFIED", "preserved-test-inventory.md", null, "metadata NUL byte"));
  return issues;
}

export function rustTokens(source) {
  const tokens = []; let index = 0; let line = 1;
  const push = (value, start, tokenLine) => tokens.push({ value, start, end: index, line: tokenLine });
  const take = () => { const value = source[index++]; if (value === "\n") line += 1; return value; };
  const quoted = (prefix, raw = false) => {
    const start = index; const tokenLine = line; let hashes = 0;
    for (const char of prefix) if (source[index] === char) take();
    if (raw) { while (source[index] === "#") { hashes += 1; take(); } }
    if (source[index] !== '"') return false; take();
    while (index < source.length) {
      if (!raw && source[index] === "\\") { take(); if (index < source.length) take(); continue; }
      if (source[index] !== '"') { take(); continue; }
      take(); if (raw && source.slice(index, index + hashes) !== "#".repeat(hashes)) continue;
      for (let count = 0; count < hashes; count += 1) take(); push(source.slice(start, index), start, tokenLine); return true;
    }
    throw new Error("unterminated Rust string literal");
  };
  while (index < source.length) {
    if (/\s/.test(source[index])) { take(); continue; }
    if (source.startsWith("//", index)) { while (index < source.length && take() !== "\n") {} continue; }
    if (source.startsWith("/*", index)) { let depth = 0; while (index < source.length) { if (source.startsWith("/*", index)) { take(); take(); depth += 1; } else if (source.startsWith("*/", index)) { take(); take(); depth -= 1; if (!depth) break; } else take(); } if (depth) throw new Error("unterminated Rust block comment"); continue; }
    if (/^(?:br|rb)#{0,}/.test(source.slice(index)) && quoted(source.startsWith("br", index) ? "br" : "rb", true)) continue;
    if (/^r#{0,}"/.test(source.slice(index)) && quoted("r", true)) continue;
    if (source.startsWith("b\"", index) && quoted("b")) continue;
    if (source[index] === '"' && quoted("")) continue;
    const charStart = source.startsWith("b'", index) ? index + 1 : index;
    const charEnd = source[charStart + 1] === "\\" ? charStart + 3 : charStart + 2;
    if ((source.startsWith("b'", index) || source[index] === "'") && source[charEnd] === "'") { const start = index; const tokenLine = line; while (index <= charEnd) take(); push(source.slice(start, index), start, tokenLine); continue; }
    const start = index; const tokenLine = line;
    if (/[A-Za-z_]/.test(source[index])) { take(); while (/[A-Za-z0-9_]/.test(source[index] ?? "")) take(); push(source.slice(start, index), start, tokenLine); continue; }
    if (/[0-9]/.test(source[index])) { take(); while (/[A-Za-z0-9_.]/.test(source[index] ?? "")) take(); push(source.slice(start, index), start, tokenLine); continue; }
    const triple = source.slice(index, index + 3); const pair = source.slice(index, index + 2);
    if (["..=", "<<=", ">>=", "::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "..", "+=", "-=", "*=", "/=", "%="] .includes(triple)) { index += 3; push(triple, start, tokenLine); continue; }
    if (["::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "..", "+=", "-=", "*=", "/=", "%="] .includes(pair)) { index += 2; push(pair, start, tokenLine); continue; }
    push(take(), start, tokenLine);
  }
  return tokens;
}

export function rustItems(source) {
  const tokens = rustTokens(source); const items = [];
  for (let index = 0; index < tokens.length; index += 1) {
    if (tokens[index].value !== "fn" || !tokens[index + 1] || !/^[A-Za-z_]/.test(tokens[index + 1].value)) continue;
    let brace = -1; let paren = 0;
    for (let cursor = index + 2; cursor < tokens.length; cursor += 1) { if (tokens[cursor].value === "(") paren += 1; else if (tokens[cursor].value === ")") paren -= 1; else if (tokens[cursor].value === "{" && paren === 0) { brace = cursor; break; } }
    if (brace < 0) continue;
    let depth = 0; let end = brace;
    for (; end < tokens.length; end += 1) { if (tokens[end].value === "{") depth += 1; if (tokens[end].value === "}" && --depth === 0) break; }
    if (end === tokens.length) throw new Error("unterminated Rust item");
    items.push({ symbol: tokens[index + 1].value, start: tokens[index].start, end: tokens[end].start + 1, startLine: tokens[index].line, endLine: tokens[end].line, body: source.slice(tokens[index].start, tokens[end].start + 1) });
  }
  return items;
}
export const itemAt = (items, line) => items.find((item) => line >= item.startLine && line <= item.endLine) ?? null;
export function normalizedTokens(body) {
  const tokens = rustTokens(body).map((token) => token.value); const result = []; const parens = [];
  for (let index = 0; index < tokens.length; index += 1) {
    const candidate = tokens.slice(index, index + 5);
    if (candidate[0] === "." && candidate[1] === "expect" && candidate[2] === "(" && ["\"healthy step\"", "\"healthy save\""].includes(candidate[3]) && candidate[4] === ")") { index += 4; continue; }
    const token = tokens[index];
    if (token === "(") { parens.push(result.length > 0 && (/[A-Za-z0-9_]/.test(result.at(-1)) || [")", "]"].includes(result.at(-1)))); result.push(token); continue; }
    if (token === ")" && result.at(-1) === "," && parens.pop()) result.pop();
    else if (token === ")") parens.pop();
    result.push(token);
  }
  return result;
}
export const normalizeResultBody = (body) => JSON.stringify(normalizedTokens(body));
export function hasForbiddenTokens(text, tokens) { const values = new Set(rustTokens(text.replace(/^[-+]/gm, "")).map((token) => token.value)); return tokens.some((token) => values.has(token)); }

export function assertions(source, item) {
  const tokens = rustTokens(item.body); const result = [];
  for (let index = 0; index < tokens.length - 2; index += 1) {
    if (!new Set(["assert", "assert_eq", "assert_ne"]).has(tokens[index].value) || tokens[index + 1].value !== "!" || tokens[index + 2].value !== "(") continue;
    let depth = 0; let end = index + 2;
    for (; end < tokens.length; end += 1) { if (tokens[end].value === "(") depth += 1; if (tokens[end].value === ")" && --depth === 0) break; }
    if (tokens[end + 1]?.value !== ";") throw new Error(`assertion lacks semicolon: ${item.symbol}`);
    const start = item.start + tokens[index].start; const finish = item.start + tokens[end + 1].end;
    const raw = source.slice(start, finish);
    result.push({ index: result.length, start, end: finish, tokens: tokens.slice(index, end + 2).map((token) => token.value), anchor: sha256(raw), current_hash: sha256(JSON.stringify(normalizedTokens(raw))) });
    index = end + 1;
  }
  return result;
}
function itemWithoutAssertions(item, found) {
  const tokens = rustTokens(item.body); const ranges = found.map((entry) => [entry.start - item.start, entry.end - item.start]); const result = []; let offset = 0;
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    while (ranges[offset] && token.start >= ranges[offset][1]) offset += 1;
    if (ranges[offset] && token.start >= ranges[offset][0] && token.end <= ranges[offset][1]) { if (token.start === ranges[offset][0]) result.push(`@assert:${offset}`); continue; }
    const candidate = tokens.slice(index, index + 5).map((candidate) => candidate.value);
    if (candidate[0] === "." && candidate[1] === "expect" && candidate[2] === "(" && ["\"healthy step\"", "\"healthy save\""].includes(candidate[3]) && candidate[4] === ")") { index += 4; continue; }
    result.push(token.value);
  }
  return JSON.stringify(result);
}
function bPredicate(entry, values) {
  const prohibited = ["buyer", "T1", "t1", "owned", "shares", "commission", "stamp", "transfer", "fee", "rate", "qty", "price", "SessionSetup"];
  if (values.some((value) => prohibited.some((term) => value.toLowerCase().includes(term.toLowerCase())))) return false;
  if (entry.effect_id === "E9-A_SELL_CASH_RESERVATION") return values.some((value) => /seller|reserved|available|capped|intent|InsufficientCash/i.test(value)) && !values.some((value) => /buyer/i.test(value));
  if (entry.effect_id === "E9-B_SELL_ACCEPTANCE_FLIP") return values.some((value) => /InsufficientCash|IntentRejected|rejected|accepted|order/i.test(value));
  return false;
}
export function verifyBChange({ entry, oldItem, newItem, sourceBefore, sourceAfter, change }) {
  const baseline = assertions(sourceBefore, oldItem); const current = assertions(sourceAfter, newItem);
  if (baseline.length !== entry.assertions.length || current.length !== baseline.length) return issue("RECLASSIFIED", entry.file, entry.symbol, "B assertion count/order");
  for (const [index, sealed] of entry.assertions.entries()) if (baseline[index].anchor !== sealed.hunk_anchor || sealed.identity !== `${entry.symbol}:assertion:${index}`) return issue("RECLASSIFIED", entry.file, entry.symbol, "B sealed assertion anchor");
  if (change.effect_id !== entry.effect_id || change.allowed_transformation !== entry.allowed_transformation) return issue("RECLASSIFIED", entry.file, entry.symbol, "B effect/transformation");
  if (itemWithoutAssertions(oldItem, baseline) !== itemWithoutAssertions(newItem, current)) return issue("EXPANDED", entry.file, entry.symbol, "B non-assertion body");
  const declared = new Map(change.assertions.map((item) => [item.identity, item]));
  for (const [index, before] of baseline.entries()) {
    const after = current[index]; const identity = `${entry.symbol}:assertion:${index}`; const changed = before.current_hash !== after.current_hash;
    const mapped = declared.get(identity);
    if (changed && !mapped) return issue("RECLASSIFIED", entry.file, entry.symbol, "B unmapped assertion");
    if (!changed && mapped) return issue("RECLASSIFIED", entry.file, entry.symbol, "B unchanged declared assertion");
    if (!changed) continue;
    if (mapped.baseline_anchor !== before.anchor || mapped.current_hash !== after.current_hash || mapped.effect_id !== entry.effect_id) return issue("RECLASSIFIED", entry.file, entry.symbol, "B assertion identity/anchor/hash");
    if (!bPredicate(entry, after.tokens)) return issue("EXPANDED", entry.file, entry.symbol, "B effect domain");
  }
  if (declared.size !== baseline.filter((before, index) => before.current_hash !== current[index].current_hash).length) return issue("RECLASSIFIED", entry.file, entry.symbol, "B extra assertion declaration");
  return null;
}
export function verifyClassA({ entry, baselineSource, currentSource }) {
  const baselineItem = rustItems(baselineSource).find((item) => item.symbol === entry.symbol);
  const currentItem = rustItems(currentSource).find((item) => item.symbol === entry.symbol);
  const expected = baselineItem && sha256(normalizeResultBody(baselineItem.body));
  if (!baselineItem || entry.baseline_hash !== expected) return issue("RECLASSIFIED", entry.path, entry.symbol, "A sealed baseline hash");
  if (!currentItem || !currentItem.body.includes(entry.anchor)) return issue("MISSING", entry.path, entry.symbol, "A assertion anchor");
  if (sha256(normalizeResultBody(currentItem.body)) !== expected) return issue("EXPANDED", entry.path, entry.symbol, "A normalized body");
  return null;
}

export function parseHunks(diff) {
  const result = []; let path = null; let hunk = null;
  const finish = () => { if (hunk) result.push({ ...hunk, hash: sha256(hunk.lines.join("\n")) }); };
  for (const line of diff.split("\n")) {
    if (line.startsWith("diff --git ")) { finish(); hunk = null; } else if (line.startsWith("+++ b/")) path = line.slice(6);
    else if (line.startsWith("@@ ")) { finish(); const match = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(line); hunk = match ? { path, header: line, oldLine: Number(match[1]), newLine: Number(match[3]), lines: [] } : null; }
    else if (hunk && /^[ +\-]/.test(line) && !/^(---|\+\+\+)/.test(line)) hunk.lines.push(line);
  } finish(); return result;
}

/** The Rust lexer is only valid for Rust test sources. Keep documentation,
 * inventory JSON, and engine implementation hunks outside that language
 * boundary even when Git pathspec matching would otherwise include `*.rs`. */
export function rustTestHunks(diff) {
  return parseHunks(diff).filter((hunk) => safeRelative(hunk.path) && hunk.path.startsWith("packages/engine/tests/") && hunk.path.endsWith(".rs"));
}

export function protectedRustHunks(diff, protectedPaths) {
  const allowed = new Set(protectedPaths);
  return parseHunks(diff).filter((hunk) => safeRelative(hunk.path) && hunk.path.endsWith(".rs") && (hunk.path.startsWith("packages/engine/tests/") || allowed.has(hunk.path)));
}

export function classifyTracked({ hunks, baselineFiles, currentFiles, exactC, classB = [], classBChanges = [], forbiddenTokens }) {
  const issues = []; let mechanical = 0; let mechanicalSymbols = 0; let exact = 0; const unresolved = new Map(); const matchedExact = new Set(); const matchedB = new Set();
  for (const hunk of hunks) {
    const oldItem = baselineFiles.get(hunk.path) && itemAt(rustItems(baselineFiles.get(hunk.path)), hunk.oldLine);
    const newItem = currentFiles.get(hunk.path) && itemAt(rustItems(currentFiles.get(hunk.path)), hunk.newLine);
    const symbol = newItem?.symbol ?? oldItem?.symbol;
    if (!symbol) { issues.push(issue("ADDED", hunk.path, null, `unresolved ${hunk.header}`)); continue; }
    const exactEntry = exactC.find((entry) => entry.path === hunk.path && entry.hunk_hashes.includes(hunk.hash));
    if (exactEntry) {
      if (exactEntry.symbol !== symbol && exactEntry.baseline_symbol !== oldItem?.symbol) issues.push(issue("RECLASSIFIED", hunk.path, symbol, "exact C symbol"));
      else if (hasForbiddenTokens(hunk.lines.join("\n"), exactEntry.forbidden_tokens ?? forbiddenTokens)) issues.push(issue("EXPANDED", hunk.path, symbol, "exact C forbidden token"));
      else { exact += 1; matchedExact.add(`${exactEntry.path}:${hunk.hash}`); }
      continue;
    }
    const key = `${hunk.path}::${symbol}`; const group = unresolved.get(key) ?? { path: hunk.path, symbol, oldItem, newItem, hunks: [] }; group.hunks.push(hunk); unresolved.set(key, group);
  }
  for (const group of unresolved.values()) {
    if (!group.oldItem || !group.newItem) { issues.push(issue("ADDED", group.path, group.symbol, "item missing across baseline/current")); continue; }
    if (normalizeResultBody(group.oldItem.body) === normalizeResultBody(group.newItem.body)) { mechanical += group.hunks.length; mechanicalSymbols += 1; continue; }
    const b = classB.find((entry) => entry.file === group.path && entry.symbol === group.symbol);
    const changed = group.hunks.flatMap((hunk) => hunk.lines).join("\n");
    if (b) { const change = classBChanges.find((entry) => entry.file === group.path && entry.symbol === group.symbol); if (!change) { issues.push(issue("RECLASSIFIED", group.path, group.symbol, "B change declaration required")); continue; } matchedB.add(`${group.path}:${group.symbol}`); const result = verifyBChange({ entry: b, oldItem: group.oldItem, newItem: group.newItem, sourceBefore: baselineFiles.get(group.path), sourceAfter: currentFiles.get(group.path), change }); if (result) issues.push(result); continue; }
    const code = hasForbiddenTokens(changed, forbiddenTokens) ? "EXPANDED" : "ADDED"; for (const hunk of group.hunks) issues.push(issue(code, group.path, group.symbol, `non-mechanical ${hunk.header}`));
  }
  for (const change of classBChanges) if (!matchedB.has(`${change.file}:${change.symbol}`)) issues.push(issue("MISSING", change.file, change.symbol, "declared B change has no diff hunk"));
  for (const entry of exactC) for (const hunkHash of entry.hunk_hashes ?? []) if (!matchedExact.has(`${entry.path}:${hunkHash}`)) issues.push(issue("MISSING", entry.path, entry.symbol, "declared exact C hunk has no diff"));
  return { issues, mechanical, mechanicalSymbols, exact };
}

export function verifyInventoryShape(inventory, sealed) {
  if (inventory.class_b.length !== sealed.b_test_inventory.length) return [issue("RECLASSIFIED", "inventory", null, "B cardinality")];
  return sealed.b_test_inventory.flatMap((entry) => { const actual = inventory.class_b.find((item) => item.path === entry.file && item.symbol === entry.symbol); if (!actual) return [issue("MISSING", entry.file, entry.symbol, "sealed B entry")]; return ["effect_id", "allowed_transformation", "reason", "candidate_terms", "assertions"].some((field) => JSON.stringify(actual[field]) !== JSON.stringify(entry[field])) ? [issue("RECLASSIFIED", entry.file, entry.symbol, "sealed B metadata")] : []; });
}
