//! Central licensing configuration. Every Lemon Squeezy identifier GridMode
//! uses lives here; nothing else in the project hard-codes them.
//!
//! None of these values are secrets. The License API used for activation and
//! validation is designed to be called from distributed apps with only the
//! customer's license key; no Lemon Squeezy API key is embedded anywhere.

use std::time::Duration;

/// Identifiers for one Lemon Squeezy mode (Test Mode or Live Mode). Products
/// and variants created in Test Mode have different IDs from their Live Mode
/// copies, so each mode gets its own set.
#[derive(Clone, Copy, Debug)]
pub struct LemonSqueezyConfig {
    pub mode_label: &'static str,
    /// Your Lemon Squeezy store ID (the same in both modes).
    pub store_id: u64,
    /// The GridMode product ID.
    pub product_id: u64,
    /// Variant IDs that grant each major version, e.g. `(1, &[V1_VARIANT])`.
    /// A future GridMode 2.x build adds `(2, &[V2_VARIANT])` and keeps the
    /// 1.x entry so it can recognize (and politely explain) 1.x licenses.
    pub major_variants: &'static [(u32, &'static [u64])],
    /// Checkout page opened by "Buy GridMode".
    pub checkout_url: &'static str,
}

impl LemonSqueezyConfig {
    /// True once real identifiers have been filled in.
    pub fn is_configured(&self) -> bool {
        self.store_id != 0
            && self.product_id != 0
            && self
                .major_variants
                .iter()
                .any(|(_, variants)| variants.iter().any(|variant| *variant != 0))
    }
}

/// The major version this build licenses. Must match the `version` in
/// Cargo.toml / tauri.conf.json (a unit test enforces this).
pub const GRIDMODE_MAJOR_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Test Mode identifiers, confirmed with a Test Mode license key.
// See docs/licensing.md for where to find each value.
const TEST_STORE_ID: u64 = 484225;
const TEST_PRODUCT_ID: u64 = 1391055; // GridMode
const TEST_V1_VARIANT_ID: u64 = 2172597; // GridMode 1.x
const TEST_CHECKOUT_URL: &str =
    "https://store.standfaststudios.com/checkout/buy/526f1726-156b-43c6-8861-9c7d46754c75";

pub const TEST_MODE: LemonSqueezyConfig = LemonSqueezyConfig {
    mode_label: "Test Mode",
    store_id: TEST_STORE_ID,
    product_id: TEST_PRODUCT_ID,
    major_variants: &[(1, &[TEST_V1_VARIANT_ID])],
    checkout_url: TEST_CHECKOUT_URL,
};

// TODO(lemon-squeezy): Live Mode values. Leave these unset until you are ready
// to sell; ACTIVE below stays on TEST_MODE until then.
const LIVE_STORE_ID: u64 = 0;
const LIVE_PRODUCT_ID: u64 = 0;
const LIVE_V1_VARIANT_ID: u64 = 0;
const LIVE_CHECKOUT_URL: &str = "";

#[allow(dead_code)] // Switched to by changing ACTIVE when Live Mode values exist.
pub const LIVE_MODE: LemonSqueezyConfig = LemonSqueezyConfig {
    mode_label: "Live Mode",
    store_id: LIVE_STORE_ID,
    product_id: LIVE_PRODUCT_ID,
    major_variants: &[(1, &[LIVE_V1_VARIANT_ID])],
    checkout_url: LIVE_CHECKOUT_URL,
};

/// The configuration this build uses. Do not switch to LIVE_MODE until the
/// Live Mode identifiers above are filled in.
pub const ACTIVE: LemonSqueezyConfig = TEST_MODE;
// ---------------------------------------------------------------------------

pub const PRICE_LABEL: &str = "$10";
pub const TRIAL_DAYS: i64 = 10;
/// Lemon Squeezy is set up to allow this many activations per license key;
/// used only to word the "limit reached" message.
pub const ACTIVATION_LIMIT: u32 = 2;

/// Revalidate a license with Lemon Squeezy when the last successful check is
/// older than this.
pub const VALIDATION_INTERVAL: Duration = Duration::from_secs(3 * 24 * 60 * 60);
/// How long a validated license keeps working without reaching Lemon Squeezy.
pub const OFFLINE_GRACE_PERIOD: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// How often the background task considers revalidating.
pub const REVALIDATION_POLL: Duration = Duration::from_secs(6 * 60 * 60);

pub const LICENSE_API_BASE: &str = "https://api.lemonsqueezy.com/v1/licenses";
