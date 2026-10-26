// C2-C 专属产物来源门；不创建或修改任何 build receipt。
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readFile } from "node:fs/promises";
import path from "node:path";

async function verifyFile(artifact, label) {
  assert(artifact && typeof artifact.path === "string", `${label} receipt缺少文件绑定`);
  assert.equal(path.resolve(artifact.path), artifact.path, `${label} receipt必须使用绝对路径`);
  assert.match(artifact.sha256, /^[a-f0-9]{64}$/, `${label} receipt缺少实际SHA`);
  const hash = createHash("sha256");
  let bytes = 0;
  for await (const chunk of createReadStream(artifact.path)) { hash.update(chunk); bytes += chunk.length; }
  assert.equal(bytes, artifact.bytes, `${label} receipt字节数不符`);
  assert.equal(hash.digest("hex"), artifact.sha256, `${label} receipt内容hash不符`);
}

async function receipt(artifact, label) {
  await verifyFile(artifact, label);
  return JSON.parse(await readFile(artifact.path, "utf8"));
}

export async function verifyNativeReceiptChain(manifest) {
  const receipts = manifest.nativeReceipts;
  assert(receipts, "缺少真实native build/run receipt链");
  const [plan, build, binding, tracePlan, compiled, ran] = await Promise.all([
    receipt(receipts.enginePlan, "native engine plan"),
    receipt(receipts.engineResult, "native engine result"),
    receipt(receipts.engineBinding, "native engine binding"),
    receipt(receipts.tracePlan, "native trace plan"),
    receipt(receipts.traceCompile, "native trace compile"),
    receipt(receipts.traceRun, "native trace run"),
  ]);
  assert.equal(plan.head, manifest.sourceHead, "native build source head不符");
  assert.equal(binding.head, manifest.sourceHead, "native artifact source head不符");
  assert.equal(build.exitCode, 0, "native engine build receipt未成功");
  assert.equal(binding.build_finished_success, true, "native engine artifact receipt未成功");
  assert.equal(path.basename(receipts.enginePlan.path), "build-plan.json");
  assert.equal(path.dirname(receipts.engineResult.path), path.dirname(receipts.enginePlan.path));
  assert.equal(path.dirname(receipts.engineBinding.path), path.dirname(receipts.enginePlan.path));
  assert.equal(path.resolve(path.dirname(receipts.enginePlan.path), plan.sourceManifest), manifest.sourceManifest.path, "native build source manifest必须来自实际plan所引用文件");
  const source = await receipt(manifest.sourceManifest, "native source manifest");
  assert.equal(source.head, manifest.sourceHead, "native source manifest head不符");
  const engines = binding.artifacts.filter((item) => item.target === "engine" && item.normal_rlib === true && item.test === false);
  assert.equal(engines.length, 1, "native receipt必须唯一绑定normal engine rlib");
  const engine = engines[0];
  await verifyFile({ path: engine.sealed, bytes: engine.bytes, sha256: engine.sha256 }, "native normal rlib");
  assert.equal(tracePlan.engine_artifact.sealed, engine.sealed, "trace编译未使用已验normal rlib");
  assert.equal(tracePlan.engine_artifact.sha256, engine.sha256, "trace normal rlib hash不符");
  assert.equal(tracePlan.engine_artifact.bytes, engine.bytes, "trace normal rlib大小不符");
  assert(tracePlan.compile_outer_argv.includes("engine=" + engine.sealed), "trace真实编译argv未携同normal rlib");
  assert.deepEqual(compiled.outer_argv, tracePlan.compile_outer_argv, "trace编译receipt与plan argv不符");
  assert.equal(compiled.wrapper_exit_code, 0, "trace编译未成功");
  assert.equal(compiled.source_after, tracePlan.source.sha256, "trace编译source漂移");
  const traceSource = manifest.sourceFiles.find((item) => item.path === tracePlan.source.path);
  assert(traceSource, "必须冻结实际trace源码");
  assert.equal(traceSource.sha256, tracePlan.source.sha256, "trace source hash不符");
  await verifyFile(traceSource, "native trace source");
  await verifyFile(compiled.binary, "native trace binary");
  assert.equal(tracePlan.compile_outer_argv[tracePlan.compile_outer_argv.indexOf("-o") + 1], compiled.binary.path);
  assert.deepEqual(ran.outer_argv, tracePlan.run_outer_argv, "trace运行receipt与plan argv不符");
  assert.equal(ran.wrapper_exit_code, 0, "trace运行未成功");
  assert.equal(tracePlan.run_outer_argv[4], compiled.binary.path, "trace运行不是实际编译产物");
  assert.equal(tracePlan.input.path, manifest.fixture.path, "trace不是同一个实际输入JSON");
  assert.equal(tracePlan.input.sha256, manifest.fixture.sha256, "trace输入hash不符");
  assert.equal(tracePlan.input.bytes, manifest.fixture.bytes, "trace输入大小不符");
  await verifyFile(manifest.fixture, "native trace input");
  assert.equal(tracePlan.run_outer_argv.at(-2), manifest.fixture.path);
  assert.equal(tracePlan.run_outer_argv.at(-1), manifest.nativeTrace.path);
  assert.deepEqual(ran.trace, manifest.nativeTrace, "trace输出未绑定实际run receipt");
  await verifyFile(manifest.nativeTrace, "native trace output");
}

export async function verifyBuildReceiptChain(manifest) {
  await verifyNativeReceiptChain(manifest);
  assert(manifest.wasmReceipts, "缺少正式WASM build receipt链，禁止初始化");
  // W1尚无已审成功receipt schema；取得最终真实合同后再消费，不能猜字段冒成功。
  throw new Error("WASM build receipt schema尚未冻结，当前禁止初始化；须由root提供正式fresh build绑定");
}

export function assertCheckedGamePath(games, directory, wasmPath) {
  assert.equal(games.length, 1, "WASM checker必须唯一返回实际game文件");
  assert.equal(path.resolve(directory, games[0]), path.resolve(wasmPath), "WASM manifest指向的文件不是checker实际校验的game");
  return wasmPath;
}
