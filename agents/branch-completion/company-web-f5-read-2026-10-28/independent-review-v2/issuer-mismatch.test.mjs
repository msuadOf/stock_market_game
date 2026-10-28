import assert from "node:assert/strict";
import test from "node:test";
import { prepareCatalogResolver } from "../../../../apps/web/src/host/catalog-resolver.ts";
import { createCompanyReadQueries } from "../../../../apps/web/src/host/company-read.ts";

test("审查复现：已存在但不匹配的 issuer/stock 不应作为真实配股事实接收", { timeout: 10000 }, async () => {
  const catalog = prepareCatalogResolver([{ id: 0, code: "600101" }, { id: 1, code: "600102" }], [
    { id: 7, code: "公司甲", name: "甲", listed_stock: 0 }, { id: 8, code: "公司乙", name: "乙", listed_stock: 1 },
  ]);
  const query = createCompanyReadQueries(async () => ({ owner: "0", value: { kind: "OwnerRightsOfferings", data: [{
    event_id: "rights-1", stock: "600101", issuer: 8, stage: "Approved", payment_window: "BeforeOpen", price_per_share: "100",
    payment_start_on: "2030-06-01", payment_deadline_on: "2030-06-03", ex_rights_on: "2030-06-04", settlement_on: "2030-06-05",
    owner_entitlement: null, open_subscription_remaining_shares: null, queued_subscription: null, settled_subscription: null,
  }] } }), () => catalog, "0");
  await assert.rejects(query.queryOwnerRightsOfferings(), /issuer|发行人|证券|映射/);
});
