# GridMode licensing

## Overview

GridMode costs $10, a one-time purchase via Lemon Squeezy (not a subscription).

Licenses are per MAJOR version: a GridMode 1.x license covers 1.0, 1.1, and every 1.x release, but not 2.x. GridMode 2.x will use a separate Lemon Squeezy variant.

A 10-day free trial is available with no credit card required, starting from the first-launch screen. This is not Lemon Squeezy's subscription trial.

Each license allows 2 activations (devices). Users can deactivate a computer from Settings → License.

## How it works

GridMode's licensing system has these components:

**`src-tauri/src/licensing/config.rs`** — the only place with Lemon Squeezy identifiers. Defines:
- Checkout URL
- `GRIDMODE_MAJOR_VERSION` — the major version this build supports (currently 1)
- Trial length: `TRIAL_DAYS` (10)
- `VALIDATION_INTERVAL` — when to revalidate (3 days)
- `OFFLINE_GRACE_PERIOD` — how long a license works without network (30 days)
- `REVALIDATION_POLL` — how often the background task checks (6 hours)
- `ACTIVATION_LIMIT` — maximum activations per key (2)
- `ACTIVE` — selects `TEST_MODE` or `LIVE_MODE`

**`entitlement.rs`** — pure validation rules. A key is accepted only if its store_id, product_id, and variant_id match GridMode. The variant maps to a major version via `major_variants`. A valid license for another major version is reported as "other major" (never "expired"). Keys from other products or variants are rejected even if Lemon Squeezy marks them valid.

**`api.rs`** — calls the public Lemon Squeezy License API (`/v1/licenses/activate`, `/validate`, `/deactivate`) with only the customer's license key. No Lemon Squeezy API key is embedded in the app. Network failures are treated differently from explicit rejections.

**`store.rs`** — trial and license state are stored in the OS credential store (Windows Credential Manager / macOS Keychain) plus an encrypted, tamper-evident mirror file `license.dat` in the app data folder. Deleting either one alone does not reset the trial. The install is identified by a random ID; no hardware fingerprinting. Clock rollback cannot extend the trial.

