import { RefreshCcw } from "lucide-react";
import type { ScanProgress } from "../../shared/types";
import { formatScanProgress, getScanProgressPercent } from "../lib/format";

export function LoadingOverlay({ label, progress }: { label: string; progress?: ScanProgress }): JSX.Element {
  const percent = getScanProgressPercent(progress);

  return (
    <div className="loading-overlay">
      <RefreshCcw size={22} />
      <div className="loading-copy">
        <span>{label}</span>
        {progress ? <small>{formatScanProgress(progress, percent)}</small> : null}
        <div
          className={`scan-progress-track${percent === undefined ? " indeterminate" : ""}`}
          role="progressbar"
          aria-label={label}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
        >
          <i style={percent === undefined ? undefined : { width: `${percent}%` }} />
        </div>
      </div>
    </div>
  );
}
