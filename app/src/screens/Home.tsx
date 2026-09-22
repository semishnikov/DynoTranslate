import { useEffect, useState } from "react";
import { Card, LanguageCombobox, Segmented, Toggle } from "../components/controls";
import { languages } from "../data/catalog";
import type { OverlayStyle, Responsiveness } from "../data/catalog";
import { useT } from "../i18n";
import { currentProfile, useStore } from "../state/store";
import "./home.css";

const demoLines = [
  { original: "ゲームを続ける", translated: "Продолжить игру", top: 18 },
  { original: "新しい冒険", translated: "Новое приключение", top: 38 },
  { original: "設定", translated: "Настройки", top: 58 },
  { original: "終了", translated: "Выход", top: 78 },
];

export function Home() {
  const t = useT();
  const running = useStore((state) => state.running);
  const toggleRunning = useStore((state) => state.toggleRunning);
  const profiles = useStore((state) => state.profiles);
  const toggleProfile = useStore((state) => state.toggleProfile);
  const targetLanguage = useStore((state) => state.targetLanguage);
  const overlayStyle = useStore((state) => state.overlayStyle);
  const responsiveness = useStore((state) => state.responsiveness);
  const set = useStore((state) => state.set);
  const active = currentProfile(profiles);
  const [peeking, setPeeking] = useState(false);

  useEffect(() => {
    const down = (event: KeyboardEvent) => {
      if (event.key === "Alt" || event.altKey) setPeeking(true);
    };
    const up = () => setPeeking(false);
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    window.addEventListener("blur", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", up);
    };
  }, []);

  const sourceLabel = languages.find((item) => item.value === active.source)?.label ?? active.source;
  const targetLabel = languages.find((item) => item.value === targetLanguage)?.label ?? targetLanguage;
  const status = !running
    ? t("home.status.paused")
    : active.enabled
      ? t("home.status.translating", { app: active.name, from: sourceLabel, to: targetLabel })
      : t("home.status.waiting");

  return (
    <div className="page">
      <header className="page__head">
        <h1>{t("home.title")}</h1>
        <p>{t("home.blurb")}</p>
      </header>

      <section className={running ? "status status--on" : "status"} aria-label={t("home.power")}>
        <button
          type="button"
          className="power"
          role="switch"
          aria-checked={running}
          aria-label={t("home.power")}
          onClick={toggleRunning}
        >
          <span className="power__ring" />
          <span className="power__core">
            <svg viewBox="0 0 24 24" width="26" height="26" aria-hidden="true">
              <path
                d="M12 4.5v7"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
              />
              <path
                d="M7.4 7.6a6.5 6.5 0 1 0 9.2 0"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
              />
            </svg>
          </span>
        </button>
        <div className="status__text">
          <p className="status__title">{running ? t("home.on") : t("home.off")}</p>
          <p className="status__detail">{status}</p>
          <p className="status__hint">{t("home.hint")}</p>
        </div>
        <dl className="status__metrics">
          <div>
            <dt>{t("home.metric.response")}</dt>
            <dd>{running ? "186 ms" : "—"}</dd>
          </div>
          <div>
            <dt>{t("home.metric.cache")}</dt>
            <dd>{running ? "94%" : "—"}</dd>
          </div>
          <div>
            <dt>{t("home.metric.cpu")}</dt>
            <dd>{running ? "5.2%" : "0.1%"}</dd>
          </div>
        </dl>
      </section>

      <Card
        title={t("home.current.title")}
        description={t("home.current.description")}
        action={
          <button type="button" className="ghost-button" onClick={() => toggleProfile(active.id)}>
            {active.enabled ? t("home.current.exclude") : t("home.current.include")}
          </button>
        }
      >
        <div className="current">
          <span className="current__icon" aria-hidden="true">
            {active.name.slice(0, 1)}
          </span>
          <div>
            <p className="current__name">{active.name}</p>
            <p className="current__meta">
              {active.process} · {sourceLabel} → {targetLabel}
              {active.antiCheat ? ` · ${t("home.current.antiCheat", { name: active.antiCheat })}` : ""}
            </p>
          </div>
          <Toggle
            label={t("home.current.translate")}
            checked={active.enabled}
            onChange={() => toggleProfile(active.id)}
          />
        </div>
      </Card>

      <div className="quick">
        <Card>
          <LanguageCombobox
            label={t("home.target")}
            value={targetLanguage}
            options={languages}
            onChange={(value) => set("targetLanguage", value)}
          />
        </Card>
        <Card>
          <span className="quick__label">{t("home.speed")}</span>
          <Segmented<Responsiveness>
            label={t("home.speed")}
            value={responsiveness}
            options={[
              { value: "fast", label: t("speed.fast") },
              { value: "balanced", label: t("speed.balanced") },
              { value: "accurate", label: t("speed.accurate") },
            ]}
            onChange={(value) => set("responsiveness", value)}
          />
        </Card>
        <Card>
          <span className="quick__label">{t("home.style")}</span>
          <Segmented<OverlayStyle>
            label={t("home.style")}
            value={overlayStyle}
            options={[
              { value: "seamless", label: t("style.seamless") },
              { value: "plate", label: t("style.plate") },
              { value: "subtitles", label: t("style.subtitles") },
            ]}
            onChange={(value) => set("overlayStyle", value)}
          />
        </Card>
      </div>

      <Card
        title={t("home.preview.title")}
        description={t("home.preview.description")}
        action={
          <button
            type="button"
            className="ghost-button"
            onMouseDown={() => setPeeking(true)}
            onMouseUp={() => setPeeking(false)}
            onMouseLeave={() => setPeeking(false)}
          >
            {t("home.preview.hold")}
          </button>
        }
      >
        <div className={`preview preview--${overlayStyle}`}>
          {demoLines.map((line) => (
            <div key={line.original} className="preview__line" style={{ top: `${line.top}%` }}>
              <span className="preview__original">{line.original}</span>
              <span
                className="preview__translated"
                style={{ opacity: running && !peeking ? 1 : 0 }}
              >
                {line.translated}
              </span>
            </div>
          ))}
          <span className="preview__badge">
            {peeking ? t("home.preview.original") : running ? t("home.preview.translated") : t("home.preview.off")}
          </span>
        </div>
      </Card>
    </div>
  );
}
