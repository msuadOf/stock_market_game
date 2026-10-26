import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let vite: ViteDevServer;
let MechanismSwitchesInput: typeof import("./MechanismSwitchesInput.tsx").MechanismSwitchesInput;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  ({ MechanismSwitchesInput } = await vite.ssrLoadModule("/src/components/MechanismSwitchesInput.tsx"));
});
after(async () => { await vite.close(); });
type Props = { children?: React.ReactNode; name?: string; checked?: boolean; onChange?: (event: { currentTarget: { checked: boolean } }) => void };
function nodes(tree: React.ReactNode): React.ReactElement<Props>[] {
  if (Array.isArray(tree)) return tree.flatMap(nodes);
  if (!React.isValidElement<Props>(tree)) return [];
  return [tree, ...nodes(tree.props.children)];
}
test("配股与回购开关独立回调，受控 bool 可来回切换", { timeout: 10000 }, () => {
  let rights = false; let repurchase = false;
  const writes: [string, boolean][] = [];
  function tree() { return MechanismSwitchesInput({ rightsOfferingEnabled: rights, issuerRepurchaseEnabled: repurchase,
    onRightsOfferingChange: (value) => { rights = value; writes.push(["rights", value]); },
    onIssuerRepurchaseChange: (value) => { repurchase = value; writes.push(["repurchase", value]); } }); }
  function change(name: string, checked: boolean) {
    const input = nodes(tree()).find((node) => node.props.name === name); assert.ok(input?.props.onChange);
    input.props.onChange({ currentTarget: { checked } });
  }
  assert.equal(nodes(tree()).filter((node) => node.type === "input" && node.props.checked === false).length, 2);
  change("rights-offering-enabled", true); assert.equal(rights, true); assert.equal(repurchase, false);
  change("issuer-repurchase-enabled", true); assert.equal(rights, true); assert.equal(repurchase, true);
  change("rights-offering-enabled", false); assert.equal(rights, false); assert.equal(repurchase, true);
  change("issuer-repurchase-enabled", false);
  assert.deepEqual(writes, [["rights", true], ["repurchase", true], ["rights", false], ["repurchase", false]]);
});
test("默认关闭及同局不可切可见，FullByDefault 如实解释本人现金与弃配", { timeout: 10000 }, () => {
  const tree = MechanismSwitchesInput({ rightsOfferingEnabled: false, issuerRepurchaseEnabled: false, onRightsOfferingChange: () => {}, onIssuerRepurchaseChange: () => {} });
  const markup = renderToStaticMarkup(tree);
  assert.match(markup, /默认关闭/); assert.match(markup, /开局后不可修改/);
  assert.match(markup, /FullByDefault/); assert.match(markup, /本人真实现金/); assert.match(markup, /不足部分.*放弃.*记录/);
  assert.equal(nodes(tree).filter((node) => node.type === "select" || node.type === "option").length, 0, "不提供未实现策略提交入口");
});
