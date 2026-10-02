import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const api = () => import("../apps/web/src/pages-isolation.ts");

function browser(extra = {}) {
  const navigations = [];
  const registrations = [];
  const listeners = new Set();
  const serviceWorker = {
    controller: null,
    addEventListener: (_type, listener) => listeners.add(listener),
    removeEventListener: (_type, listener) => listeners.delete(listener),
    register: async (url, options) => {
      registrations.push([url, options]);
      serviceWorker.controller = {};
      for (const listener of listeners) listener();
      return {};
    },
  };
  return { isolated: false, secure: true, serviceWorker, href: "https://owner.github.io/game/?user=1#chart",
    replace: (url) => navigations.push(url), cleanUrl: (url) => navigations.push(url),
    navigations, registrations, listeners, ...extra };
}

test("Pages registers only its repository scope and reloads once before mounting the game", async () => {
  const { ensurePagesIsolation } = await api();
  const context = browser();
  assert.equal(await ensurePagesIsolation("/game/", context), false);
  assert.deepEqual(context.registrations, [["https://owner.github.io/game/pages-isolation.js", { scope: "/game/", updateViaCache: "none" }]]);
  const next = new URL(context.navigations[0]);
  assert.equal(next.searchParams.get("user"), "1");
  assert.equal(next.hash, "#chart");
  assert.equal(next.searchParams.get("__stock_pages_isolation"), "1");
  assert.equal(context.listeners.size, 0);
  const controlled = browser({ href: next.href, isolated: true });
  assert.equal(await ensurePagesIsolation("/game/", controlled), true);
  assert.equal(controlled.registrations.length, 0);
  assert.equal(controlled.navigations[0], "https://owner.github.io/game/?user=1#chart");
});

test("unsupported browsers, failed registration, timeout and failed isolation are explicit; no reload loop", async () => {
  const { ensurePagesIsolation } = await api();
  await assert.rejects(ensurePagesIsolation("/game/", browser({ secure: false })), /HTTPS/);
  await assert.rejects(ensurePagesIsolation("/game/", browser({ serviceWorker: undefined })), /Service Worker/);
  for (const base of ["//evil/", "/../", "/./", "/game/\n"]) await assert.rejects(ensurePagesIsolation(base, browser()), /路径/);
  const repeated = browser({ href: "https://owner.github.io/game/?__stock_pages_isolation=1" });
  await assert.rejects(ensurePagesIsolation("/game/", repeated), /跨源隔离/);
  assert.equal(repeated.navigations.length, 0);
  const rejected = browser();
  rejected.serviceWorker.register = async () => { throw new Error("registration denied"); };
  await assert.rejects(ensurePagesIsolation("/game/", rejected), /registration denied/);
  assert.equal(rejected.listeners.size, 0);
  const stalled = browser();
  stalled.serviceWorker.register = async () => ({});
  await assert.rejects(ensurePagesIsolation("/game/", stalled, 10), /超时/);
  assert.equal(stalled.listeners.size, 0);
});

function worker(fetch) {
  const handlers = new Map();
  const self = { registration: { scope: "https://owner.github.io/game/" },
    addEventListener: (name, callback) => handlers.set(name, callback),
    skipWaiting: async () => {}, clients: { claim: async () => {} } };
  runInNewContext(readFileSync(new URL("../apps/web/public/pages-isolation.js", import.meta.url), "utf8"), { self, URL, Headers, Response, fetch });
  return handlers;
}

test("isolation worker preserves response bytes, status, content type and request credentials without caching", async () => {
  const request = new Request("https://owner.github.io/game/assets/engine.wasm", { credentials: "include" });
  let forwarded;
  const handlers = worker(async (input) => {
    forwarded = input;
    return new Response("wasm bytes", { status: 206, headers: { "Content-Type": "application/wasm" } });
  });
  let response;
  handlers.get("fetch")({ request, respondWith: (promise) => { response = promise; } });
  const result = await response;
  assert.equal(forwarded, request);
  assert.equal(result.status, 206);
  assert.equal(result.headers.get("Content-Type"), "application/wasm");
  assert.equal(result.headers.get("Cross-Origin-Opener-Policy"), "same-origin");
  assert.equal(result.headers.get("Cross-Origin-Embedder-Policy"), "require-corp");
  assert.equal(await result.text(), "wasm bytes");
});

test("worker leaves remote API, other repositories and non-GET requests untouched and propagates errors", async () => {
  const failure = new Error("network failed");
  const handlers = worker(async () => { throw failure; });
  for (const request of [new Request("https://server.example/api"), new Request("https://owner.github.io/other/"), new Request("https://owner.github.io/game/api"), new Request("https://owner.github.io/game/api", { method: "POST" })]) {
    handlers.get("fetch")({ request, respondWith: () => assert.fail("outside static resource scope") });
  }
  let response;
  handlers.get("fetch")({ request: new Request("https://owner.github.io/game/"), respondWith: (promise) => { response = promise; } });
  await assert.rejects(response, (error) => error === failure);
});
