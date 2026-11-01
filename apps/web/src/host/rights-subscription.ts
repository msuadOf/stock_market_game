import { canonicalU64, companyCivilDate, nonempty } from "./company-contract-views.ts";
import { exact, record } from "../save/schema/primitives.ts";

export interface RightsSubscriptionReceipt { readonly event_id: string; readonly requested_shares: string; readonly submitted_on: string }

/** 校验认购请求及受理回执；缴款结果须另行查询。 */
export function createRightsSubscription(transport: (eventId: string, requestedShares: string) => Promise<unknown>): (eventId: string, requestedShares: string) => Promise<RightsSubscriptionReceipt> {
  return async (inputEvent, inputShares) => {
    const eventId = nonempty(inputEvent, "rights_subscription.event_id");
    const shares = canonicalU64(inputShares, "rights_subscription.requested_shares", true);
    const raw = record(await transport(eventId, shares), "rights_subscription.receipt");
    exact(raw, ["event_id", "requested_shares", "submitted_on"], "rights_subscription.receipt");
    const receipt = { event_id: nonempty(raw.event_id, "receipt.event_id"), requested_shares: canonicalU64(raw.requested_shares, "receipt.requested_shares", true), submitted_on: companyCivilDate(raw.submitted_on, "receipt.submitted_on") };
    if (receipt.event_id !== eventId || receipt.requested_shares !== shares) throw new Error("配股认购受理回执与请求不一致");
    return receipt;
  };
}
