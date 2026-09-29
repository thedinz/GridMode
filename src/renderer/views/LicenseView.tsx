import { Download, KeyRound, RefreshCcw, X } from "lucide-react";
import { useState } from "react";
import type { LicenseStatus } from "../../shared/types";
import { GridModeLogo } from "../components/GridModeLogo";
import { LicenseKeyForm } from "../components/LicenseKeyForm";

/**
 * Full-screen gate shown instead of the normal app whenever `canUseApp` is
 * false: no trial yet, trial expired, or a validation check is overdue.
 */
export function LicenseView({
  status,
  onStartTrial,
  onActivate,
  onRefresh,
  onOpenCheckout,
  onDismissNotice
}: {
  status: LicenseStatus;
  onStartTrial: () => void;
  onActivate: (licenseKey: string) => Promise<unknown>;
  onRefresh: () => void;
  onOpenCheckout: () => void;
  onDismissNotice: () => void;
}): JSX.Element {
  const [showKeyForm, setShowKeyForm] = useState(false);

  const buyDisabled = !status.checkoutAvailable;

  const heading = (): { eyebrow: string; title: string } => {
    if (status.otherMajorLicense) {
      return { eyebrow: "License", title: `GridMode ${status.appMajor}` };
    }
    switch (status.state) {
      case "trialExpired":
        return { eyebrow: "Trial ended", title: "GridMode" };
      case "validationRequired":
        return { eyebrow: "License check", title: "GridMode" };
      default:
        return { eyebrow: "Welcome", title: "GridMode" };
    }
  };

  const body = (): JSX.Element => {
    if (status.otherMajorLicense) {
      return (
        <p>
          Your license is valid for GridMode {status.otherMajorLicense}.x. GridMode {status.appMajor} requires a
          GridMode {status.appMajor} license. You may continue using GridMode {status.otherMajorLicense}.x or upgrade
          to GridMode {status.appMajor}.
        </p>
      );
    }
    switch (status.state) {
      case "trialExpired":
        return <p>Your GridMode trial has ended.</p>;
      case "validationRequired":
        return <p>Connect to the internet so GridMode can confirm your license.</p>;
      default:
        return <p>Try GridMode free for {status.trialLengthDays} days, no credit card required.</p>;
    }
  };

  const { eyebrow, title } = heading();

  return (
    <section className="first-run-view license-gate">
      <div>
        <GridModeLogo />
        <p className="license-eyebrow">{eyebrow}</p>
        {title !== "GridMode" ? <h1>{title}</h1> : null}
        {body()}
        {status.notice ? (
          <div className="license-notice">
            <span>{status.notice}</span>
            <button
              className="icon-button"
              onClick={onDismissNotice}
              title="Dismiss"
            >
              <X size={16} />
            </button>
          </div>
        ) : null}
        <div className="first-run-actions">
          {status.state === "trialAvailable" ? (
            <button
              className="text-button primary"
              onClick={onStartTrial}
            >
              <span>Start {status.trialLengthDays}-day free trial</span>
            </button>
          ) : null}
          {status.state === "validationRequired" ? (
            <button
              className="text-button primary"
              onClick={onRefresh}
            >
              <RefreshCcw size={16} />
              <span>Try again</span>
            </button>
          ) : null}
          <button
            className="text-button"
            onClick={onOpenCheckout}
            disabled={buyDisabled}
            title={buyDisabled ? "Purchasing isn't available in this build" : undefined}
          >
            <Download size={16} />
            <span>Buy GridMode — {status.priceLabel}</span>
          </button>
          {!showKeyForm ? (
            <button
              className="text-button"
              onClick={() => setShowKeyForm(true)}
              disabled={!status.configured}
              title={!status.configured ? "Activation isn't available in this build" : undefined}
            >
              <KeyRound size={16} />
              <span>Enter license key</span>
            </button>
          ) : null}
        </div>
        {status.state === "trialAvailable" ? <p className="settings-note">No credit card required</p> : null}
        {!status.configured ? (
          <p className="settings-note">Purchasing and activation aren't available in this build.</p>
        ) : null}
        {showKeyForm && status.configured ? (
          <LicenseKeyForm onActivate={onActivate} />
        ) : null}
      </div>
    </section>
  );
}
