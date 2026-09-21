import { useEffect, useState } from "react";
import { Card, LanguageCombobox, Segmented, Toggle } from "../components/controls";
import { languages } from "../data/catalog";
import type { OverlayStyle, Responsiveness } from "../data/catalog";
import { currentProfile, useStore } from "../state/store";
import "./home.css";

const demoLines = [
  { original: "ゲームを続ける", translated: "Продолжить игру", top: 18 },
  { original: "新しい冒険", translated: "Новое приключение", top: 38 },
  { original: "設定", translated: "Настройки", top: 58 },
  { original: "終了", translated: "Выход", top: 78 },
];

export function Home() {
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
    ? "Paused. Nothing is captured."
    : active.enabled
      ? `Translating ${active.name} · ${sourceLabel} → ${targetLabel}`
      : "Waiting for a window to translate";

  return (
    <div className="page">
      <header className="page__head">
        <h1>Overview</h1>
        <p>
          Lumen watches the window you are using, reads the text it shows and draws the translation in place. It never
          reads game memory and never injects code.
        </p>
      </header>

      <section className={running ? "status status--on" : "status"}>
        <button
          type="button"
          className="power"
          role="switch"
          aria-checked={running}
          aria-label="Translation"
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
          <p className="status__title">{running ? "Translation is on" : "Translation is off"}</p>
          <p className="status__detail">{status}</p>
          <p className="status__hint">Hold Alt+Q to see the original text. Alt+T turns the overlay on and off.</p>
        </div>
        <dl className="status__metrics">
          <div>
            <dt>Response</dt>
            <dd>{running ? "186 ms" : "—"}</dd>
          </div>
          <div>
            <dt>Cache hits</dt>
            <dd>{running ? "94%" : "—"}</dd>
          </div>
          <div>
            <dt>CPU</dt>
            <dd>{running ? "5.2%" : "0.1%"}</dd>
          </div>
        </dl>
      </section>

      <Card
        title="Current window"
        description="Each app keeps its own language pair and overlay style."
        action={
          <button type="button" className="ghost-button" onClick={() => toggleProfile(active.id)}>
            {active.enabled ? "Exclude this app" : "Include this app"}
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
              {active.antiCheat ? ` · ${active.antiCheat} detected` : ""}
            </p>
          </div>
          <Toggle
            label="Translate this app"
            checked={active.enabled}
            onChange={() => toggleProfile(active.id)}
          />
        </div>
      </Card>

      <div className="quick">
        <Card>
          <LanguageCombobox
            label="Translate into"
            value={targetLanguage}
            options={languages}
            onChange={(value) => set("targetLanguage", value)}
          />
        </Card>
        <Card>
          <span className="quick__label">Speed</span>
          <Segmented<Responsiveness>
            label="Speed"
            value={responsiveness}
            options={[
              { value: "fast", label: "Fast" },
              { value: "balanced", label: "Balanced" },
              { value: "accurate", label: "Accurate" },
            ]}
            onChange={(value) => set("responsiveness", value)}
          />
        </Card>
        <Card>
          <span className="quick__label">Overlay style</span>
          <Segmented<OverlayStyle>
            label="Overlay style"
            value={overlayStyle}
            options={[
              { value: "seamless", label: "Seamless" },
              { value: "plate", label: "Plate" },
              { value: "subtitles", label: "Subtitles" },
            ]}
            onChange={(value) => set("overlayStyle", value)}
          />
        </Card>
      </div>

      <Card
        title="Live preview"
        description="A sample menu in Japanese, rendered the way the overlay draws it."
        action={
          <button
            type="button"
            className="ghost-button"
            onMouseDown={() => setPeeking(true)}
            onMouseUp={() => setPeeking(false)}
            onMouseLeave={() => setPeeking(false)}
          >
            Hold to see original
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
          <span className="preview__badge">{peeking ? "Original" : running ? "Translated" : "Overlay off"}</span>
        </div>
      </Card>
    </div>
  );
}
