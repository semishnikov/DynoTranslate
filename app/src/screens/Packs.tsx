import { Card } from "../components/controls";
import { useT } from "../i18n";
import { useStore } from "../state/store";
import type { MessageKey } from "../i18n";
import "./packs.css";

const stateKey = {
  installed: "packs.state.installed",
  update: "packs.state.update",
  available: "packs.state.available",
  downloading: "packs.state.downloading",
} as const satisfies Record<string, MessageKey>;

export function Packs() {
  const t = useT();
  const packs = useStore((state) => state.packs);
  const installPack = useStore((state) => state.installPack);
  const removePack = useStore((state) => state.removePack);
  const notify = useStore((state) => state.notify);
  const installed = packs.filter((pack) => pack.state !== "available").length;

  return (
    <div className="page">
      <header className="page__head">
        <h1>{t("packs.title")}</h1>
        <p>
          {t("packs.blurb", { installed, total: packs.length, size: "1.06 GB" })}
        </p>
      </header>

      <div className="packs">
        {packs.map((pack) => (
          <Card key={pack.id}>
            <div className="pack">
              <div>
                <p className="pack__pair">{pack.pair}</p>
                <p className="pack__meta">
                  {pack.size} · {t(stateKey[pack.state])}
                </p>
              </div>
              {pack.state === "downloading" ? (
                <div
                  className="pack__progress"
                  role="progressbar"
                  aria-valuenow={pack.progress ?? 0}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-label={pack.pair}
                >
                  <span style={{ width: `${pack.progress ?? 0}%` }} />
                </div>
              ) : null}
              <div className="pack__actions">
                {pack.state === "available" ? (
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => {
                      installPack(pack.id);
                      notify(t("toast.packInstalled"));
                    }}
                  >
                    {t("packs.install")}
                  </button>
                ) : null}
                {pack.state === "update" ? (
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => {
                      installPack(pack.id);
                      notify(t("toast.packInstalled"));
                    }}
                  >
                    {t("packs.update")}
                  </button>
                ) : null}
                {pack.state !== "available" ? (
                  <button
                    type="button"
                    className="ghost-button"
                    onClick={() => {
                      removePack(pack.id);
                      notify(t("toast.packRemoved"));
                    }}
                  >
                    {t("packs.remove")}
                  </button>
                ) : null}
              </div>
            </div>
          </Card>
        ))}
      </div>
    </div>
  );
}
