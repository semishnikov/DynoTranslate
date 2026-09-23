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
  paused: boolean;
  translated: number;
};

const STARTING: LiveStatus = {
  phase: "starting",
  title: "Запускаю перевод",
  detail: "Секунду. Сейчас будет видно, что программа делает.",
  watched: "",
  capture: "",
  sample: "",
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
    void pull();
    void invoke<string>("live_preview")
      .then((image) => {
        if (!stop && image) {
          setPreview(image);
        }
      })
      .catch(() => undefined);
    const timer = window.setInterval(() => void pull(), 1000);
    const unlistenStatus = listen<LiveStatus>("live-status", (event) => apply(event.payload));
    const unlistenPreview = listen<{ image: string }>("live-preview", (event) => {
      if (!stop) {
        setPreview(event.payload.image);
      }
    });
    return () => {
      stop = true;
      window.clearInterval(timer);
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

  const sample = sampleParts(status.sample);
  const canPause =
    status.paused || ["waiting", "watching", "translating", "paused"].includes(status.phase);
  const shown = broken
    ? {
        ...status,
        phase: "error",
        title: "Окно не видит перевод",
        detail: "Закройте программу и откройте её снова.",
      }
    : status;

  return (
    <main className="stage" data-phase={shown.phase}>
      <header className="brand-row">
        <p className="brand">
          DynoTranslate <span>перевод на экране</span>
        </p>
        {shown.translated > 0 ? (
          <p className="count">Переведено строк: {shown.translated}</p>
        ) : null}
      </header>

      <section className="preview" aria-label="Окно, которое программа смотрит">
        {preview ? (
          <img src={preview} alt="Картинка окна, которое программа сейчас смотрит" />
        ) : (
          <p className="preview__empty">
            Здесь появится картинка окна. Способ захвата программа выбирает сама.
          </p>
        )}
        <p className="preview__caption">
          {shown.watched
            ? `Смотрю: ${shown.watched}${shown.capture ? ` · ${shown.capture}` : ""}`
            : "Пока ни одно окно. Щёлкните по Блокноту или игре."}
        </p>
      </section>

      <section className="status" aria-live="polite">
        <h1>
          <span className="dot" aria-hidden="true" />
          {shown.title}
        </h1>
        <p>{shown.detail}</p>
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
      </section>

      <section className="steps">
        <h2>Как проверить</h2>
        <ol>
          <li>Оставьте это окно открытым, чтобы видеть, что происходит.</li>
          <li>Откройте Блокнот, напишите английскую фразу крупными буквами и щёлкните по нему.</li>
          <li>Русский должен появиться поверх текста. В игре то же самое: обычное окно, не весь экран.</li>
        </ol>
        <p className="limit">
          Сейчас перевожу английский на русский. Язык выбирать не нужно. Если текст на другом языке,
          здесь будет написано, что его пропускаю.
        </p>
      </section>
    </main>
  );
}
