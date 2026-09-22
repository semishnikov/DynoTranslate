import { useRef } from "react";
import { Card } from "../components/controls";
import { useT } from "../i18n";
import { DEFAULT_REGION, useStore, type RegionRect } from "../state/store";
import "./region.css";

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

type Handle = "nw" | "ne" | "sw" | "se" | "n" | "s" | "e" | "w" | "move";

function applyHandle(region: RegionRect, handle: Handle, dx: number, dy: number): RegionRect {
  let { x, y, w, h } = region;
  const min = 8;
  if (handle === "move") {
    x = clamp(x + dx, 0, 100 - w);
    y = clamp(y + dy, 0, 100 - h);
    return { x, y, w, h };
  }
  if (handle.includes("w")) {
    const nx = clamp(x + dx, 0, x + w - min);
    w += x - nx;
    x = nx;
  }
  if (handle.includes("n")) {
    const ny = clamp(y + dy, 0, y + h - min);
    h += y - ny;
    y = ny;
  }
  if (handle.includes("e")) {
    w = clamp(w + dx, min, 100 - x);
  }
  if (handle.includes("s")) {
    h = clamp(h + dy, min, 100 - y);
  }
  return { x, y, w, h };
}

export function Region() {
  const t = useT();
  const region = useStore((state) => state.region);
  const draft = useStore((state) => state.draftRegion);
  const setDraftRegion = useStore((state) => state.setDraftRegion);
  const saveRegion = useStore((state) => state.saveRegion);
  const notify = useStore((state) => state.notify);
  const frameRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ handle: Handle; x: number; y: number } | null>(null);

  const beginDrag = (handle: Handle) => (event: React.PointerEvent) => {
    event.preventDefault();
    (event.target as HTMLElement).setPointerCapture?.(event.pointerId);
    dragRef.current = { handle, x: event.clientX, y: event.clientY };
  };

  const onPointerMove = (event: React.PointerEvent) => {
    const drag = dragRef.current;
    const frame = frameRef.current;
    if (!drag || !frame) return;
    const rect = frame.getBoundingClientRect();
    const dx = ((event.clientX - drag.x) / rect.width) * 100;
    const dy = ((event.clientY - drag.y) / rect.height) * 100;
    drag.x = event.clientX;
    drag.y = event.clientY;
    setDraftRegion(applyHandle(draft, drag.handle, dx, dy));
  };

  const endDrag = () => {
    dragRef.current = null;
  };

  const nudge = (event: React.KeyboardEvent) => {
    const step = event.shiftKey ? 5 : 1;
    const map: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, -step],
      ArrowDown: [0, step],
    };
    const delta = map[event.key];
    if (!delta) return;
    event.preventDefault();
    const handle: Handle = event.altKey ? "se" : "move";
    setDraftRegion(applyHandle(draft, handle, delta[0], delta[1]));
  };

  const fields: { key: keyof RegionRect; label: string }[] = [
    { key: "x", label: t("region.x") },
    { key: "y", label: t("region.y") },
    { key: "w", label: t("region.w") },
    { key: "h", label: t("region.h") },
  ];

  return (
    <div className="page">
      <header className="page__head">
        <h1>{t("region.title")}</h1>
        <p>{t("region.blurb")}</p>
      </header>

      <Card title={t("region.editor")} description={t("region.help")}>
        <div
          ref={frameRef}
          className="region-frame"
          onPointerMove={onPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
        >
          <div className="region-frame__desktop" aria-hidden="true" />
          <div
            className="region-frame__box"
            role="application"
            aria-label={t("region.editor")}
            tabIndex={0}
            style={{
              left: `${draft.x}%`,
              top: `${draft.y}%`,
              width: `${draft.w}%`,
              height: `${draft.h}%`,
            }}
            onKeyDown={nudge}
            onPointerDown={beginDrag("move")}
          >
            {(["nw", "ne", "sw", "se", "n", "s", "e", "w"] as Handle[]).map((handle) => (
              <button
                key={handle}
                type="button"
                className={`region-frame__handle region-frame__handle--${handle}`}
                aria-label={handle}
                tabIndex={-1}
                onPointerDown={beginDrag(handle)}
              />
            ))}
          </div>
        </div>
        <p className="region-frame__hint">{t("region.hint")}</p>

        <div className="region-fields">
          {fields.map((field) => (
            <label key={field.key} className="region-fields__item">
              <span>{field.label}</span>
              <input
                type="number"
                min={field.key === "w" || field.key === "h" ? 8 : 0}
                max={100}
                value={Math.round(draft[field.key])}
                onChange={(event) => {
                  const next = Number(event.target.value);
                  const max = field.key === "x" ? 100 - draft.w : field.key === "y" ? 100 - draft.h : 100;
                  const min = field.key === "w" || field.key === "h" ? 8 : 0;
                  setDraftRegion({ ...draft, [field.key]: clamp(next, min, max) });
                }}
              />
            </label>
          ))}
        </div>

        <div className="region-actions">
          <button
            type="button"
            className="ghost-button"
            onClick={() => setDraftRegion({ ...DEFAULT_REGION })}
          >
            {t("region.fullscreen")}
          </button>
          <button
            type="button"
            className="ghost-button"
            onClick={() => setDraftRegion({ x: Math.round((100 - draft.w) / 2), y: Math.round((100 - draft.h) / 2), w: draft.w, h: draft.h })}
          >
            {t("region.center")}
          </button>
          <button
            type="button"
            className="ghost-button"
            onClick={() => setDraftRegion({ ...region })}
          >
            {t("region.reset")}
          </button>
          <button
            type="button"
            className="primary-button"
            onClick={() => {
              saveRegion();
              notify(t("toast.regionSaved"));
            }}
          >
            {t("region.save")}
          </button>
        </div>
      </Card>
    </div>
  );
}
