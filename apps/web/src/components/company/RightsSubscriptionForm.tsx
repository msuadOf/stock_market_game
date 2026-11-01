/* oxlint-disable react/only-export-components -- 纯校验供测试复用 */
import { useState } from "react";
import type { OwnerRightsOfferingView } from "../../host/company-contract-views.ts";

export type RightsSubscription = (eventId: string, requestedShares: string) => Promise<unknown>;
type RightsStatus = Pick<OwnerRightsOfferingView, "event_id" | "payment_window" | "owner_entitlement" | "open_subscription_remaining_shares" | "queued_subscription" | "settled_subscription">;

export function validSubscriptionShares(value: string): boolean {
  return /^[1-9][0-9]*$/.test(value) && BigInt(value) <= 18446744073709551615n;
}

export function canSubscribe(view: RightsStatus): boolean {
  const available = view.owner_entitlement?.rights_shares ?? view.open_subscription_remaining_shares;
  return view.payment_window === "Open" && available !== null && BigInt(available) > 0n && view.queued_subscription === null && view.settled_subscription === null;
}

export function RightsSubscriptionForm({ view, onSubscribe, onAccepted }: { readonly view: RightsStatus; readonly onSubscribe: RightsSubscription | undefined; readonly onAccepted: () => void }) {
  const [shares, setShares] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [accepted, setAccepted] = useState(false);
  if (onSubscribe === undefined) return <p>当前宿主不支持配股认购。</p>;
  if (!canSubscribe(view)) return null;
  const submit = async () => {
    if (pending || !validSubscriptionShares(shares)) return;
    setPending(true);
    setError(null);
    try {
      await onSubscribe(view.event_id, shares);
      setAccepted(true);
      onAccepted();
    } catch (failure: unknown) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setPending(false);
    }
  };
  return <div>
    <p>排队受理不代表缴款成功，日终结果以查询为准。</p>
    <label>认购股数<input aria-label={`事件 ${view.event_id} 认购股数`} inputMode="numeric" value={shares} onChange={(event) => setShares(event.currentTarget.value)} /></label>
    <button type="button" disabled={pending || accepted || !validSubscriptionShares(shares)} onClick={() => { void submit(); }}>{pending ? "提交中…" : "提交认购"}</button>
    {accepted && <p role="status">认购已排队受理。</p>}
    {error !== null && <p role="alert">认购提交失败：{error}</p>}
  </div>;
}
