import { create } from "zustand";
import { packs as initialPacks, profiles as initialProfiles } from "../data/catalog";
import type { AppProfile, LanguagePack, OverlayStyle, Responsiveness } from "../data/catalog";
import type { LocaleCode } from "../i18n";

export type Theme = "dark" | "light";

/** Overlay watch area in percentages of the window (0–100), so DPI never matters. */
export type RegionRect = { x: number; y: number; w: number; h: number };

export const DEFAULT_REGION: RegionRect = { x: 0, y: 0, w: 100, h: 100 };

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
  interfaceLanguage: LocaleCode;
  trayMinimise: boolean;
  trayNotify: boolean;
  trayQuickToggle: boolean;
  hotkeysEnabled: boolean;
  onboardingDone: boolean;
  region: RegionRect;
  draftRegion: RegionRect;
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
  interfaceLanguage: "ru",
  trayMinimise: true,
  trayNotify: true,
  trayQuickToggle: true,
  hotkeysEnabled: true,
  onboardingDone: false,
  region: { ...DEFAULT_REGION },
  draftRegion: { ...DEFAULT_REGION },
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
  completeOnboarding: () => void;
  skipOnboarding: () => void;
  setDraftRegion: (region: RegionRect) => void;
  applyDraftRegion: () => void;
  saveRegion: () => void;
};

export const useStore = create<Store>((set, get) => ({
  ...recommended,
  running: true,
  profiles: initialProfiles,
  packs: initialPacks,
  toast: null,
  set: (key, value) => set({ [key]: value } as Partial<Store>),
  reset: () =>
    set({
      ...recommended,
      // Keep onboarding state; the caller raises a localized toast.
      onboardingDone: get().onboardingDone,
      toast: null,
    }),
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
    })),
  removePack: (id) =>
    set((state) => ({
      packs: state.packs.map((pack) => (pack.id === id ? { ...pack, state: "available", progress: undefined } : pack)),
    })),
  notify: (message) => set({ toast: message }),
  completeOnboarding: () => set({ onboardingDone: true, running: true }),
  skipOnboarding: () => set({ onboardingDone: true }),
  setDraftRegion: (region) => set({ draftRegion: region }),
  applyDraftRegion: () => set({ region: { ...get().draftRegion } }),
  saveRegion: () => set((state) => ({ region: { ...state.draftRegion } })),
}));

export const currentProfile = (profiles: AppProfile[]) =>
  profiles.find((profile) => profile.lastSeen === "Running now") ?? profiles[0];
