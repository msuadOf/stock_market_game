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
  for (const filename of ["ci.yml", "distributions.yml"]) assert.match(workflow(filename), /workflow_call:/);
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

test("release gates all distributions and Pages on validation and CI, publishes only current-run verified assets", () => {
  const text = workflow("release.yml");
  assert.match(text, /uses: \.\/\.github\/workflows\/ci.yml/);
  assert.match(text, /uses: \.\/\.github\/workflows\/distributions.yml/);
  assert.match(text, /needs: \[validate, ci\]/);
  assert.match(text, /needs: \[validate, distributions\]/);
  assert.match(text, /scripts\/publish-release.mjs/);
  assert.match(text, /contents: write/);
  assert.match(text, /actions\/download-artifact@v4/);
  assert.doesNotMatch(text, /merge-multiple: true|continue-on-error|release create.*\$\{\{/);
  assert.match(text, /uses: \.\/\.github\/workflows\/build-web.yml/);
  assert.match(text, /cancel-in-progress: false\s+queue: max/);
});

test("release accepts only numeric versions or nonempty safe test suffixes", async () => {
  const { parseReleaseTag } = await import("./release-policy.mjs");
  assert.deepEqual(parseReleaseTag("v1.2.3"), { tag: "v1.2.3", prerelease: false });
  assert.deepEqual(parseReleaseTag("test-20261002-120000"), { tag: "test-20261002-120000", prerelease: true });
  assert.equal(parseReleaseTag("test-abc1234").prerelease, true);
  for (const tag of ["v1", "v1.2.3.4", "v01.2.3", "v1.2.3-beta", "test-", "test-../x", "test-a b", "test-x\n", "other"]) assert.throws(() => parseReleaseTag(tag), /tag/);
});
