import { Card } from "../components/controls";
import { useStore } from "../state/store";
import "./packs.css";

const stateLabel = {
  installed: "Installed",
  update: "Update available",
  available: "Not installed",
  downloading: "Downloading",
} as const;

export function Packs() {
  const packs = useStore((state) => state.packs);
  const installPack = useStore((state) => state.installPack);
  const removePack = useStore((state) => state.removePack);
  const installed = packs.filter((pack) => pack.state !== "available").length;

  return (
    <div className="page">
      <header className="page__head">
        <h1>Languages</h1>
        <p>
          Packs run on this computer, so translation keeps working without a connection. {installed} of {packs.length}{" "}
          packs are on disk, using 1.06 GB.
        </p>
      </header>

      <div className="packs">
        {packs.map((pack) => (
          <Card key={pack.id}>
            <div className="pack">
              <div>
                <p className="pack__pair">{pack.pair}</p>
                <p className="pack__meta">
                  {pack.size} · {stateLabel[pack.state]}
                </p>
              </div>
              {pack.state === "downloading" ? (
                <div className="pack__progress" role="progressbar" aria-valuenow={pack.progress ?? 0} aria-valuemin={0} aria-valuemax={100}>
                  <span style={{ width: `${pack.progress ?? 0}%` }} />
                </div>
              ) : null}
              <div className="pack__actions">
                {pack.state === "available" ? (
                  <button type="button" className="primary-button" onClick={() => installPack(pack.id)}>
                    Install
                  </button>
                ) : null}
                {pack.state === "update" ? (
                  <button type="button" className="primary-button" onClick={() => installPack(pack.id)}>
                    Update
                  </button>
                ) : null}
                {pack.state !== "available" ? (
                  <button type="button" className="ghost-button" onClick={() => removePack(pack.id)}>
                    Remove
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
