import { useState } from "react";
import { getErrorMessage } from "../lib/format";

/**
 * Inline license-key entry, shared by the license gate and the settings
 * License section. The caller owns what happens after a successful activate.
 */
export function LicenseKeyForm({
  onActivate,
  onActivated,
  autoFocus = true
}: {
  onActivate: (licenseKey: string) => Promise<unknown>;
  onActivated?: () => void;
  autoFocus?: boolean;
}): JSX.Element {
  const [licenseKey, setLicenseKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const submit = async () => {
    const trimmed = licenseKey.trim();
    if (!trimmed || busy) {
      return;
    }
    setBusy(true);
    setError(undefined);
    try {
      await onActivate(trimmed);
      setLicenseKey("");
      onActivated?.();
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="license-key-form"
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <input
        type="text"
        className="license-key-input"
        value={licenseKey}
        onChange={(event) => setLicenseKey(event.target.value)}
        placeholder="XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX"
        autoFocus={autoFocus}
        autoComplete="off"
        spellCheck={false}
        disabled={busy}
        aria-label="License key"
      />
      <button
        type="submit"
        className="text-button primary"
        disabled={busy || !licenseKey.trim()}
      >
        <span>{busy ? "Activating..." : "Activate"}</span>
      </button>
      {error ? <p className="settings-note license-key-error">{error}</p> : null}
    </form>
  );
}
