import { useEffect, useMemo } from "react";
import { useI18nStore } from "../i18n";
import { useStore, type RegionRect } from "./store";

export type HotkeyAction = "toggleOverlay" | "peekOriginal" | "openShell" | "gotoHome" | "gotoApps" | "gotoPacks" | "gotoRegion" | "gotoSettings";

type Binding = { combo: string; action: HotkeyAction; description: string };

/** Shell shortcuts that work while the window has focus (and map to global ones in Tauri). */
export const BINDINGS: Binding[] = [
  { combo: "Alt+KeyT", action: "toggleOverlay", description: "Toggle overlay" },
  { combo: "Alt+KeyQ", action: "peekOriginal", description: "Hold to show original" },
  { combo: "Alt+Shift+KeyO", action: "openShell", description: "Open Lumen" },
  { combo: "Alt+Digit1", action: "gotoHome", description: "Overview" },
  { combo: "Alt+Digit2", action: "gotoApps", description: "Apps" },
  { combo: "Alt+Digit3", action: "gotoPacks", description: "Languages" },
  { combo: "Alt+Digit4", action: "gotoRegion", description: "Region" },
  { combo: "Alt+Digit5", action: "gotoSettings", description: "Settings" },
];

export function comboFromEvent(event: KeyboardEvent): string {
  const parts: string[] = [];
  if (event.altKey) parts.push("Alt");
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Meta");
  parts.push(event.code);
  return parts.join("+");
}

type Handlers = Partial<Record<HotkeyAction, { run: () => void; hold?: boolean }>>;

/**
 * Registers in-window shortcuts. When `enabled` is false nothing fires, matching the
 * hotkeys setting; the Tauri shell reads the same setting for global registration.
 */
export function useHotkeys(enabled: boolean, handlers: Handlers) {
  const handlersRef = useMemo(() => ({ handlers }), [handlers]);

  useEffect(() => {
    if (!enabled) return;
    const map = new Map(BINDINGS.map((binding) => [binding.combo, binding.action]));
    const down = (event: KeyboardEvent) => {
      if (event.repeat && comboFromEvent(event) === "Alt+KeyQ") return;
      const action = map.get(comboFromEvent(event));
      if (!action) return;
      const handler = handlersRef.handlers[action];
      if (!handler) return;
      if (action === "peekOriginal" && handler.hold) return;
      event.preventDefault();
      handler.run();
    };
    const up = (event: KeyboardEvent) => {
      if (comboFromEvent(event) !== "Alt+KeyQ") return;
      handlersRef.handlers.peekOriginal?.run();
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  }, [enabled, handlersRef]);
}

/** Keeps interface language and draft region in sync with the main store. */
export function useSyncI18nWithStore() {
  const interfaceLanguage = useStore((state) => state.interfaceLanguage);
  const setLocale = useI18nStore((state) => state.setLocale);
  useEffect(() => {
    setLocale(interfaceLanguage);
  }, [interfaceLanguage, setLocale]);
}

export type { RegionRect };
