import { useState } from "react";
import { LanguageCombobox, Toggle } from "../components/controls";
import { languages } from "../data/catalog";
import { localeOptions, useI18nStore, useT, type LocaleCode } from "../i18n";
import { DEFAULT_REGION, useStore } from "../state/store";
import "./onboarding.css";

type Step = "welcome" | "language" | "pack" | "region" | "done";

const ORDER: Step[] = ["welcome", "language", "pack", "region", "done"];

export function Onboarding({ onFinished }: { onFinished: () => void }) {
  const t = useT();
  const setLocale = useI18nStore((state) => state.setLocale);
  const interfaceLanguage = useStore((state) => state.interfaceLanguage);
  const set = useStore((state) => state.set);
  const targetLanguage = useStore((state) => state.targetLanguage);
  const packs = useStore((state) => state.packs);
  const installPack = useStore((state) => state.installPack);
  const region = useStore((state) => state.region);
  const setDraftRegion = useStore((state) => state.setDraftRegion);
  const saveRegion = useStore((state) => state.saveRegion);
  const completeOnboarding = useStore((state) => state.completeOnboarding);
  const skipOnboarding = useStore((state) => state.skipOnboarding);

  const [step, setStep] = useState<Step>("welcome");
  const index = ORDER.indexOf(step);
  const total = ORDER.length;
  const primaryPack = packs.find((pack) => pack.id === "ja-ru") ?? packs[0];
  const packReady = primaryPack?.state === "installed";
  const customRegion = region.w < 100 || region.h < 100;

  const back = () => setStep(ORDER[Math.max(0, index - 1)] as Step);
  const next = () => {
    if (step === "done") {
      completeOnboarding();
      onFinished();
      return;
    }
    setStep(ORDER[Math.min(ORDER.length - 1, index + 1)] as Step);
  };
  const skip = () => {
    skipOnboarding();
    onFinished();
  };

  const switchUiLocale = (value: string) => {
    const locale = value as LocaleCode;
    set("interfaceLanguage", locale);
    setLocale(locale);
  };

  return (
    <div className="onboarding" role="dialog" aria-modal="true" aria-labelledby="onboarding-title">
      <div className="onboarding__card">
        <header className="onboarding__head">
          <p className="onboarding__step">{t("onboarding.step", { current: index + 1, total })}</p>
          <button type="button" className="ghost-button onboarding__skip" onClick={skip}>
            {t("onboarding.skip")}
          </button>
        </header>

        <div className="onboarding__body">
          {step === "welcome" ? (
            <>
              <h2 id="onboarding-title">{t("onboarding.welcome.title")}</h2>
              <p>{t("onboarding.welcome.body")}</p>
              <ul className="onboarding__points">
                <li>{t("onboarding.welcome.point1")}</li>
                <li>{t("onboarding.welcome.point2")}</li>
                <li>{t("onboarding.welcome.point3")}</li>
              </ul>
            </>
          ) : null}

          {step === "language" ? (
            <>
              <h2 id="onboarding-title">{t("onboarding.language.title")}</h2>
              <p>{t("onboarding.language.body")}</p>
              <div className="onboarding__fields">
                <LanguageCombobox
                  label={t("settings.interfaceLang")}
                  value={interfaceLanguage}
                  options={localeOptions}
                  onChange={switchUiLocale}
                />
                <LanguageCombobox
                  label={t("home.target")}
                  value={targetLanguage}
                  options={languages}
                  onChange={(value) => set("targetLanguage", value)}
                />
              </div>
            </>
          ) : null}

          {step === "pack" && primaryPack ? (
            <>
              <h2 id="onboarding-title">{t("onboarding.pack.title")}</h2>
              <p>{t("onboarding.pack.body")}</p>
              <div className="onboarding__pack">
                <p className="onboarding__pack-pair">{primaryPack.pair}</p>
                <p className="onboarding__pack-meta">{primaryPack.size}</p>
                {packReady ? (
                  <p className="onboarding__pack-ok">{t("onboarding.pack.installed", { pair: primaryPack.pair })}</p>
                ) : (
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => installPack(primaryPack.id)}
                  >
                    {t("onboarding.pack.install", { pair: primaryPack.pair })}
                  </button>
                )}
              </div>
            </>
          ) : null}

          {step === "region" ? (
            <>
              <h2 id="onboarding-title">{t("onboarding.region.title")}</h2>
              <p>{t("onboarding.region.body")}</p>
              <div className="onboarding__fields">
                <Toggle
                  label={t("onboarding.region.full")}
                  description={t("region.help")}
                  checked={!customRegion}
                  onChange={(full) => {
                    const nextRegion = full ? { ...DEFAULT_REGION } : { x: 10, y: 15, w: 80, h: 60 };
                    setDraftRegion(nextRegion);
                    set("region", nextRegion);
                    saveRegion();
                  }}
                />
              </div>
            </>
          ) : null}

          {step === "done" ? (
            <>
              <h2 id="onboarding-title">{t("onboarding.done.title")}</h2>
              <p>{t("onboarding.done.body")}</p>
            </>
          ) : null}
        </div>

        <footer className="onboarding__foot">
          <button
            type="button"
            className="ghost-button"
            onClick={back}
            disabled={index === 0}
          >
            {t("onboarding.back")}
          </button>
          <button type="button" className="primary-button" onClick={next}>
            {step === "done" ? t("onboarding.finish") : t("onboarding.next")}
          </button>
        </footer>
      </div>
    </div>
  );
}
