import { create } from "zustand";
import { packs as initialPacks, profiles as initialProfiles } from "../data/catalog";
import type { AppProfile, LanguagePack, OverlayStyle, Responsiveness } from "../data/catalog";

export type Theme = "dark" | "light";

type Settings = {
  targetLanguage: string;
  overlayStyle: OverlayStyle;
  responsiveness: Responsiveness;
  overlayOpacity: number;
  textScale: number;
  edgeSoftness: number;
  animationSpeed: number;
  uiScale: number;
  resourceCap: number;
  autostart: boolean;
  offlineOnly: boolean;
  pauseOnPasswordFields: boolean;
  theme: Theme;
};

const recommended: Settings = {
  targetLanguage: "ru",
  overlayStyle: "seamless",
  responsiveness: "balanced",
  overlayOpacity: 100,
  textScale: 100,
  edgeSoftness: 40,
  animationSpeed: 100,
  uiScale: 100,
  resourceCap: 60,
  autostart: true,
  offlineOnly: true,
  pauseOnPasswordFields: true,
  theme: "dark",
};

type Store = Settings & {
  running: boolean;
  profiles: AppProfile[];
  packs: LanguagePack[];
  toast: string | null;
  set: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
  reset: () => void;
  toggleRunning: () => void;
  toggleProfile: (id: string) => void;
  updateProfile: (id: string, patch: Partial<AppProfile>) => void;
  installPack: (id: string) => void;
  removePack: (id: string) => void;
  notify: (message: string | null) => void;
};

export const useStore = create<Store>((set) => ({
  ...recommended,
  running: true,
  profiles: initialProfiles,
  packs: initialPacks,
  toast: null,
  set: (key, value) => set({ [key]: value } as Partial<Store>),
  reset: () => set({ ...recommended, toast: "Settings restored to recommended values." }),
  toggleRunning: () => set((state) => ({ running: !state.running })),
  toggleProfile: (id) =>
    set((state) => ({
      profiles: state.profiles.map((profile) =>
        profile.id === id ? { ...profile, enabled: !profile.enabled } : profile,
      ),
    })),
  updateProfile: (id, patch) =>
    set((state) => ({
      profiles: state.profiles.map((profile) => (profile.id === id ? { ...profile, ...patch } : profile)),
    })),
  installPack: (id) =>
    set((state) => ({
      packs: state.packs.map((pack) => (pack.id === id ? { ...pack, state: "installed", progress: undefined } : pack)),
      toast: "Language pack installed.",
    })),
  removePack: (id) =>
    set((state) => ({
      packs: state.packs.map((pack) => (pack.id === id ? { ...pack, state: "available", progress: undefined } : pack)),
      toast: "Language pack removed.",
    })),
  notify: (message) => set({ toast: message }),
}));

export const currentProfile = (profiles: AppProfile[]) =>
  profiles.find((profile) => profile.lastSeen === "Running now") ?? profiles[0];
