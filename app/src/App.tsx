import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type LiveStatus = {
  phase: string;
  title: string;
  detail: string;
  watched: string;
  capture: string;
  sample: string;
  log_path: string;
  paused: boolean;
  translated: number;
};

type ListedWindow = {
  id: string;
  title: string;
  process: string;
  selected: boolean;
};

const STARTING: LiveStatus = {
  phase: "waiting",
  title: "Выберите окно",
  detail: "Нажмите на него в списке. Выводить вперёд не нужно.",
  watched: "",
  capture: "",
  sample: "",
  log_path: "",
  paused: false,
  translated: 0,
};

function onDesktop(): boolean {
  return typeof window !== "undefined" && ("__TAURI_INTERNALS__" in window || "__TAURI__" in window);
}

function sampleParts(sample: string): { from: string; to: string } | null {
  const split = sample.split(" → ");
  if (split.length < 2 || !split[0] || !split[1]) {
    return null;
  }
  return { from: split[0], to: split.slice(1).join(" → ") };
}

export default function App() {
  const [status, setStatus] = useState<LiveStatus>(STARTING);
  const [preview, setPreview] = useState("");
  const [windows, setWindows] = useState<ListedWindow[]>([]);
  const [broken, setBroken] = useState(false);

  useEffect(() => {
    if (!onDesktop()) {
      return;
    }
    let stop = false;
    let misses = 0;
    const apply = (next: LiveStatus) => {
      if (!stop) {
        setStatus(next);
        setBroken(false);
      }
    };
    const pull = async () => {
      try {
        apply(await invoke<LiveStatus>("live_status"));
        misses = 0;
      } catch {
        misses += 1;
        if (misses >= 3 && !stop) {
          setBroken(true);
        }
      }
    };
    const pullWindows = async () => {
      try {
        const next = await invoke<ListedWindow[]>("list_windows");
        if (!stop) {
          setWindows(next);
        }
      } catch {
        if (!stop) {
          setWindows([]);
        }
      }
    };
    void pull();
    void pullWindows();
    void invoke<string>("live_preview")
      .then((image) => {
        if (!stop && image) {
          setPreview(image);
        }
      })
      .catch(() => undefined);
    const timer = window.setInterval(() => void pull(), 1000);
    const windowTimer = window.setInterval(() => void pullWindows(), 2000);
    const unlistenStatus = listen<LiveStatus>("live-status", (event) => apply(event.payload));
    const unlistenPreview = listen<{ image: string }>("live-preview", (event) => {
      if (!stop) {
        setPreview(event.payload.image);
      }
    });
    return () => {
      stop = true;
      window.clearInterval(timer);
      window.clearInterval(windowTimer);
      void unlistenStatus.then((stopListening) => stopListening());
      void unlistenPreview.then((stopListening) => stopListening());
    };
  }, []);

  async function togglePause() {
    const next = !status.paused;
    setStatus({
      ...status,
      paused: next,
      phase: next ? "paused" : status.phase === "paused" ? "waiting" : status.phase,
      title: next ? "Пауза" : status.title,
      detail: next ? "Перевод спрятан. Нажмите «Продолжить» или Alt+T." : status.detail,
    });
    try {
      await invoke("set_paused", { paused: next });
    } catch {
      setBroken(true);
    }
  }

  async function choose(id: string) {
    setWindows((current) => current.map((item) => ({ ...item, selected: item.id === id })));
    try {
      await invoke("choose_window", { id });
    } catch {
      setBroken(true);
    }
  }

  const sample = sampleParts(status.sample);
  const canPause =
    status.paused || ["waiting", "watching", "translating", "paused"].includes(status.phase);
  const shown = broken
    ? {
        ...status,
        phase: "error",
        title: "Связь с переводом потеряна",
        detail: "Список окон ниже всё равно можно нажать. Если он пуст, закройте программу и откройте снова.",
      }
    : status;
  const autoSelected = windows.length > 0 && windows.every((item) => !item.selected);

  return (
    <main className="stage" data-phase={shown.phase}>
      <header className="brand-row">
        <p className="brand">
          DynoTranslate <span>перевод на экране</span>
        </p>
        {shown.translated > 0 ? <p className="count">Переведено строк: {shown.translated}</p> : null}
      </header>

      <section className="status" aria-live="polite">
        <h1>
          <span className="dot" aria-hidden="true" />
          {shown.title}
        </h1>
        <p>{shown.detail}</p>
      </section>

      <section className="picker" aria-label="Какое окно переводить">
        <h2>Какое окно переводить</h2>
        <ul className="windows">
          <li>
            <button
              type="button"
              className={autoSelected ? "window window--on" : "window"}
              onClick={() => void choose("")}
            >
              <span className="window__title">Само: то, что было впереди</span>
              <span className="window__process">Надёжнее нажать нужное окно ниже</span>
            </button>
          </li>
          {windows.map((item) => (
            <li key={item.id}>
              <button
                type="button"
                className={item.selected ? "window window--on" : "window"}
                onClick={() => void choose(item.id)}
              >
                <span className="window__title">{item.title}</span>
                {item.process ? <span className="window__process">{item.process}</span> : null}
              </button>
            </li>
          ))}
        </ul>
        {windows.length === 0 ? (
          <p className="picker__empty">Открытых окон не вижу. Откройте Блокнот или игру и подождите секунду.</p>
        ) : null}
      </section>

      <section className="preview" aria-label="Окно, которое программа смотрит">
        {preview ? (
          <img src={preview} alt="Картинка окна, которое программа сейчас смотрит" />
        ) : (
          <p className="preview__empty">Здесь появится картинка выбранного окна.</p>
        )}
        <p className="preview__caption">
          {shown.watched
            ? `Смотрю: ${shown.watched}${shown.capture ? ` · ${shown.capture}` : ""}`
            : "Окно ещё не выбрано."}
        </p>
      </section>

      {sample ? (
        <div className="sample">
          <p className="sample__from">{sample.from}</p>
          <p className="sample__to">{sample.to}</p>
        </div>
      ) : null}
      {canPause ? (
        <button type="button" className="pause" onClick={() => void togglePause()}>
          {shown.paused ? "Продолжить" : "Пауза"}
          <span>Alt+T</span>
        </button>
      ) : null}

      <p className="limit">
        Нажмите нужное окно в списке. Его не обязательно выводить вперёд. Сейчас перевожу английский на русский.
        Язык выбирать не нужно. Игру лучше держать обычным окном, не на весь экран.
      </p>
      {shown.log_path ? (
        <p className="limit">
          Журнал этапов: {shown.log_path}
          . Если перевод ошибается, пришлите этот файл — в нём каждый шаг с временем.
        </p>
      ) : null}
    </main>
  );
}
