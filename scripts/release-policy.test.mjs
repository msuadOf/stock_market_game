import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";

const workflows = new URL("../.github/workflows/", import.meta.url);
const workflow = (name) => readFileSync(new URL(name, workflows), "utf8");

test("ordinary commits and pull requests trigger no workflow; release alone subscribes to tags", () => {
  for (const filename of readdirSync(workflows).filter((name) => name.endsWith(".yml"))) {
    const text = workflow(filename);
    assert.doesNotMatch(text, /^  (?:pull_request|pull_request_target|schedule|workflow_run):/m, filename);
    if (filename === "release.yml") {
      assert.match(text, /push:\s+tags: \["v\*", "test-\*"\]/);
      assert.doesNotMatch(text, /branches:/);
    } else assert.doesNotMatch(text, /^  push:/m, filename);
  }
  assert.match(workflow("distributions.yml"), /workflow_call:/);
  assert.match(workflow("ci.yml"), /workflow_dispatch:/);
  assert.doesNotMatch(workflow("ci.yml"), /workflow_call:/);
});

test("seven independent manual entries cover individual products and all products with Pages", () => {
  for (const [name, runner] of [["windows", "windows-2022"], ["linux", "ubuntu-24.04"], ["macos", "macos-15"]]) {
    const text = workflow(`build-${name}.yml`);
    assert.match(text, /workflow_dispatch:/);
    assert.match(text, /uses: \.\/\.github\/workflows\/distributions.yml/);
    assert.match(text, /product: desktop/);
    assert.ok(text.includes(`operating-systems: '["${runner}"]'`));
    assert.doesNotMatch(text, /contents: write|gh release/);
  }
  assert.match(workflow("build-server.yml"), /product: server/);
  assert.match(workflow("build-web.yml"), /product: web/);
  assert.match(workflow("build-web.yml"), /deploy-pages@v4/);
  assert.match(workflow("build-web.yml"), /environment:\s+name: github-pages/);
  assert.match(workflow("build-web.yml"), /group: github-pages-deployment\s+cancel-in-progress: false\s+queue: max/);
  for (const [filename, product] of [["build-server.yml", "server"], ["build-webui-server.yml", "webui-server"], ["build-all.yml", "all"]]) {
    const text = workflow(filename);
    assert.match(text, /workflow_dispatch:/);
    assert.match(text, /uses: \.\/\.github\/workflows\/distributions.yml/);
    assert.ok(text.includes(`product: ${product}\n`));
    assert.doesNotMatch(text, /operating-systems:|contents: write|gh release/);
  }
  const all = workflow("build-all.yml");
  assert.match(all, /pages:\s+needs: build/);
  assert.match(all, /uses: \.\/\.github\/workflows\/build-web.yml/);
  assert.match(all, /reuse-site: true/);
  assert.match(all, /pages: write/);
  assert.match(all, /id-token: write/);
  assert.doesNotMatch(all, /if: always\(\)|continue-on-error/);
});

test("发布从标签校验直接构建十组制品，只公开本轮核验的资产并部署 Pages", () => {
  const text = workflow("release.yml");
  const jobs = [...text.split("jobs:\n")[1].matchAll(/^  ([a-z][a-z-]*):$/gm)].map((match) => match[1]);
  assert.deepEqual(jobs, ["validate", "distributions", "publish", "pages", "prune-caches"]);
  assert.doesNotMatch(text, /workflows\/ci\.yml|\bci\b/);
  assert.match(text, /uses: \.\/\.github\/workflows\/distributions.yml/);
  assert.match(text, /distributions:\s+needs: validate/);
  assert.match(text, /product: all/);
  assert.match(text, /needs: \[validate, distributions\]/);
  assert.match(text, /scripts\/publish-release.mjs/);
  assert.match(text, /contents: write/);
  assert.match(text, /actions\/download-artifact@v4/);
  assert.doesNotMatch(text, /merge-multiple: true|continue-on-error|release create.*\$\{\{/);
  assert.match(text, /uses: \.\/\.github\/workflows\/build-web.yml/);
  assert.match(text, /pages:\s+needs: publish/);
  assert.match(text, /reuse-site: true/);
  assert.match(text, /needs: \[distributions, publish, pages\]/);
  assert.match(text, /RELEASE_SHA: \$\{\{ needs\.validate\.outputs\.sha \}\}/);
  assert.match(text, /cancel-in-progress: false\s+queue: max/);
});

test("所有构建与发布入口均不执行 CI、测试、lint、Clippy 或浏览器及部署 smoke", () => {
  for (const filename of readdirSync(workflows).filter((name) => name.endsWith(".yml") && name !== "ci.yml")) {
    assert.doesNotMatch(workflow(filename), /workflows\/ci\.yml|--test(?:\b|-)|\.test\.[cm]?[jt]s|run-full-regression|smoke-(?:pages|deployment)|playwright|cargo (?:test|clippy)|pnpm[^\n]*(?: test(?::|\b)| lint\b)/, filename);
  }
});

test("release accepts only numeric versions or nonempty safe test suffixes", async () => {
  const { parseReleaseTag } = await import("./release-policy.mjs");
  assert.deepEqual(parseReleaseTag("v1.2.3"), { tag: "v1.2.3", prerelease: false });
  assert.deepEqual(parseReleaseTag("test-20261002-120000"), { tag: "test-20261002-120000", prerelease: true });
  assert.equal(parseReleaseTag("test-abc1234").prerelease, true);
  for (const tag of ["v1", "v1.2.3.4", "v01.2.3", "v1.2.3-beta", "test-", "test-../x", "test-a b", "test-x\n", "other"]) assert.throws(() => parseReleaseTag(tag), /tag/);
});
