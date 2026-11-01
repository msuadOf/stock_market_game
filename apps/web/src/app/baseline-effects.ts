export function applyBaselineEffects(continuation: boolean, effects: {
  readonly replaceSession: () => void;
  readonly installView: () => void;
  readonly refreshOrders: () => void;
}): void {
  if (!continuation) effects.replaceSession();
  effects.installView();
  effects.refreshOrders();
}
