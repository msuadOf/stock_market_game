type FocusTarget = Pick<HTMLElement, "isConnected" | "focus">;

export class MobileDetailFocus {
  private trigger: FocusTarget | null = null;
  private detailOpen = false;

  remember(trigger: FocusTarget | null) {
    if (!this.detailOpen) this.trigger = trigger;
  }

  apply(detailOpen: boolean, findDestination: () => FocusTarget | null) {
    if (detailOpen === this.detailOpen) return;
    this.detailOpen = detailOpen;
    const destination = !detailOpen && this.trigger?.isConnected ? this.trigger : findDestination();
    destination?.focus({ preventScroll: true });
    if (!detailOpen) this.trigger = null;
  }
}
