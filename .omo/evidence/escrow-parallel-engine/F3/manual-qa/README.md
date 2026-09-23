# F3 real manual QA — Playwright five-scenario receipt

Status: **PASS**. Executed 2026-09-23 (Asia/Shanghai) against repository HEAD
`bf3d444f243cbc8447995648d5fe12556ebd9d02`.

## Source binding

```text
32c86da6b33b149abd03eca5c91d9d3fecc54e4ea44db1ee1ca5011e20d4c8ab  apps/web/e2e/trading-workflows.spec.ts
3813ebeb83351050cdec22951ce3787587ba1c7375be39b8c06f2d5ee86d3107  apps/web/src/App.tsx
6fea559725cdfa5dc543d9328e22c60e91562ccb0807da74689b9e51d9f8f7d4  apps/web/src/components/player-orders.ts
7319337e82ae074008a54abffbe5f90f6ee3fb7a4d61207c5a2a5597f8f94e4b  apps/web/src/host/wasm-worker.ts
```

The unrelated uncommitted edit in `docs/decisions/0018-long-running-immutable-timeline.md` was not
read, modified, staged, or included in this QA source binding.

## Invocation and bounds

The existing `trading-workflows.spec.ts` was executed once as one batch, with two workers. A
temporary, untracked Playwright config enabled `screenshot: "on"`, `trace: "on"`, a JSON reporter,
and the existing E2E-mode Vite build. It was deleted after the run.

```text
TMPDIR=/data1/baiyifan/workplace/stock_market_game/.tmp/process-tmp/f3-manual
TMP=/data1/baiyifan/workplace/stock_market_game/.tmp/process-tmp/f3-manual
TEMP=/data1/baiyifan/workplace/stock_market_game/.tmp/process-tmp/f3-manual
XDG_CACHE_HOME=/data1/baiyifan/workplace/stock_market_game/.tmp/cache/f3-manual
node scripts/run-long-validation.mjs 300000 -- \
  apps/web/node_modules/.bin/playwright test \
  --config apps/web/f3-playwright.config.ts --workers=2
```

The temporary web-server command used only workspace-local installed binaries:

```text
apps/web/node_modules/.bin/tsc -b &&
apps/web/node_modules/.bin/vite build --mode e2e &&
apps/web/node_modules/.bin/vite preview --host 127.0.0.1 --port 4174
```

Result: **5 passed, 0 failed, 0 skipped, 0 flaky**, 28.736 seconds total. The 300000ms outer
supervisor covers build, server startup, both workers, reporting, shutdown, and cleanup. The
machine-readable result is `playwright-results.json`; the complete terminal output is
`playwright.log`.

## Scenario and assertion mapping

Every scenario starts from a 1440×900 page loaded with `?tradingE2E=1`, no visible `.app-error`,
an available E2E control capability, and authoritative game tick 0.

| Scenario | Required actions and assertions reached | Duration | Screenshot / trace |
|---|---|---:|---|
| Opening auction market-order rejection | Select market buy; price input disabled; advance one tick; visible “集合竞价仅接受限价委托”; no player order. | 2.881s | `artifacts/trading-workflows-开盘集合竞价明确拒绝市价委托/test-finished-1.png`; adjacent `trace.zip` |
| 09:15–09:20 cancellation | Submit 100-share limit buy at ¥10.08; observe frozen cash/order; cancel and advance; order disappears and empty-state text is visible. | 3.147s | `artifacts/trading-workflows-09-15–09-20-的未成交限价委托可撤销并释放冻结/test-finished-1.png`; adjacent `trace.zip` |
| 09:20–09:25 cancellation rejection | Submit the same auction limit order; after the deadline, cancellation shows “集合竞价委托当前不可撤销” and remains frozen; after opening it becomes `continuous` and can be canceled. | 4.082s | `artifacts/trading-workflows-09-20–09-25-的集合竞价委托不可撤销且继续冻结/test-finished-1.png`; adjacent `trace.zip` |
| Continuous order plus insufficient funds | At continuous tick 9, submit a limit buy; activity list shows frozen cash and available cash decreases; a 1,000,000-share buy visibly reports “资金不足” without removing the valid order. | 4.185s | `artifacts/trading-workflows-连续竞价展示活动委托冻结，并明确拒绝资金不足的买单/test-finished-1.png`; adjacent `trace.zip` |
| Save, refresh, restore, continue | Save a live continuous order; reload and restore; available cash, positions, activity list and tick exactly equal their pre-refresh values; advancing one more tick succeeds. | 9.152s | `artifacts/trading-workflows-本地存档经刷新读档后保留资金、持仓与活动委托，并可继续推进/test-finished-1.png`; adjacent `trace.zip` |

Trace audit found **zero browser console-error events and zero page-error events** across all five
trace archives. The assertions therefore cover visible expected failures without a silent console
failure.

## Artifact hashes

```text
db07e6e3e1a183b7dea9e94fb41b9c0169384bf8b0103a057bdb37c406001b18  opening-market-rejection screenshot
033fcf8a6418cc94aa15446d62decf4b3eb35e5d8368dfb213134d926b64a780  early-auction-cancel screenshot
f21776b4b48f03b730b971fb2fb14086d5e8be5ebfc1dfcb664a9bbd39870531  cancellation-deadline screenshot
ed8dbe5a2757a510815a0d2f588afffa13209bc7e430bf120df7c405a7ab8a0d  continuous-and-insufficient-funds screenshot
18a7d79704624385ffb78deecc9840c7918962daebce58761ff19af1dab865a9  save-refresh-restore screenshot
6cffe6aae9900d43e746be7e5b2b51d58e77fc96aae9eceafa36906b40b4c360  playwright.log
b9653b95ecb80c6d8b5824e174417bdda1263c4d91b3f3ddc842935932aac142  playwright-results.json
```

`preflight-corepack-failure.log` and `preflight-config-resolution-failure.log` record two honest
preflight failures that occurred before Playwright loaded any test. They are not counted as test
runs or PASS evidence.
