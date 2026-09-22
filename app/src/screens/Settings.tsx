import { Card, Segmented, Slider, Toggle } from "../components/controls";
import { localeOptions, useT } from "../i18n";
import { useStore } from "../state/store";
import type { Theme } from "../state/store";
import type { LocaleCode } from "../i18n";

export function Settings() {
  const t = useT();
  const state = useStore();

  return (
    <div className="page">
      <header className="page__head">
        <h1>{t("settings.title")}</h1>
        <p>{t("settings.blurb")}</p>
      </header>

      <Card
        title={t("settings.general")}
        description={t("settings.general.description")}
        action={
          <button
            type="button"
            className="ghost-button"
            onClick={() => {
              state.reset();
              state.notify(t("toast.settingsReset"));
            }}
          >
            {t("settings.reset")}
          </button>
        }
      >
        <div className="field">
          <div className="field__text">
            <span className="field__label">{t("settings.interfaceLang")}</span>
            <p className="field__hint">{t("settings.interfaceLang.hint")}</p>
          </div>
          <div style={{ width: 240 }}>
            <Segmented<LocaleCode>
              label={t("settings.interfaceLang")}
              value={state.interfaceLanguage}
              options={localeOptions.map((option) => ({
                value: option.value,
                label: option.native,
              }))}
              onChange={(value) => state.set("interfaceLanguage", value)}
            />
          </div>
        </div>
        <Toggle
          label={t("settings.autostart")}
          description={t("settings.autostart.hint")}
          checked={state.autostart}
          onChange={(value) => state.set("autostart", value)}
        />
        <Toggle
          label={t("settings.pausePasswords")}
          description={t("settings.pausePasswords.hint")}
          checked={state.pauseOnPasswordFields}
          onChange={(value) => state.set("pauseOnPasswordFields", value)}
        />
        <Toggle
          label={t("settings.offlineOnly")}
          description={t("settings.offlineOnly.hint")}
          checked={state.offlineOnly}
          onChange={(value) => state.set("offlineOnly", value)}
        />
      </Card>

      <Card title={t("settings.tray")} description={t("settings.tray.description")}>
        <Toggle
          label={t("settings.tray.minimise")}
          description={t("settings.tray.minimise.hint")}
          checked={state.trayMinimise}
          onChange={(value) => state.set("trayMinimise", value)}
        />
        <Toggle
          label={t("settings.tray.notify")}
          description={t("settings.tray.notify.hint")}
          checked={state.trayNotify}
          onChange={(value) => state.set("trayNotify", value)}
        />
        <Toggle
          label={t("settings.tray.quickToggle")}
          description={t("settings.tray.quickToggle.hint")}
          checked={state.trayQuickToggle}
          onChange={(value) => state.set("trayQuickToggle", value)}
        />
      </Card>

      <Card title={t("settings.hotkeys")} description={t("settings.hotkeys.description")}>
        <Toggle
          label={t("settings.hotkeys.enabled")}
          description={t("settings.hotkeys.enabled.hint")}
          checked={state.hotkeysEnabled}
          onChange={(value) => state.set("hotkeysEnabled", value)}
        />
        <p className="field__hint">{t("settings.hotkeys.list")}</p>
      </Card>

      <Card title={t("settings.appearance")} description={t("settings.appearance.description")}>
        <div className="field">
          <div className="field__text">
            <span className="field__label">{t("settings.theme")}</span>
            <p className="field__hint">{t("settings.theme.hint")}</p>
          </div>
          <div style={{ width: 240 }}>
            <Segmented<Theme>
              label={t("settings.theme")}
              value={state.theme}
              options={[
                { value: "dark", label: t("settings.theme.dark") },
                { value: "light", label: t("settings.theme.light") },
              ]}
              onChange={(value) => state.set("theme", value)}
            />
          </div>
        </div>
        <Slider
          label={t("settings.overlayOpacity")}
          value={state.overlayOpacity}
          min={40}
          max={100}
          unit="%"
          onChange={(value) => state.set("overlayOpacity", value)}
        />
        <Slider
          label={t("settings.textScale")}
          value={state.textScale}
          min={80}
          max={140}
          unit="%"
          onChange={(value) => state.set("textScale", value)}
        />
        <Slider
          label={t("settings.edgeSoftness")}
          value={state.edgeSoftness}
          min={0}
          max={100}
          unit="%"
          onChange={(value) => state.set("edgeSoftness", value)}
        />
        <Slider
          label={t("settings.animationSpeed")}
          value={state.animationSpeed}
          min={0}
          max={150}
          format={(value) => (value === 0 ? t("settings.animationOff") : `${value}%`)}
          onChange={(value) => state.set("animationSpeed", value)}
        />
        <Slider
          label={t("settings.uiScale")}
          value={state.uiScale}
          min={100}
          max={200}
          step={25}
          unit="%"
          onChange={(value) => state.set("uiScale", value)}
        />
      </Card>

      <Card title={t("settings.performance")} description={t("settings.performance.description")}>
        <Slider
          label={t("settings.resourceCap")}
          value={state.resourceCap}
          min={20}
          max={100}
          format={(value) =>
            value <= 40
              ? t("settings.resource.light")
              : value <= 75
                ? t("settings.resource.balanced")
                : t("settings.resource.full")
          }
          onChange={(value) => state.set("resourceCap", value)}
        />
      </Card>

      <Card title={t("settings.about")}>
        <div className="field">
          <div className="field__text">
            <span className="field__label">{t("app.name")} 1.0.0</span>
            <p className="field__hint">{t("settings.about.meta")}</p>
          </div>
          <button
            type="button"
            className="ghost-button"
            onClick={() => state.notify(t("toast.upToDate"))}
          >
            {t("settings.about.check")}
          </button>
        </div>
      </Card>
    </div>
  );
}
