/**
 * useTheme：把 settingsSlice.theme 同步到 <html data-theme>。
 * 颜色变量在 index.css 按主题定义。
 */
import { useEffect } from "react";
import { useSelector } from "react-redux";
import type { RootState } from "../store/store";

export function useTheme(): "light" | "dark" {
  const theme = useSelector((s: RootState) => s.settings.theme);
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);
  return theme;
}
