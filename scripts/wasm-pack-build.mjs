import { createHash } from "node:crypto";
import { copyFile, lstat, mkdir, readFile, realpath, stat } from "node:fs/promises";
import path from "node:path";
import { captureWasmBuildSourceIdentity, invalidateWasmPackageMetadata, isDefaultFeaturesRelease, produceWasmPackageMetadata } from "./wasm-package-capability.mjs";

async function licenseSnapshot(filename) {
  try {
    const metadata = await stat(filename);
    if (!metadata.isFile() || metadata.size === 0) throw new Error("expected a non-empty regular file");
    const content = await readFile(filename);
    if (content.length === 0) throw new Error("file became empty while reading");
    return { metadata, hash: createHash("sha256").update(content).digest("hex"), real: await realpath(filename) };
  } catch (error) {
    throw new Error(`LICENSE integrity check failed at ${filename}: ${error.message}; inspect the source and build output before retrying.`, { cause: error });
  }
}

async function assertLicenseTarget(source, target) {
  let metadata;
  try { metadata = await lstat(target); } catch (error) {
    if (error.code === "ENOENT") return;
    throw error;
  }
  if (metadata.isSymbolicLink()) throw new Error(`LICENSE output alias/symlink rejected: ${target}; use an isolated regular file.`);
  if (!metadata.isFile() || metadata.nlink !== 1) throw new Error(`LICENSE output alias/shared or non-regular file rejected: ${target}; use an isolated regular file.`);
  if (source.real === await realpath(target) || (source.metadata.dev === metadata.dev && source.metadata.ino === metadata.ino)) {
    throw new Error(`LICENSE source/output alias rejected: ${target}; choose an isolated wasm-pack --out-dir.`);
  }
}

async function assertUnchanged(filename, before) {
  const after = await licenseSnapshot(filename);
  if (after.hash !== before.hash) throw new Error(`LICENSE source changed during wasm-pack build: ${filename}; inspect the build before retrying.`);
  return after;
}

export async function executeWasmPackBuild(root, command, commandRunner) {
  const crate = path.join(root, "apps/web-wasm");
  const outIndex = command.args.indexOf("--out-dir");
  if (command.command !== "wasm-pack" || command.cwd !== root || command.args[0] !== "build" || command.args[1] !== "apps/web-wasm" || outIndex < 0 || command.args.lastIndexOf("--out-dir") !== outIndex || typeof command.args[outIndex + 1] !== "string" || command.args[outIndex + 1].length === 0) {
    throw new Error("LICENSE gate requires the workspace wasm-pack build with one explicit --out-dir.");
  }
  const manifest = await readFile(path.join(crate, "Cargo.toml"), "utf8");
  if (!/^license-file\s*=\s*"\.\.\/\.\.\/LICENSE"\s*$/m.test(manifest)) throw new Error(`LICENSE gate requires apps/web-wasm/Cargo.toml license-file = "../../LICENSE"; update the gate for a changed manifest.`);
  // wasm-pack 0.13.1 先将 out-dir 与 crate 拼接并 clean，再原样拼接 license-file。
  // 这里显式使用 resolve：Node path.join 对绝对参数的行为与 Rust Path::join 不同。
  const outDir = path.resolve(crate, command.args[outIndex + 1]);
  const sourceFile = path.join(root, "LICENSE");
  // 先解析真实 outDir，再处理 ../..，避免目录符号链改变文件系统实际目标。
  const wasmPackLicense = async () => path.resolve(await realpath(outDir), "../../LICENSE");
  const packageLicense = path.join(outDir, "LICENSE");
  const releaseDefaultFeatures = isDefaultFeaturesRelease(command);
  const producesCapability = releaseDefaultFeatures && command.env?.RUSTUP_TOOLCHAIN === "nightly-2026-09-05";
  const sourceSnapshot = producesCapability ? await captureWasmBuildSourceIdentity(root) : undefined;
  const before = await licenseSnapshot(sourceFile);
  await mkdir(outDir, { recursive: true });
  await assertLicenseTarget(before, await wasmPackLicense());
  await assertLicenseTarget(before, packageLicense);
  await invalidateWasmPackageMetadata(outDir);
  if (releaseDefaultFeatures && !producesCapability) throw new Error("WASM package metadata requires the actual release command to pin RUSTUP_TOOLCHAIN=nightly-2026-09-05.");
  let buildError;
  try { await commandRunner(command); } catch (error) { buildError = error; }
  let currentSource;
  try { currentSource = await assertUnchanged(sourceFile, before); } catch (error) {
    if (buildError !== undefined) throw new AggregateError([buildError, error], `${buildError.message}; ${error.message}`);
    throw error;
  }
  if (buildError !== undefined) throw buildError;
  await assertLicenseTarget(currentSource, await wasmPackLicense());
  await assertLicenseTarget(currentSource, packageLicense);
  // license-file 含 ../.. 时 wasm-pack 不会生成 pkg/LICENSE，须显式补齐实际分发文件。
  await copyFile(sourceFile, packageLicense);
  const packaged = await licenseSnapshot(packageLicense);
  if (packaged.hash !== before.hash) throw new Error(`LICENSE package content mismatch at ${packageLicense}; no artifact may be published.`);
  await assertUnchanged(sourceFile, before);
  if (producesCapability) await produceWasmPackageMetadata({ root, outDir, command, sourceSnapshot });
}
