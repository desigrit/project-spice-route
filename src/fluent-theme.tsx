import { createDarkTheme, createLightTheme, FluentProvider, type BrandVariants } from "@fluentui/react-components";
import { useEffect, useState, type ReactNode } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ThemeMode } from "./types";

const ocean: BrandVariants = {
  10: "#081724", 20: "#102438", 30: "#102F4C", 40: "#173D5B",
  50: "#204B69", 60: "#295A79", 70: "#336B8B", 80: "#3E7D9E",
  90: "#548FAD", 100: "#6BA2BC", 110: "#85B4CA", 120: "#A0C7D7",
  130: "#BED9E3", 140: "#D8E7E9", 150: "#EAEFEA", 160: "#F5E8C5",
};
const font = 'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI Variable", "Segoe UI", sans-serif';
const common = { fontFamilyBase: font, borderRadiusMedium: "8px", borderRadiusLarge: "12px" };
const light = { ...createLightTheme(ocean), ...common, colorBrandBackground: ocean[30], colorBrandBackgroundHover: ocean[40], colorBrandBackgroundPressed: ocean[20], colorBrandForeground1: ocean[30], colorCompoundBrandBackground: ocean[30], colorNeutralBackground1: "#FFFDF7", colorNeutralForeground1: "#17354D" };
const dark = { ...createDarkTheme(ocean), ...common, colorBrandBackground: "#F5E8C5", colorBrandBackgroundHover: "#FFF1CF", colorBrandBackgroundPressed: "#DCCEAD", colorNeutralForegroundOnBrand: ocean[20], colorBrandForeground1: "#F5E8C5", colorCompoundBrandBackground: "#F5E8C5", colorNeutralBackground1: "#17354D", colorNeutralForeground1: "#F5EDD9" };

export function AppTheme({ mode, children }: { mode: ThemeMode; children: ReactNode }) {
  const [systemDark, setSystemDark] = useState(() => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false);
  useEffect(() => {
    const query = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!query) return;
    const change = () => setSystemDark(query.matches);
    query.addEventListener("change", change);
    return () => query.removeEventListener("change", change);
  }, []);
  const resolved = mode === "system" ? (systemDark ? "dark" : "light") : mode;
  useEffect(() => {
    document.documentElement.dataset.theme = resolved;
    if (isTauri()) {
      document.documentElement.dataset.native = "true";
      void getCurrentWindow().setTheme(mode === "system" ? null : mode).catch(() => {
        // The application theme remains usable if native theme control is unavailable.
      });
    }
  }, [mode, resolved]);
  return <FluentProvider theme={resolved === "dark" ? dark : light} className="app-provider">{children}</FluentProvider>;
}