**`mod.rs`** — the main service. Activation first validates the key (so a wrong-product key doesn't consume an activation), then activates with a privacy-conscious instance name like "GridMode · Windows · a1b2c3" (no hostname or user name). Revalidation runs in the background approximately 20 seconds after launch and every 6 hours when due (older than 3 days). Offline, a validated license keeps working for 30 days; after that the app asks the user to connect. Only an explicit Lemon Squeezy answer (disabled, expired, unknown key, activation removed) revokes a license.

**Enforcement** — backend library and photo commands and the image protocol refuse to work without a license or active trial. The UI shows a license screen.

**Debug builds** — run with `GRIDMODE_DATA_DIR` pointing to a separate folder use a separate credential entry (`license-dev`), so development never touches an installed copy's license.

**Existing users** updating to 1.0 will see the trial screen on first launch (they had no license before).

## Values you need from Lemon Squeezy

For Test Mode (Live Mode uses the `LIVE_*` equivalents in config.rs):

| Constant in config.rs | What it is | Where to find it |
|---|---|---|
| `TEST_STORE_ID` | Numeric store ID | Lemon Squeezy dashboard → Settings → Stores; visible in the store ID field and in License API validate response as `meta.store_id` |
| `TEST_PRODUCT_ID` | The GridMode product | Products → GridMode; numeric ID in the product page URL or "Product ID" field; also `meta.product_id` in a validate response |
| `TEST_V1_VARIANT_ID` | The "GridMode 1.x" variant | Products → GridMode → Variants; the variant ID; also `meta.variant_id` in a validate response |
| `TEST_CHECKOUT_URL` | The checkout link | Products → GridMode → Share / "Checkout link" (use the Test Mode link while Test Mode is on) |

## Setting up the Lemon Squeezy product

1. Turn on Test Mode in the Lemon Squeezy dashboard (toggle at the bottom-left).

2. Create a product:
   - Name: "GridMode"
   - Pricing: "Single payment"
   - Price: $10

3. Name the default variant "GridMode 1.x" (a future "GridMode 2.x" will be a separate variant or product).

4. Enable "Generate license keys" for the product/variant. Set:
   - Activation limit: 2
   - License length: "Unlimited" (perpetual; do not set an expiry)

5. Save, then copy the checkout link.

6. Find the IDs. The most reliable method:
   - Make a Test Mode purchase with test card 4242 4242 4242 4242 (any future expiration date, any CVC).
   - Copy the license key from the receipt email or Orders → license keys.
   - Run:
     ```
     curl -X POST https://api.lemonsqueezy.com/v1/licenses/validate \
       -H "Accept: application/json" \
       -d "license_key=YOUR-KEY"
     ```
   - Read `meta.store_id`, `meta.product_id`, and `meta.variant_id` from the response.
   - (This call needs no API key and does not consume an activation.)

7. Put the values into the `TEST_*` constants in config.rs. Rebuild with `pnpm dev`.

8. When going live:
   - Copy the product to Live Mode (Lemon Squeezy's "Copy to live mode").
   - Repeat step 6 with a live key, or read the IDs from the dashboard.
   - Fill in the `LIVE_*` constants.
   - Change `ACTIVE` to `LIVE_MODE`.
   - Note: Test Mode keys stop working in a Live Mode build and vice versa because the product/variant IDs differ. This is expected.

## Releasing GridMode 2.0 later

Create a checklist as you prepare for 2.0:

- Create the 2.x variant in Lemon Squeezy.
- In config.rs, set `GRIDMODE_MAJOR_VERSION = 2` and add `(2, &[V2_VARIANT_ID])` to `major_variants` while keeping the `(1, …)` entry so 1.x licenses are recognized and explained ("Your license is valid for GridMode 1.x. GridMode 2 requires a GridMode 2 license…").
- Bump the app version to 2.0.0 in Cargo.toml / tauri.conf.json (a unit test enforces that the major matches).
- Trials are tracked per major version, so 2.x users get a fresh trial.

## Test checklist

Run dev builds with `GRIDMODE_DATA_DIR` pointing to a scratch folder:

**PowerShell:**
```powershell
$env:GRIDMODE_DATA_DIR = "C:\temp\gridmode-dev"
pnpm dev
```

To reset for a fresh-install test, delete the folder AND the Windows Credential Manager entry: Control Panel → Credential Manager → Windows Credentials → find "license-dev.com.thedinz.gridmode" and remove it (or run `cmdkey /delete:license-dev.com.thedinz.gridmode`). Installed release builds use "license.com.thedinz.gridmode" instead. On macOS, open Keychain Access, search "com.thedinz.gridmode", and delete the credential.

**New installation**
- Expected: License screen with Start trial / Enter key / Buy buttons.
- How: Use a fresh `GRIDMODE_DATA_DIR` folder and fresh credential manager entry.

**Starting trial**
- Expected: App opens; top bar shows "GridMode Trial — 10 days remaining"; Settings → License shows "10-day trial / 10 days remaining".
- How: Click Start trial on the license screen.

**Trial persistence after restart**
- Expected: Still counting down, not reset.
- How: Close and reopen the app. Also test deleting only license.dat (keep the credential) — the trial should be the same. Verify the start trial button does not reset it.

**Valid license activation (Test Mode key for GridMode 1.x)**
- Expected: Licensed state shown; masked key displayed; activation appears in Lemon Squeezy dashboard (license key's activations list) with name like "GridMode · Windows · a1b2c3".
- How: Click Enter key, paste a valid Test Mode license key, press Enter.

**Invalid license**
- Expected: "We couldn't validate that license key. Check the key and try again."
- How: Enter a fake key.

**Wrong product (key from another product in the same store)**
- Expected: "This license key isn't for GridMode." No activation consumed.
- How: Buy a different product in Test Mode, then try its key in GridMode.

**Wrong major-version variant (1.x build with a 2.x key)**
- Expected: Key refused; no activation consumed.
- How: Create a temporary "GridMode 2.x" variant in Lemon Squeezy, buy it in Test Mode, then try its key in the 1.x build.

**Activation limit (2 activations)**
- Expected: Third activation shows "This license is already active on 2 computers…"
- How: Dev builds on one computer share a single credential entry, so use up the activations directly instead. Run this twice with a fresh Test Mode key (each run creates one activation):
  ```
  curl -X POST https://api.lemonsqueezy.com/v1/licenses/activate -H "Accept: application/json" -d "license_key=YOUR-KEY" -d "instance_name=Test slot"
  ```
  Then activate the same key in GridMode. Afterwards, deactivate the test slots in the Lemon Squeezy dashboard (the license key's activations list) and activate again — it now succeeds. Two separate computers or VMs also work.

**Deactivation**
- Expected: Settings → License → Deactivate This Computer → back to trial/expired state; activation count drops in Lemon Squeezy.
- How: Activate a license, then deactivate.

**Offline startup**
- Expected: Disconnect network, restart app → still Licensed, no alarming error.
- How: Activate, disconnect network, close and reopen app.
- Note: After >30 days offline, the app asks to connect. This is covered by automated tests; manual testing is impractical.

**Expired trial**
- Expected: (Covered by automated tests; manual testing requires clock adjustment.)
- How: Set system clock 11 days ahead before launching, then set it back. The trial stays expired because clock rollback is ignored.

**Revocation**
- Expected: After the license is disabled in the Lemon Squeezy dashboard, Settings → License (or wait for the background check to run) shows it's no longer accepted; notice displayed.
- How: Activate, disable the key in the Lemon Squeezy dashboard, then click Settings → License in the app.

**Lemon Squeezy Test Mode purchase**
- Expected: Buy GridMode button opens the Test Mode checkout in the browser; pay with 4242 4242 4242 4242; the emailed key activates in the app.
- How: Click Buy GridMode on the license screen.

**Automated coverage**

Run `pnpm test:rust` to execute approximately 30 licensing tests:
- Entitlement matrix including versions 1.0.0, 1.1.0, 1.8.4, 1.99.0, and 2.0.0
- API error mapping against a local stub server
- Storage merge and tamper detection
- Trial countdown
- Clock rollback protection
- Offline grace period
- License revocation
