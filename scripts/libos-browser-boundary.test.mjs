import assert from "node:assert/strict";
import { lstat, readdir, readFile, realpath } from "node:fs/promises";
import path from "node:path";
import { it } from "node:test";
import ts from "../apps/web/node_modules/typescript/lib/typescript.js";
import { collectFullRegressionSourceFingerprint } from "./run-full-regression.mjs";

const browser = path.resolve(import.meta.dirname, "../packages/libos/browser");
const host = path.resolve(import.meta.dirname, "../apps/web/src/host");

it("Browser LibOS 的执行机制必须在基础库，并且不能导入产品宿主", { timeout: 10000 }, async () => {
  const required = [
    "facade-slot-protocol", "facade-supervisor", "facade-thread-binding", "engine-clock-env",
    "wasm-wait-abort", "wasm-wait-death", "wasm-tombstone", "facade-child-shutdown",
    "facade-child-bootstrap", "facade-wasm-module", "facade-engine-protocol",
    "facade-supervisor-control", "engine-supervisor", "wasm-failure", "facade-engine-handshake",
  ];
  for (const name of required) {
    const source = await readFile(path.join(browser, `${name}.ts`), "utf8");
    assert.match(source, /\bexport\b/, name);
  }
  const realRoot = await realpath(browser);
  const entries = await readdir(browser, { recursive: true, withFileTypes: true });
  for (const entry of entries) {
    const absolute = path.join(entry.parentPath, entry.name);
    assert.ok(!(await lstat(absolute)).isSymbolicLink(), absolute);
    if (!entry.isFile() || !/\.(?:[cm]?[jt]s|[jt]sx)$/.test(entry.name)) continue;
    const bytes = await readFile(absolute, "utf8");
    assert.doesNotMatch(bytes, /stock-game|CATALOG_BASELINE_FAILED|StockCatalog/, absolute);
    const source = ts.createSourceFile(absolute, bytes, ts.ScriptTarget.Latest, true);
    assert.equal(source.parseDiagnostics.length, 0, absolute);
    assert.equal(source.referencedFiles.length + source.typeReferenceDirectives.length, 0, absolute);
    const specifiers = [];
    function visit(node) {
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier !== undefined) {
        assert.ok(ts.isStringLiteral(node.moduleSpecifier), absolute);
        specifiers.push(node.moduleSpecifier.text);
      }
      if (ts.isImportTypeNode(node)) {
        assert.ok(ts.isLiteralTypeNode(node.argument) && ts.isStringLiteral(node.argument.literal), absolute);
        specifiers.push(node.argument.literal.text);
      }
      assert.ok(!ts.isImportEqualsDeclaration(node), `${absolute}: 禁止隐式 CommonJS 依赖`);
      if (ts.isCallExpression(node)) {
        assert.ok(node.expression.kind !== ts.SyntaxKind.ImportKeyword
          && !(ts.isIdentifier(node.expression) && node.expression.text === "require"),
        `${absolute}: 使用静态导入以便验证 LibOS 依赖闭包`);
      }
      ts.forEachChild(node, visit);
    }
    visit(source);
    for (const specifier of specifiers) {
      assert.ok(specifier.startsWith("./") || specifier.startsWith("../"), `${absolute}: ${specifier}`);
      const importFile = await realpath(path.resolve(entry.parentPath, specifier));
      const relative = path.relative(realRoot, importFile);
      assert.ok(relative !== "" && !relative.startsWith("..") && !path.isAbsolute(relative), `${absolute}: ${specifier}`);
    }
  }
});

it("既有时钟及共享槽位的宿主入口只重导出同一个 LibOS 模块", { timeout: 10000 }, async () => {
  for (const name of ["engine-clock-env", "facade-slot-protocol", "facade-supervisor", "facade-thread-binding", "wasm-wait-abort", "wasm-wait-death", "wasm-tombstone", "facade-child-shutdown"]) {
    const source = await readFile(path.join(host, `${name}.ts`), "utf8");
    const code = source.split(/\r?\n/).filter((line) => !line.trimStart().startsWith("//")).join("\n").trim();
    assert.equal(code, `export * from "../../../../packages/libos/browser/${name}.ts";`, name);
  }
});

it("宿主 env 与 LibOS 暴露同一槽绑定单例，不能各自缓存状态", { timeout: 10000 }, async () => {
  const [hostEnv, libosEnv] = await Promise.all([
    import("../apps/web/src/host/engine-clock-env.ts"),
    import("../packages/libos/browser/engine-clock-env.ts"),
  ]);
  assert.deepEqual(Object.keys(hostEnv), Object.keys(libosEnv));
  for (const name of Object.keys(libosEnv)) assert.equal(hostEnv[name], libosEnv[name], name);
});

it("静态复核指纹包含真正的 Browser LibOS 源文件", { timeout: 10000 }, async () => {
  const fingerprint = await collectFullRegressionSourceFingerprint(path.resolve(import.meta.dirname, ".."));
  const closure = new Set(fingerprint.files.map(({ path: name }) => name));
  assert.ok(closure.has("packages/libos/browser/engine-clock-env.ts"));
  assert.ok(closure.has("packages/libos/browser/engine-supervisor.ts"));
  assert.ok(closure.has("packages/libos/browser/facade-supervisor.ts"));
});

it("基础库可独立创建自定义大小的共享 Memory，不依赖产品容量", { timeout: 10000 }, async () => {
  const { createFacadeSharedMemory } = await import("../packages/libos/browser/facade-wasm-module.ts");
  assert.throws(() => createFacadeSharedMemory({ isolated: false, initialPages: 1, maximumPages: 2 }), /crossOriginIsolated/);
  const memory = createFacadeSharedMemory({ isolated: true, initialPages: 1, maximumPages: 2 });
  assert.ok(memory.buffer instanceof SharedArrayBuffer);
  assert.equal(memory.buffer.byteLength, 65536);
  assert.equal(memory.grow(1), 1);
  assert.throws(() => memory.grow(1), RangeError);
});
