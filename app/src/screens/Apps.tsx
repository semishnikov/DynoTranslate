import { useState } from "react";
import { Card, LanguageCombobox, Segmented, Toggle } from "../components/controls";
import { languages } from "../data/catalog";
import type { OverlayStyle } from "../data/catalog";
import { useStore } from "../state/store";
import "./apps.css";

export function Apps() {
  const profiles = useStore((state) => state.profiles);
  const toggleProfile = useStore((state) => state.toggleProfile);
  const updateProfile = useStore((state) => state.updateProfile);
  const notify = useStore((state) => state.notify);
  const [expanded, setExpanded] = useState<string | null>(profiles[0]?.id ?? null);
  const [query, setQuery] = useState("");

  const visible = profiles.filter((profile) =>
    `${profile.name} ${profile.process}`.toLowerCase().includes(query.trim().toLowerCase()),
  );

  return (
    <div className="page">
      <header className="page__head">
        <h1>Apps</h1>
        <p>Profiles appear on their own the first time Lumen sees an app. Change one only if you want something different from the default.</p>
      </header>

      <div className="apps__toolbar">
        <input
          className="apps__search"
          value={query}
          placeholder="Find an app"
          aria-label="Find an app"
          onChange={(event) => setQuery(event.target.value)}
        />
        <button type="button" className="ghost-button" onClick={() => notify("Profiles exported to Documents/Lumen.")}>
          Export profiles
        </button>
      </div>

      <div className="apps__list">
        {visible.map((profile) => {
          const open = expanded === profile.id;
          return (
            <Card key={profile.id}>
              <div className="app-row">
                <span className="current__icon" aria-hidden="true">
                  {profile.name.slice(0, 1)}
                </span>
                <button
                  type="button"
                  className="app-row__main"
                  aria-expanded={open}
                  onClick={() => setExpanded(open ? null : profile.id)}
                >
                  <span className="app-row__name">{profile.name}</span>
                  <span className="app-row__meta">
                    {profile.process} · {profile.lastSeen}
                  </span>
                </button>
                <Toggle
                  label={`Translate ${profile.name}`}
                  checked={profile.enabled}
                  onChange={() => toggleProfile(profile.id)}
                />
              </div>

              {open ? (
                <div className="app-row__details">
                  <LanguageCombobox
                    label="Source language"
                    value={profile.source}
                    options={languages}
                    onChange={(value) => updateProfile(profile.id, { source: value })}
                  />
                  <LanguageCombobox
                    label="Target language"
                    value={profile.target}
                    options={languages}
                    onChange={(value) => updateProfile(profile.id, { target: value })}
                  />
                  <div className="app-row__style">
                    <span className="quick__label">Overlay style</span>
                    <Segmented<OverlayStyle>
                      label="Overlay style"
                      value={profile.style}
                      options={[
                        { value: "seamless", label: "Seamless" },
                        { value: "plate", label: "Plate" },
                        { value: "subtitles", label: "Subtitles" },
                      ]}
                      onChange={(value) => updateProfile(profile.id, { style: value })}
                    />
                  </div>
                  {profile.antiCheat ? (
                    <p className="app-row__notice">
                      {profile.antiCheat} protects this game. Lumen only reads the picture the system already shows, so
                      nothing is injected into the game process.
                    </p>
                  ) : null}
                </div>
              ) : null}
            </Card>
          );
        })}
        {visible.length === 0 ? (
          <Card>
            <p className="empty">No profile matches “{query}”. Start an app and it will show up here.</p>
          </Card>
        ) : null}
      </div>
    </div>
  );
}
