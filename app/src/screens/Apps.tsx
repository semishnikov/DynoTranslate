import { useState } from "react";
import { Card, LanguageCombobox, Segmented, Toggle } from "../components/controls";
import { languages } from "../data/catalog";
import type { OverlayStyle } from "../data/catalog";
import { useT } from "../i18n";
import { useStore } from "../state/store";
import "./apps.css";

export function Apps() {
  const t = useT();
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
        <h1>{t("apps.title")}</h1>
        <p>{t("apps.blurb")}</p>
      </header>

      <div className="apps__toolbar">
        <input
          className="apps__search"
          value={query}
          placeholder={t("apps.find")}
          aria-label={t("apps.find")}
          onChange={(event) => setQuery(event.target.value)}
        />
        <button
          type="button"
          className="ghost-button"
          onClick={() => notify(t("toast.profilesExported"))}
        >
          {t("apps.export")}
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
                  label={t("apps.translate", { name: profile.name })}
                  checked={profile.enabled}
                  onChange={() => toggleProfile(profile.id)}
                />
              </div>

              {open ? (
                <div className="app-row__details">
                  <LanguageCombobox
                    label={t("apps.source")}
                    value={profile.source}
                    options={languages}
                    onChange={(value) => updateProfile(profile.id, { source: value })}
                  />
                  <LanguageCombobox
                    label={t("apps.target")}
                    value={profile.target}
                    options={languages}
                    onChange={(value) => updateProfile(profile.id, { target: value })}
                  />
                  <div className="app-row__style">
                    <span className="quick__label">{t("home.style")}</span>
                    <Segmented<OverlayStyle>
                      label={t("home.style")}
                      value={profile.style}
                      options={[
                        { value: "seamless", label: t("style.seamless") },
                        { value: "plate", label: t("style.plate") },
                        { value: "subtitles", label: t("style.subtitles") },
                      ]}
                      onChange={(value) => updateProfile(profile.id, { style: value })}
                    />
                  </div>
                  {profile.antiCheat ? (
                    <p className="app-row__notice">
                      {t("apps.antiCheat", { name: profile.antiCheat })}
                    </p>
                  ) : null}
                </div>
              ) : null}
            </Card>
          );
        })}
        {visible.length === 0 ? (
          <Card>
            <p className="empty">{t("apps.empty", { query })}</p>
          </Card>
        ) : null}
      </div>
    </div>
  );
}
