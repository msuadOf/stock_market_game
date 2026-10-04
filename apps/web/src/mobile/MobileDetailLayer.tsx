import type { ReactNode } from "react";
import type { MobileUiState } from "./mobile-ui-state.ts";

export function MobileDetailLayer({ ui, children }: { ui: MobileUiState; children: ReactNode }) {
  return ui.detailCode === null ? null : <div className="mobile-detail-page">{children}</div>;
}
