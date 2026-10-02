import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

function workflow() {
  return readFileSync(new URL("../.github/workflows/distributions.yml", import.meta.url), "utf8");
}

test("Rust caches restore only compilation directories, never prior-run published artifacts", () => {
  for (const [filename, directories] of [["distributions.yml", ["target/release", "target/wasm32-unknown-unknown"]],
    ["ci.yml", ["target/debug", "target/release", "target/wasm32-unknown-unknown", "target/build-cache"]]]) {
    const text = readFileSync(new URL(`../.github/workflows/${filename}`, import.meta.url), "utf8");
    const cache = text.split("uses: Swatinem/rust-cache@v2")[1].split(/^      - name:/m)[0];
    assert.match(cache, /cache-targets: false/);
    const paths = cache.split("cache-directories: |\n")[1]?.match(/^(?: +target\/[^\n]+\n)+/)?.[0].trim().split(/\s+/);
    assert.deepEqual(paths, directories);
    assert.match(cache, /prefix-key: v1-rust-compile-only/);
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

test("pure Server job does not set up Node, pnpm, WASM or UI and packaging smoke is short", () => {
  const text = workflow();
  const steps = text.split(/^  native:/m)[1].split(/^      - name: /m).slice(1);
  for (const step of steps.filter((entry) => /actions\/setup-node|pnpm\/action-setup|Install pinned Tauri|actions\/download-artifact/.test(entry))) {
    assert.match(step, /if: matrix\.product != 'server'|if: matrix\.product == 'desktop'/);
  }
  assert.match(text, /scripts[\\/]build\.bat server/);
  assert.match(text, /scripts\/build\.sh server/);
  assert.match(text, /run-with-deadline\.mjs 10000 -- node scripts\/smoke-deployment\.mjs/);
  assert.match(text, /availableParallelism|NUMBER_OF_PROCESSORS/);
  assert.match(text, /run-long-validation\.mjs 300000 -- node scripts\/package-distributions\.mjs/);
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
