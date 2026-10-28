// C2-C 专属产物来源门；不创建或修改任何 build receipt。
import assert from "node:assert/strict";

export async function verifyBuildReceiptChain(manifest) {
  // TDD 初红：忠实保留 e67 只有 caller 字符串相等的旧来源门。
  assert.equal(manifest.nativeSourceManifestSha256, manifest.sourceManifest.sha256);
  assert.equal(manifest.wasmSourceManifestSha256, manifest.sourceManifest.sha256);
}

export function assertCheckedGamePath(_games, _directory, wasmPath) {
  // TDD 初红：e67 丢弃 checker 返回列表，仍直接实例化 manifest 路径。
  return wasmPath;
}
