#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFile, realpath, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  adaptLegacyStream,
  assembleLegacyProjection,
  currentReplayRequest,
  extractLegacyAcceptanceFlipSurface,
  extractLegacyBuyerFeeSurface,
  extractLegacyContinuousBuyLegSurface,
  extractLegacyControlledSellSurface,
  extractLegacyPriceCageSurface,
  extractLegacyT1Surface,
  extractLegacyThreeLegFeeCatchupSurface,
  loadSealedCorpusRun,
} from "./escrow-corpus-adapter.mjs";
import { compareCapturedPrimaryStream, compareExactCorpusCase, corpusUnmappedDiff } from "./escrow-corpus-exact.mjs";

export async function main(argv) {
  const [command, ...args] = argv;
  if (command === "diagnose-current") {
    assert.equal(args.length, 4, "usage: escrow-corpus.mjs diagnose-current <sealed-root> <scenario> <seed> <current-run.json>");
    const run = await loadSealedCorpusRun(...args.slice(0, 3));
    const current = JSON.parse(await readFile(args[3], "utf8"));
    const result = compareCapturedPrimaryStream(run, current);
    console.log(JSON.stringify(result));
    if (result.status === "UNMAPPED_DIFFERENCES") process.exitCode = 2;
    return;
  }
  if (command === "prepare-current") {
    assert.equal(args.length, 4, "usage: escrow-corpus.mjs prepare-current <sealed-root> <scenario> <seed> <new-request.json>");
    const workspace = process.env.ESCROW_WORKSPACE_ROOT;
    assert(workspace && path.isAbsolute(workspace) && await realpath(workspace) === workspace,
      "ESCROW_WORKSPACE_ROOT must be canonical and absolute");
    const output = path.resolve(args[3]);
    const parent = await realpath(path.dirname(output));
    const temp = await realpath(path.join(workspace, ".tmp"));
    assert(parent === path.dirname(output) && temp === path.join(workspace, ".tmp")
      && parent.startsWith(`${temp}${path.sep}`), "current replay output must be in a non-symlinked workspace .tmp child");
    const run = await loadSealedCorpusRun(...args.slice(0, 3));
    await writeFile(output, JSON.stringify(currentReplayRequest(run)), { flag: "wx" });
    console.log(JSON.stringify({ status: "PREPARED", historical_comparison_performed: false,
      output, provenance: run.provenance }));
    return;
  }
  if (command === "inspect") {
    assert.equal(args.length, 3, "usage: escrow-corpus.mjs inspect <sealed-root> <scenario> <seed>");
    const run = await loadSealedCorpusRun(...args);
    const stream = adaptLegacyStream(run);
    console.log(JSON.stringify({ status: "ADAPTED", historical_comparison_performed: false,
      scenario: run.scenario, seed: run.seed, updates: stream.updates.length,
      checkpoints: stream.state.checkpoints.length, event_keys: stream.provenance.event_key_derivations.length,
      provenance: run.provenance }));
    return;
  }
  if (command === "extract-controlled") {
    assert.equal(args.length, 4,
      "usage: escrow-corpus.mjs extract-controlled <sealed-root> <scenario> <seed> <auction-rollover|cross-tick-partial-fill>");
    const run = await loadSealedCorpusRun(...args.slice(0, 3));
    const surfaceEvidence = extractLegacyControlledSellSurface(run, args[3]);
    console.log(JSON.stringify({ status: "EXTRACTED_FOR_REVIEW", task9_acceptance: false,
      surface_evidence: surfaceEvidence, provenance: run.provenance }));
    return;
  }
  if (command === "extract-buyer-fees") {
    assert.equal(args.length, 3,
      "usage: escrow-corpus.mjs extract-buyer-fees <sealed-root> <equivalence> <seed>");
    const run = await loadSealedCorpusRun(...args);
    const surfaceEvidence = extractLegacyBuyerFeeSurface(run);
    console.log(JSON.stringify({ status: "EXTRACTED_FOR_REVIEW", task9_acceptance: false,
      surface_evidence: surfaceEvidence, provenance: run.provenance }));
    return;
  }
  if (command === "extract-equivalence") {
    assert.equal(args.length, 4,
      "usage: escrow-corpus.mjs extract-equivalence <sealed-root> <equivalence> <seed> <buyer-fees|t1|price-cage|continuous-buy-leg>");
    const run = await loadSealedCorpusRun(...args.slice(0, 3));
    const extracted = args[3] === "buyer-fees" ? extractLegacyBuyerFeeSurface(run)
      : args[3] === "t1" ? extractLegacyT1Surface(run)
        : args[3] === "price-cage" ? extractLegacyPriceCageSurface(run).surface
          : args[3] === "continuous-buy-leg" ? extractLegacyContinuousBuyLegSurface(run) : null;
    assert(extracted, "equivalence surface must be buyer-fees, t1, price-cage or continuous-buy-leg");
    console.log(JSON.stringify({ status: "EXTRACTED_FOR_REVIEW", task9_acceptance: false,
      surface_evidence: extracted, provenance: run.provenance }));
    return;
  }
  if (command === "extract-divergence") {
    assert.equal(args.length, 4,
      "usage: escrow-corpus.mjs extract-divergence <sealed-root> <divergence-9> <seed> <acceptance-flip|three-leg-fee-catchup>");
    const run = await loadSealedCorpusRun(...args.slice(0, 3));
    const extracted = args[3] === "acceptance-flip" ? extractLegacyAcceptanceFlipSurface(run)
      : args[3] === "three-leg-fee-catchup" ? extractLegacyThreeLegFeeCatchupSurface(run) : null;
    assert(extracted, "divergence surface must be acceptance-flip or three-leg-fee-catchup");
    console.log(JSON.stringify({ status: "EXTRACTED_FOR_REVIEW", task9_acceptance: false,
      surface_evidence: extracted.surface, provenance: extracted.provenance }));
    return;
  }
  assert.equal(command, "compare",
    "command must be inspect, prepare-current, diagnose-current, extract-controlled, extract-buyer-fees, extract-equivalence, extract-divergence or compare");
  assert.equal(args.length, 1, "usage: escrow-corpus.mjs compare <request.json>");
  const requestFile = path.resolve(args[0]);
  const request = JSON.parse(await readFile(requestFile, "utf8"));
  assert.deepEqual(Object.keys(request).sort(), ["current", "mappings", "scenario", "sealed_root", "seed", "surface_evidence"], "corpus comparison request keys");
  const run = await loadSealedCorpusRun(path.resolve(path.dirname(requestFile), request.sealed_root), request.scenario, request.seed);
  const { projection, provenance } = assembleLegacyProjection(run, request.surface_evidence);
  const comparison = compareExactCorpusCase(projection, request.current, request.mappings);
  const negatives = corpusUnmappedDiff(projection, request.current, request.mappings);
  console.log(JSON.stringify({ status: "PASS", comparison, negatives, provenance }));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(`escrow corpus failed: ${error.message}`);
    process.exitCode = 1;
  });
}
