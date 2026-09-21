export type Language = { value: string; label: string; native: string };

export const languages: Language[] = [
  { value: "ru", label: "Russian", native: "Русский" },
  { value: "en", label: "English", native: "English" },
  { value: "de", label: "German", native: "Deutsch" },
  { value: "fr", label: "French", native: "Français" },
  { value: "es", label: "Spanish", native: "Español" },
  { value: "pt", label: "Portuguese", native: "Português" },
  { value: "it", label: "Italian", native: "Italiano" },
  { value: "pl", label: "Polish", native: "Polski" },
  { value: "uk", label: "Ukrainian", native: "Українська" },
  { value: "tr", label: "Turkish", native: "Türkçe" },
  { value: "ja", label: "Japanese", native: "日本語" },
  { value: "ko", label: "Korean", native: "한국어" },
  { value: "zh", label: "Chinese (Simplified)", native: "简体中文" },
  { value: "ar", label: "Arabic", native: "العربية" },
  { value: "he", label: "Hebrew", native: "עברית" },
  { value: "hi", label: "Hindi", native: "हिन्दी" },
  { value: "vi", label: "Vietnamese", native: "Tiếng Việt" },
  { value: "th", label: "Thai", native: "ไทย" },
  { value: "cs", label: "Czech", native: "Čeština" },
  { value: "nl", label: "Dutch", native: "Nederlands" },
];

export type AppProfile = {
  id: string;
  name: string;
  process: string;
  enabled: boolean;
  source: string;
  target: string;
  style: OverlayStyle;
  lastSeen: string;
  antiCheat?: string;
};

export type OverlayStyle = "seamless" | "plate" | "subtitles";
export type Responsiveness = "fast" | "balanced" | "accurate";

export const profiles: AppProfile[] = [
  {
    id: "yoru-no-kagami",
    name: "Yoru no Kagami",
    process: "yoru.exe",
    enabled: true,
    source: "ja",
    target: "ru",
    style: "seamless",
    lastSeen: "Running now",
    antiCheat: "Easy Anti-Cheat",
  },
  {
    id: "steuerprofi",
    name: "SteuerProfi 2026",
    process: "steuerprofi.exe",
    enabled: true,
    source: "de",
    target: "ru",
    style: "plate",
    lastSeen: "Today, 12:41",
  },
  {
    id: "orbita-launcher",
    name: "Orbita Launcher",
    process: "orbita.exe",
    enabled: false,
    source: "en",
    target: "ru",
    style: "seamless",
    lastSeen: "Yesterday",
  },
  {
    id: "mpc-player",
    name: "Media Player Classic",
    process: "mpc-hc64.exe",
    enabled: true,
    source: "ko",
    target: "ru",
    style: "subtitles",
    lastSeen: "12 March",
  },
];

export type LanguagePack = {
  id: string;
  pair: string;
  size: string;
  state: "installed" | "update" | "available" | "downloading";
  progress?: number;
};

export const packs: LanguagePack[] = [
  { id: "ja-ru", pair: "Japanese → Russian", size: "182 MB", state: "installed" },
  { id: "en-ru", pair: "English → Russian", size: "164 MB", state: "installed" },
  { id: "de-ru", pair: "German → Russian", size: "171 MB", state: "update" },
  { id: "zh-ru", pair: "Chinese → Russian", size: "189 MB", state: "downloading", progress: 62 },
  { id: "ko-ru", pair: "Korean → Russian", size: "176 MB", state: "available" },
  { id: "fr-ru", pair: "French → Russian", size: "168 MB", state: "available" },
];
