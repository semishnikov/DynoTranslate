import { useEffect, useState, type ReactNode } from "react";
import "./App.css";
import { Home } from "./screens/Home";
import { Apps } from "./screens/Apps";
import { Packs } from "./screens/Packs";
import { Settings } from "./screens/Settings";
import { useStore } from "./state/store";

type Route = "home" | "apps" | "packs" | "settings";

const nav: { id: Route; label: string; icon: ReactNode }[] = [
  {
    id: "home",
    label: "Overview",
    icon: (
      <path d="M3 9.5 10 4l7 5.5V16a1 1 0 0 1-1 1h-4v-4H8v4H4a1 1 0 0 1-1-1z" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" />
    ),
  },
  {
    id: "apps",
    label: "Apps",
    icon: (
      <g fill="none" stroke="currentColor" strokeWidth="1.5">
        <rect x="3.25" y="3.25" width="5.5" height="5.5" rx="1.5" />
        <rect x="11.25" y="3.25" width="5.5" height="5.5" rx="1.5" />
        <rect x="3.25" y="11.25" width="5.5" height="5.5" rx="1.5" />
        <rect x="11.25" y="11.25" width="5.5" height="5.5" rx="1.5" />
      </g>
    ),
  },
  {
    id: "packs",
    label: "Languages",
    icon: (
      <g fill="none" stroke="currentColor" strokeWidth="1.5">
        <circle cx="10" cy="10" r="6.75" />
        <path d="M3.4 10h13.2M10 3.3c1.8 2 2.7 4.3 2.7 6.7s-.9 4.7-2.7 6.7c-1.8-2-2.7-4.3-2.7-6.7s.9-4.7 2.7-6.7z" />
      </g>
    ),
  },
  {
    id: "settings",
    label: "Settings",
    icon: (
      <g fill="none" stroke="currentColor" strokeWidth="1.5">
        <circle cx="10" cy="10" r="2.6" />
        <path d="M10 2.8v1.7M10 15.5v1.7M17.2 10h-1.7M4.5 10H2.8M15.1 4.9l-1.2 1.2M6.1 13.9l-1.2 1.2M15.1 15.1l-1.2-1.2M6.1 6.1 4.9 4.9" strokeLinecap="round" />
      </g>
    ),
  },
];

export default function App() {
  const [route, setRoute] = useState<Route>("home");
  const theme = useStore((state) => state.theme);
  const uiScale = useStore((state) => state.uiScale);
  const toast = useStore((state) => state.toast);
  const notify = useStore((state) => state.notify);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    document.documentElement.style.fontSize = `${(uiScale / 100) * 16}px`;
  }, [uiScale]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => notify(null), 3200);
    return () => window.clearTimeout(timer);
  }, [toast, notify]);

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <svg viewBox="0 0 28 28" width="26" height="26" aria-hidden="true" className="brand__mark">
            <rect x="2" y="2" width="24" height="24" rx="8" fill="var(--accent)" />
            <path d="M9 19.2 13 8.8h2.2l4 10.4h-2.3l-.9-2.5h-4l-.9 2.5z" fill="var(--accent-fg)" />
            <path d="M12.6 14.9h2.6L13.9 11z" fill="var(--accent)" />
          </svg>
          <div>
            <p className="brand__name">Lumen</p>
            <p className="brand__sub">Screen localization</p>
          </div>
        </div>
        <nav className="nav">
          {nav.map((item) => (
            <button
              key={item.id}
              type="button"
              className={route === item.id ? "nav__item nav__item--active" : "nav__item"}
              aria-current={route === item.id ? "page" : undefined}
              onClick={() => setRoute(item.id)}
            >
              <svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">
                {item.icon}
              </svg>
              {item.label}
            </button>
          ))}
        </nav>
        <footer className="sidebar__foot">
          <p>Version 1.0.0</p>
          <p>Runs offline · no account</p>
        </footer>
      </aside>

      <main className="content" key={route}>
        {route === "home" ? <Home /> : null}
        {route === "apps" ? <Apps /> : null}
        {route === "packs" ? <Packs /> : null}
        {route === "settings" ? <Settings /> : null}
      </main>

      <div className="toast-region" aria-live="polite">
        {toast ? <div className="toast">{toast}</div> : null}
      </div>
    </div>
  );
}
