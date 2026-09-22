import { create } from "zustand";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";

export type UpdaterStatus =
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "ready"
  | "failed"
  | "unavailable";

type UpdaterStore = {
  status: UpdaterStatus;
  /** Version offered by the release endpoint, when one is pending. */
  version: string | null;
  downloaded: number;
  /** Total bytes when the endpoint reports a content length. */
  total: number | null;
  error: string | null;
  checkForUpdates: () => Promise<UpdaterStatus>;
  downloadAndInstall: () => Promise<void>;
  restartApp: () => Promise<void>;
};

/** True only inside the desktop shell; the browser preview has no updater bridge. */
const runsUnderTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// The pending Update handle lives outside the store: it is a live resource, not state.
let pendingUpdate: Update | null = null;

const describe = (error: unknown) => (error instanceof Error ? error.message : String(error));

export const useUpdater = create<UpdaterStore>((set, get) => ({
  status: "idle",
  version: null,
  downloaded: 0,
  total: null,
  error: null,
  checkForUpdates: async () => {
    if (!runsUnderTauri()) {
      set({ status: "unavailable" });
      return "unavailable";
    }
    const current = get().status;
    if (current === "checking" || current === "downloading") {
      return current;
    }
    set({ status: "checking", error: null });
    try {
      const update = await check();
      if (!update) {
        pendingUpdate = null;
        set({ status: "upToDate", version: null });
        return "upToDate";
      }
      pendingUpdate = update;
      set({ status: "available", version: update.version, downloaded: 0, total: null });
      return "available";
    } catch (error) {
      pendingUpdate = null;
      set({ status: "failed", error: describe(error) });
      return "failed";
    }
  },
  downloadAndInstall: async () => {
    const update = pendingUpdate;
    if (!update || get().status !== "available") {
      return;
    }
    set({ status: "downloading", downloaded: 0, total: null, error: null });
    const onEvent = (event: DownloadEvent) => {
      if (event.event === "Started") {
        set({ total: event.data.contentLength ?? null });
      } else if (event.event === "Progress") {
        const chunk = event.data.chunkLength;
        set((state) => ({ downloaded: state.downloaded + chunk }));
      }
    };
    try {
      await update.downloadAndInstall(onEvent);
      pendingUpdate = null;
      set({ status: "ready" });
    } catch (error) {
      pendingUpdate = null;
      set({ status: "failed", error: describe(error) });
    }
  },
  restartApp: async () => {
    try {
      await relaunch();
    } catch (error) {
      set({ status: "failed", error: describe(error) });
    }
  },
}));
