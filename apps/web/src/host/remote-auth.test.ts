import assert from "node:assert/strict";
import test from "node:test";
import { RemoteAuthClient, credentialServerKey, parseCredentialToken } from "./remote-auth.ts";
import { parseRemoteMarketContext } from "./remote-market-context.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";

const token = "a".repeat(64);
const subject = { subject_id: "subject-1", username: "player" };
const context = { session_id: "market", setup: currentSaveFixture().setup, seed: "1", resumed: false, generation: "2", member: { account_id: "18446744073709551615", admission_funding: { external_cash: "1000000000000" } }, can_control: false, needs_rejoin: false };

test("认证真实调用 register/login/guest/me/logout，仅 Bearer 传递 credential", { timeout: 10000 }, async () => {
  const calls: { url: string; init: RequestInit }[] = [];
  const client = new RemoteAuthClient("https://example.test/", async (input, init) => {
    calls.push({ url: String(input), init: init! });
    if (String(input).endsWith("logout")) return new Response(null, { status: 204 });
    return Response.json(String(input).endsWith("me") ? subject : { subject, token }, { status: String(input).endsWith("register") || String(input).endsWith("guest") ? 201 : 200 });
  });
  await client.register("player", "password1");
  await client.login("player", "password1");
  await client.guest();
  assert.deepEqual(await client.me(token), subject);
  await client.logout(token);
  assert.deepEqual(calls.map((call) => call.url), ["register", "login", "guest", "me", "logout"].map((path) => `https://example.test/api/auth/${path}`));
  assert.equal(calls[3].init.headers && new Headers(calls[3].init.headers).get("authorization"), `Bearer ${token}`);
  assert.equal(calls[2].init.body, "{}");
});

test("凭据错误不 echo Server payload/password/token，不自动创建 guest", { timeout: 10000 }, async () => {
  let calls = 0;
  const client = new RemoteAuthClient("https://example.test", async () => { calls++; return new Response(`password1 ${token}`, { status: 401 }); });
  await assert.rejects(client.login("player", "password1"), (error: Error) => !error.message.includes(token) && !error.message.includes("password1") && error.message.includes("401"));
  assert.equal(calls, 1);
  assert.throws(() => parseCredentialToken("bad secret"), (error: Error) => !error.message.includes("bad secret"));
  assert.equal(credentialServerKey("HTTPS://EXAMPLE.test:443/"), "https://example.test");
  assert.throws(() => credentialServerKey("https://user:password@example.test"), /地址/);
});

test("context 精确保留 u64 account_id 与分金额，拒绝字段、规范及重入冲突", { timeout: 10000 }, () => {
  assert.deepEqual(parseRemoteMarketContext(context), context);
  for (const value of [
    { ...context, extra: true },
    { ...context, generation: "02" },
    { ...context, member: { ...context.member, account_id: "18446744073709551616" } },
    { ...context, member: { ...context.member, account_id: 1 } },
    { ...context, needs_rejoin: true },
  ]) assert.throws(() => parseRemoteMarketContext(value));
});

test("markets 只查询；join 明确确认且使用 generation，create 单独显式调用", { timeout: 10000 }, async () => {
  const calls: { url: string; init: RequestInit }[] = [];
  const client = new RemoteAuthClient("https://example.test", async (input, init) => {
    calls.push({ url: String(input), init: init! });
    return Response.json(String(input).endsWith("/markets") ? { markets: [context] } : context);
  });
  assert.equal((await client.markets(token)).length, 1);
  await client.join(token, { ...parseRemoteMarketContext(context), member: null, needs_rejoin: true }, true);
  assert.deepEqual(JSON.parse(calls[1].init.body as string), { session_id: "market", generation: "2", confirmed_rejoin: true });
  assert.equal(calls[1].url, "https://example.test/api/markets/join");
  await client.create(token, parseRemoteMarketContext(context).setup, "1");
  assert.equal(calls[2].url, "https://example.test/api/new");
});

test("未确认重新加入不发请求；坏列表、响应与错误只显式失败", { timeout: 10000 }, async () => {
  let calls = 0;
  const rejoin = { ...parseRemoteMarketContext(context), member: null, needs_rejoin: true };
  const client = new RemoteAuthClient("https://example.test", async () => { calls++; return Response.json({ markets: [context, context] }); });
  await assert.rejects(client.join(token, rejoin, false), /明确确认/);
  assert.equal(calls, 0);
  await assert.rejects(client.markets(token), /重复/);
  const corrupt = new RemoteAuthClient("https://example.test", async () => new Response(`password1 ${token}`));
  await assert.rejects(corrupt.me(token), (error: Error) => error.message.includes("JSON") && !error.message.includes(token));
  const network = new RemoteAuthClient("https://example.test", async () => { throw new Error(token); });
  await assert.rejects(network.me(token), (error: Error) => error.message.includes("网络") && !error.message.includes(token));
});
