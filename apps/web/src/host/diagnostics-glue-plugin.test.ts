import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import type { IncomingMessage, ServerResponse } from "node:http";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import type { Plugin, ViteDevServer } from "vite";
import productConfig from "../../vite.config.ts";
import { DIAGNOSTICS_GLUE_URL, ENGINE_ENV_BROWSER_URL, diagnosticsGluePlugin } from "../../config/diagnostics-glue.ts";

const source = `import * as __wbg_star0 from 'env';
const message = "from 'env'";
// import * as untouched from 'env';
import * as similarlyNamed from 'env-extra';
import * as relative from './env';
const dynamic = () => import('env');
export { similarlyNamed, relative, dynamic, message };
`;
type Middleware = (request: IncomingMessage, response: ServerResponse, next: (error?: unknown) => void) => void | Promise<void>;

async function fixture(run: (directory: string) => Promise<void>) {
  const parent = resolve(import.meta.dirname, "../../../../.tmp");
  await mkdir(parent, { recursive: true });
  const directory = await mkdtemp(join(parent, "diagnostics-glue-"));
  try {
    await writeFile(join(directory, "web_wasm.js"), source);
    await run(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function mount(plugin: Plugin, root: string, dev = true) {
  const handlers: Middleware[] = [];
  const hook = plugin.configureServer;
  assert.equal(typeof hook, "function", "产品插件必须安装 diagnostics glue middleware");
  if (typeof hook !== "function") throw new Error("缺少 configureServer");
  await hook.call({} as ThisParameterType<typeof hook>, { config: { root, command: "serve", env: { DEV: dev } }, middlewares: { use(handler: Middleware) { handlers.push(handler); } } } as unknown as ViteDevServer);
  return handlers;
}

async function request(handler: Middleware, url: string, method = "GET") {
  const headers: Record<string, unknown> = {};
  return await new Promise<{ body: string; headers: typeof headers; next: boolean; error?: unknown }>((done, reject) => {
    const response = { setHeader(name: string, value: unknown) { headers[name.toLowerCase()] = value; }, end(body = "") { done({ body, headers, next: false }); } };
    try {
      Promise.resolve(handler({ url, method } as IncomingMessage, response as unknown as ServerResponse, (error) => done({ body: "", headers, next: true, error }))).catch(reject);
    } catch (error) { reject(error); }
  });
}

test("产品 Vite config 安装同 singleton diagnostics glue 插件", { timeout: 10000 }, () => {
  const plugin = productConfig.plugins?.flat().find((value) => value && typeof value === "object" && "name" in value && value.name === "diagnostics-glue-env");
  assert.ok(plugin, "DEV diagnostics 不能依赖专属测试 rewrite");
  const aliases = productConfig.resolve?.alias;
  assert.ok(Array.isArray(aliases));
  const alias = aliases.find(({ find }) => find instanceof RegExp && find.test("env"));
  assert.ok(alias);
  assert.equal(alias.replacement, fileURLToPath(new URL("./engine-clock-env.ts", import.meta.url)));
  assert.equal(ENGINE_ENV_BROWSER_URL, "/src/host/engine-clock-env.ts");
});

test("DEV glue 只改真实静态 bare env import，保留其余字节且不改 generated 文件", { timeout: 10000 }, async () => {
  await fixture(async (directory) => {
    const [handler] = await mount(diagnosticsGluePlugin({ packageDirectory: directory }), directory);
    const result = await request(handler, DIAGNOSTICS_GLUE_URL);
    assert.equal(result.next, false);
    assert.equal(result.body, source.replace("from 'env';", `from '${ENGINE_ENV_BROWSER_URL}';`));
    assert.match(String(result.headers["content-type"]), /javascript/);
    assert.equal(result.headers["cache-control"], "no-store");
    assert.equal(await readFile(join(directory, "web_wasm.js"), "utf8"), source);
    await writeFile(join(directory, "web_wasm.js"), 'import * as environment from "env";\n');
    assert.equal((await request(handler, `${DIAGNOSTICS_GLUE_URL}?t=1`)).body, `import * as environment from "${ENGINE_ENV_BROWSER_URL}";\n`);
  });
});

test("guard 只处理精确 diagnostics JS URL，普通 glue/filetype/traversal 不进入", { timeout: 10000 }, async () => {
  await fixture(async (directory) => {
    const [handler] = await mount(diagnosticsGluePlugin({ packageDirectory: directory }), directory);
    for (const url of ["/wasm-pkg/web_wasm.js", "/wasm-diagnostics-pkg/web_wasm_bg.wasm", "/wasm-diagnostics-pkg/web_wasm.ts", "/wasm-diagnostics-pkg/sub/web_wasm.js", "/wasm-diagnostics-pkg/../web_wasm.js", "/wasm-diagnostics-pkg/%2e%2e/web_wasm.js", "/wasm-diagnostics-pkg/%77eb_wasm.js", "https://other.test/wasm-diagnostics-pkg/web_wasm.js"]) {
      assert.equal((await request(handler, url)).next, true, url);
    }
    assert.equal((await request(handler, DIAGNOSTICS_GLUE_URL, "POST")).next, true);
    const head = await request(handler, DIAGNOSTICS_GLUE_URL, "HEAD");
    assert.equal(head.next, false);
    assert.equal(head.body, "");
  });
});

test("非 DEV 不安装 middleware，build 不启插件且不校验诊断目录", { timeout: 10000 }, async () => {
  const plugin = diagnosticsGluePlugin({ packageDirectory: "invalid-relative" });
  assert.equal(plugin.apply, "serve");
  assert.deepEqual(await mount(plugin, "/unused", false), []);
});

test("默认只读 webroot diagnostics 包；缺失或不含真实 bare env import 必须显错", { timeout: 10000 }, async () => {
  await fixture(async (root) => {
    const [handler] = await mount(diagnosticsGluePlugin(), root);
    const missing = await request(handler, DIAGNOSTICS_GLUE_URL);
    assert.ok(missing.error instanceof Error);
    assert.ok(missing.error.cause instanceof Error);
    await mkdir(join(root, "wasm-diagnostics-pkg"));
    await writeFile(join(root, "wasm-diagnostics-pkg/web_wasm.js"), source);
    assert.equal((await request(handler, DIAGNOSTICS_GLUE_URL)).next, false);
    await writeFile(join(root, "wasm-diagnostics-pkg/web_wasm.js"), `// import 'env';\nconst value = "env";\nimport './env';\n`);
    const noImport = await request(handler, DIAGNOSTICS_GLUE_URL);
    assert.ok(noImport.error instanceof Error);
    assert.match(noImport.error.message, /bare env import/);
    assert.ok(noImport.error.cause instanceof Error);
    assert.match(noImport.error.cause.message, /bare env import/);
  });
});

test("显式目录严格验证，缺包/空值/相对或 traversal 路径不 fallback", { timeout: 10000 }, async () => {
  await fixture(async (directory) => {
    for (const packageDirectory of ["", "relative", ` ${directory}`, `${directory}/../other`, `${directory}/./other`, `${directory}/missing`]) {
      await assert.rejects(mount(diagnosticsGluePlugin({ packageDirectory }), directory));
    }
    await fixture(async (other) => {
      await rm(join(directory, "web_wasm.js"));
      await symlink(join(other, "web_wasm.js"), join(directory, "web_wasm.js"));
      await assert.rejects(mount(diagnosticsGluePlugin({ packageDirectory: directory }), directory), /symbolic link|符号链接/);
    });
  });
});

test("产品 config 将显式 server-only artifact 目录交给生产插件，无需测试 rewrite", { timeout: 10000 }, async () => {
  await fixture(async (directory) => {
    const previous = process.env.STOCK_GAME_DIAGNOSTICS_PKG;
    process.env.STOCK_GAME_DIAGNOSTICS_PKG = directory;
    try {
      const url = new URL("../../vite.config.ts", import.meta.url);
      url.searchParams.set("diagnostics-fixture", directory);
      const loaded = await import(url.href) as { default: typeof productConfig };
      const plugin = loaded.default.plugins?.flat().find((value) => value && typeof value === "object" && "name" in value && value.name === "diagnostics-glue-env") as Plugin | undefined;
      assert.ok(plugin);
      const [handler] = await mount(plugin, join(directory, "nonexistent-webroot"));
      const result = await request(handler, DIAGNOSTICS_GLUE_URL);
      assert.equal(result.next, false);
      assert.equal(result.body, source.replace("from 'env';", `from '${ENGINE_ENV_BROWSER_URL}';`));
    } finally {
      if (previous === undefined) delete process.env.STOCK_GAME_DIAGNOSTICS_PKG;
      else process.env.STOCK_GAME_DIAGNOSTICS_PKG = previous;
    }
  });
});
