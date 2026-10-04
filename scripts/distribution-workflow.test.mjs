import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

function workflow() {
  return readFileSync(new URL("../.github/workflows/distributions.yml", import.meta.url), "utf8");
}

test("审计G56：只有与owner对应的github.io仓库编译为根路径", () => {
  const step = workflow().split("- name: Record Pages repository path")[1].split("- name:")[0];
  const source = step.match(/run: node -e "([^\n]+)"/)[1];
  for (const [owner, name, expected] of [["alice", "alice.github.io", "/"], ["Alice", "ALICE.github.io", "/"], ["alice", "bob.github.io", "/bob.github.io/"], ["alice", "stock_market_game", "/stock_market_game/"]]) {
    const writes = [];
    runInNewContext(source, {
      process: { env: { REPOSITORY_OWNER: owner, REPOSITORY_NAME: name, GITHUB_ENV: "fixture-env" } },
      require(module) { assert.equal(module, "node:fs"); return { appendFileSync(filename, text) { assert.equal(filename, "fixture-env"); writes.push(text); } }; },
    }, { timeout: 1000 });
    assert.deepEqual(writes, [`PAGES_BASE=${expected}\n`]);
  }
  assert.match(step, /REPOSITORY_OWNER: \$\{\{ github\.repository_owner \}\}/);
  for (const env of [{ REPOSITORY_NAME: "alice.github.io" }, { REPOSITORY_OWNER: "alice" }, { REPOSITORY_OWNER: "alice", REPOSITORY_NAME: "bad/name" }, { REPOSITORY_OWNER: "bad/owner", REPOSITORY_NAME: "stock_market_game" }, { REPOSITORY_OWNER: "", REPOSITORY_NAME: "stock_market_game" }]) {
    assert.throws(() => runInNewContext(source, { process: { env }, require() { throw new Error("非法输入不得写GITHUB_ENV"); } }, { timeout: 1000 }), /仓库名称或owner非法/);
  }
});

test("Rust caches restore only compilation directories, never prior-run published artifacts", () => {
  for (const [filename, directories] of [["distributions.yml", ["target/release", "target/wasm32-unknown-unknown"]],
    ["ci.yml", ["target/debug", "target/release", "target/wasm32-unknown-unknown", "target/build-cache"]]]) {
    const text = readFileSync(new URL(`../.github/workflows/${filename}`, import.meta.url), "utf8");
    const cache = text.split("uses: Swatinem/rust-cache@v2")[1].split(/^      - name:/m)[0];
    assert.match(cache, /cache-targets: false/);
    const paths = cache.split("cache-directories: |\n")[1]?.match(/^(?: +target\/[^\n]+\n)+/)?.[0].trim().split(/\s+/);
    assert.deepEqual(paths, directories);
    assert.match(cache, /prefix-key: rust-compile-only-cache-\$\{\{ env\.RUST_COMPILE_CACHE_FORMAT_VERSION \}\}/);
  }
});

