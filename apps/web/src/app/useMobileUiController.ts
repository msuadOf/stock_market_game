import { useEffect, useReducer, useRef } from "react";
import { STOCK_NAMES } from "../config/defaults";
import {
  initialMobileUiState,
  reduceMobileUi,
  type MobileInfoTab,
  type MobilePrimaryTab,
} from "../mobile/mobile-ui-state";

/** Owns mobile navigation, layer focus management, and page-title synchronization. */
export function useMobileUiController(orientation: "portrait" | "landscape") {
  const [mobileUi, dispatchMobileUi] = useReducer(reduceMobileUi, initialMobileUiState);
  const mobileTab = mobileUi.primaryTab;
  const tradeSheetOpen = mobileUi.tradeSheetOpen;
  const mobileDetail = mobileUi.detailCode !== null;
  const tradeSheetRef = useRef<HTMLDivElement | null>(null);
  const tradeSheetTriggerRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    document.title = mobileDetail
      ? `${STOCK_NAMES[mobileUi.detailCode ?? ""] ?? mobileUi.detailCode} — 股票模拟游戏`
      : "股票模拟游戏";
  }, [mobileDetail, mobileUi.detailCode]);

  useEffect(() => {
    if (!tradeSheetOpen || orientation !== "portrait") return;
    const dialog = tradeSheetRef.current;
    if (!dialog) return;
    const focusableSelector = 'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';
    const focusables = () => Array.from(dialog.querySelectorAll<HTMLElement>(focusableSelector));
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        dispatchMobileUi({ type: "close-top-layer" });
        return;
      }
      if (event.key !== "Tab") return;
      const items = focusables();
      if (items.length === 0) return;
      const first = items[0];
      const last = items.at(-1)!;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    requestAnimationFrame(() => focusables()[0]?.focus());
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      tradeSheetTriggerRef.current?.focus();
    };
  }, [orientation, tradeSheetOpen]);

  function switchMobileTab(tab: MobilePrimaryTab) {
    dispatchMobileUi({ type: "switch-primary", tab });
  }

  function openTradeSheet() {
    tradeSheetTriggerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dispatchMobileUi({ type: "open-trade" });
  }

  function closeTradeSheet() {
    dispatchMobileUi({ type: "close-top-layer" });
  }

  function showDetailInfo(tab: MobileInfoTab) {
    dispatchMobileUi({ type: "select-info", tab });
    requestAnimationFrame(() => document.querySelector(".msd-info-tabs")?.scrollIntoView({ block: "start" }));
  }

  return {
    mobileUi,
    dispatchMobileUi,
    mobileTab,
    tradeSheetOpen,
    mobileDetail,
    tradeSheetRef,
    switchMobileTab,
    openTradeSheet,
    closeTradeSheet,
    showDetailInfo,
  };
}
