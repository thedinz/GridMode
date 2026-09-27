import { CheckCircle2, Download } from "lucide-react";
import type { UpdateStatus } from "../../shared/types";
import { gridModeApi } from "../gridModeApi";

export function UpdateBanner({ status }: { status: UpdateStatus }): JSX.Element | null {
  if (status.state === "idle" || status.state === "not-available") {
    return null;
  }

  const download = () => {
    if (status.manualDownload && status.downloadUrl) {
      void gridModeApi.updates.openDownload(status.downloadUrl);
      return;
    }
    void gridModeApi.updates.download();
  };

  return (
    <aside className={`update-banner ${status.state}`}>
      <div>
        <strong>{status.version ? `Update ${status.version}` : "GridMode Update"}</strong>
        <span>{status.message}</span>
      </div>
      {status.state === "available" ? (
        <button
          className="text-button"
          onClick={download}
        >
          <Download size={16} />
          <span>{status.manualDownload ? "Open download" : "Download"}</span>
        </button>
      ) : null}
      {status.state === "downloaded" ? (
        <button
          className="text-button"
          onClick={() => void gridModeApi.updates.install()}
        >
          <CheckCircle2 size={16} />
          <span>Install</span>
        </button>
      ) : null}
    </aside>
  );
}