test("native distribution matrix builds all three products on three operating systems without signing or publishing", () => {
  const text = workflow();
  for (const runner of ["ubuntu-24.04", "windows-2022", "macos-15"]) assert.ok(text.includes(runner));
  assert.match(text, /product: \$\{\{ fromJSON\(inputs.product == 'all' && '\["desktop", "webui-server"\]'/);
  assert.match(text, /product: \[server\]/);
  assert.match(text, /os: \$\{\{ fromJSON\(inputs.operating-systems\) \}\}/);
  assert.match(text, /if: inputs.product == 'all' \|\| inputs.product == 'server'/);
  assert.match(text, /fail-fast: false/);
  assert.match(text, /permissions:\s+contents: read/);
  assert.doesNotMatch(text, /contents: write|gh release create|upload-release-asset|TAURI_SIGNING_PRIVATE_KEY|APPLE_CERTIFICATE|WINDOWS_CERTIFICATE/);
  assert.match(text, /if-no-files-found: error/);
  assert.match(text, /actions\/upload-artifact@v4/);
  assert.match(text, /scripts\/package-distributions\.mjs/);
});

test("frontend is built once with pinned tools and native targets reuse only the current run artifact", () => {
  const text = workflow();
  assert.match(text, /frontend:\s+name:/);
  assert.match(text, /needs: frontend/);
  const server = text.split(/^  server:/m)[1];
  assert.ok(server, "pure Server must have an independent build matrix");
  assert.doesNotMatch(server, /needs:/);
  assert.match(server, /steps: \*native-steps/);
  assert.match(text, /version: v0\.13\.1/);
  assert.match(text, /nightly-2026-09-05/);
  assert.match(text, /tauri-cli-v2\.12\.1/);
  assert.match(text, /--frontend-dist target\/ci-frontend/);
  assert.match(text, /actions\/download-artifact@v4/);
  assert.match(text, /hashFiles\('Cargo.lock'/);
  assert.match(text, /actions\/cache\/save@v4/);
  assert.match(text, /if: always\(\)/);
  assert.match(text, /node scripts\/build-targets\.mjs desktop --compile-only --frontend-dist target\/ci-frontend --jobs/);
  assert.ok(text.indexOf("Prepare desktop native compile cache") < text.indexOf("Build native UI product with prebuilt frontend"));
  assert.doesNotMatch(text, /continue-on-error: true|run-full-regression/);
});

test("纯 Server 无前端工具依赖，构建直接进入受限打包且不执行 deployment smoke", () => {
  const text = workflow();
  const steps = text.split(/^  native:/m)[1].split(/^      - name: /m).slice(1);
  for (const step of steps.filter((entry) => /actions\/setup-node|pnpm\/action-setup|Install pinned Tauri|actions\/download-artifact/.test(entry))) {
    assert.match(step, /if: matrix\.product != 'server'|if: matrix\.product == 'desktop'/);
  }
  assert.match(text, /scripts[\\/]build\.bat server/);
  assert.match(text, /scripts\/build\.sh server/);
  assert.doesNotMatch(text, /smoke-deployment\.mjs/);
  assert.match(text, /availableParallelism|NUMBER_OF_PROCESSORS/);
  assert.match(text, /run-long-validation\.mjs 300000 -- node scripts\/package-distributions\.mjs/);
});

test("分发保留生产前端、静态 Web 和版本校验，移除测试与 Playwright smoke", () => {
  const text = workflow();
  assert.match(text, /run: node scripts\/frontend-build\.mjs --jobs/);
  assert.match(text, /wasm-pack --version/);
  assert.match(text, /vite\.js build --mode pages --base/);
  assert.match(text, /package-static-web\.mjs/);
  assert.match(text, /actions\/upload-pages-artifact@v3/);
  assert.doesNotMatch(text, /--test|\.test\.mjs|playwright|smoke-pages\.mjs|smoke-deployment\.mjs/);
});

test("Windows MSI uses a Chinese-compatible code page for the existing Chinese product name", () => {
  const config = JSON.parse(readFileSync(new URL("../apps/desktop/src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  assert.equal(config.bundle.windows.wix.language, "zh-CN");
});

test("CI workflows delegate frontend and Server build rules to repository scripts", () => {
  for (const filename of ["ci.yml", "distributions.yml"]) {
    const text = readFileSync(new URL(`../.github/workflows/${filename}`, import.meta.url), "utf8");
    assert.match(text, /run: node scripts\/frontend-build\.mjs --jobs/);
    assert.doesNotMatch(text, /run:[^\n]*(?:wasm-pack build|pnpm --filter web build|cargo build -p server)/);
    assert.doesNotMatch(text, /(?:cp -[rR]|xcopy)[^\n]*wasm-pkg/);
  }
});

test("Desktop builds only the Rust library linked into its executable, not unused mobile FFI libraries", () => {
  const manifest = readFileSync(new URL("../apps/desktop/src-tauri/Cargo.toml", import.meta.url), "utf8");
  const library = manifest.split("[lib]")[1].split(/\n\[/)[0];
  assert.match(library, /crate-type\s*=\s*\["rlib"\]/);
});
