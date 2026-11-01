import assert from "node:assert/strict";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import ts from "typescript";

test("React 18 容器 ref 与桌面组件 createElement 接线通过类型校验", () => {
  const configPath = fileURLToPath(new URL("../../tsconfig.app.json", import.meta.url));
  const config = ts.readConfigFile(configPath, ts.sys.readFile);
  if (config.error) throw new Error(ts.flattenDiagnosticMessageText(config.error.messageText, "\n"));
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, fileURLToPath(new URL("../../", import.meta.url)));
  const program = ts.createProgram({
    rootNames: [
      fileURLToPath(new URL("./WorkspaceGrid.tsx", import.meta.url)),
      fileURLToPath(new URL("./workspace-grid.test.ts", import.meta.url)),
      fileURLToPath(import.meta.url),
    ],
    options: { ...parsed.options, incremental: false, noEmit: true },
  });
  const diagnostics = [...parsed.errors, ...ts.getPreEmitDiagnostics(program)];
  assert.deepEqual(diagnostics.map((diagnostic) => ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")), []);
});
