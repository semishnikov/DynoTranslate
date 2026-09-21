import { Card, Segmented, Slider, Toggle } from "../components/controls";
import { useStore } from "../state/store";
import type { Theme } from "../state/store";

export function Settings() {
  const state = useStore();

  return (
    <div className="page">
      <header className="page__head">
        <h1>Settings</h1>
        <p>Every change applies immediately. Nothing here needs to be confirmed.</p>
      </header>

      <Card
        title="General"
        description="How Lumen behaves around the rest of your system."
        action={
          <button type="button" className="ghost-button" onClick={state.reset}>
            Reset to recommended
          </button>
        }
      >
        <Toggle
          label="Start with Windows"
          description="Lumen starts minimised in the notification area."
          checked={state.autostart}
          onChange={(value) => state.set("autostart", value)}
        />
        <Toggle
          label="Pause on password fields"
          description="Capture stops for password boxes, the sign-in screen and UAC prompts."
          checked={state.pauseOnPasswordFields}
          onChange={(value) => state.set("pauseOnPasswordFields", value)}
        />
        <Toggle
          label="Work offline only"
          description="Only local language packs are used. No text ever leaves this computer."
          checked={state.offlineOnly}
          onChange={(value) => state.set("offlineOnly", value)}
        />
      </Card>

      <Card title="Appearance" description="How the overlay and the app itself look.">
        <div className="field">
          <div className="field__text">
            <span className="field__label">Theme</span>
            <p className="field__hint">Follows the system by default.</p>
          </div>
          <div style={{ width: 240 }}>
            <Segmented<Theme>
              label="Theme"
              value={state.theme}
              options={[
                { value: "dark", label: "Dark" },
                { value: "light", label: "Light" },
              ]}
              onChange={(value) => state.set("theme", value)}
            />
          </div>
        </div>
        <Slider
          label="Overlay opacity"
          value={state.overlayOpacity}
          min={40}
          max={100}
          unit="%"
          onChange={(value) => state.set("overlayOpacity", value)}
        />
        <Slider
          label="Text scale"
          value={state.textScale}
          min={80}
          max={140}
          unit="%"
          onChange={(value) => state.set("textScale", value)}
        />
        <Slider
          label="Edge softness"
          value={state.edgeSoftness}
          min={0}
          max={100}
          unit="%"
          onChange={(value) => state.set("edgeSoftness", value)}
        />
        <Slider
          label="Animation speed"
          value={state.animationSpeed}
          min={0}
          max={150}
          format={(value) => (value === 0 ? "Off" : `${value}%`)}
          onChange={(value) => state.set("animationSpeed", value)}
        />
        <Slider
          label="Interface scale"
          value={state.uiScale}
          min={100}
          max={200}
          step={25}
          unit="%"
          onChange={(value) => state.set("uiScale", value)}
        />
      </Card>

      <Card title="Performance" description="Lumen slows itself down when the game needs the hardware.">
        <Slider
          label="Resource limit"
          value={state.resourceCap}
          min={20}
          max={100}
          format={(value) => (value <= 40 ? "Light" : value <= 75 ? "Balanced" : "Full speed")}
          onChange={(value) => state.set("resourceCap", value)}
        />
      </Card>

      <Card title="About">
        <div className="field">
          <div className="field__text">
            <span className="field__label">Lumen 1.0.0</span>
            <p className="field__hint">Offline translation engine · language packs 2026.03</p>
          </div>
          <button type="button" className="ghost-button" onClick={() => state.notify("Lumen is up to date.")}>
            Check for updates
          </button>
        </div>
      </Card>
    </div>
  );
}
