import { useMemo } from "react";
import { create } from "zustand";
import { en, type MessageKey } from "./locales/en";
import { ru } from "./locales/ru";

export type LocaleCode = "en" | "ru";

export const localeOptions: { value: LocaleCode; label: string; native: string }[] = [
  { value: "en", label: "English", native: "English" },
  { value: "ru", label: "Russian", native: "Русский" },
];

const catalogs: Record<LocaleCode, Partial<Record<MessageKey, string>>> = { en, ru };

export type Params = Record<string, string | number>;

function interpolate(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, key: string) =>
    Object.prototype.hasOwnProperty.call(params, key) ? String(params[key]) : match,
  );
}

/** Resolve a key in the active locale, falling back to English, then the key itself. */
export function translate(locale: LocaleCode, key: MessageKey, params?: Params): string {
  const template = catalogs[locale][key] ?? en[key] ?? key;
  return interpolate(template, params);
}

type I18nStore = {
  locale: LocaleCode;
  setLocale: (locale: LocaleCode) => void;
};

export const useI18nStore = create<I18nStore>((set) => ({
  locale: "ru",
  setLocale: (locale) => set({ locale }),
}));

/** Bound translator for the current locale. */
export function useT() {
  const locale = useI18nStore((state) => state.locale);
  return useMemo(() => {
    const translateBound = (key: MessageKey, params?: Params) => translate(locale, key, params);
    return Object.assign(translateBound, { locale });
  }, [locale]);
}

export type Translator = ReturnType<typeof useT>;
export type { MessageKey };
