//! Decides what a Lemon Squeezy license entitles, independent of storage and
//! networking. A key is never accepted just because Lemon Squeezy calls it
//! valid: the store, product, and variant must all belong to GridMode, and the
//! variant decides which major version it covers.

use super::config::LemonSqueezyConfig;

/// Lemon Squeezy's license key status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStatus {
    /// Valid, with no activations yet.
    Inactive,
    Active,
    Expired,
    Disabled,
    Unknown,
}

impl KeyStatus {
    pub fn parse(status: &str) -> Self {
        match status {
            "inactive" => Self::Inactive,
            "active" => Self::Active,
            "expired" => Self::Expired,
            "disabled" => Self::Disabled,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Active => "active",
            Self::Expired => "expired",
            Self::Disabled => "disabled",
            Self::Unknown => "unknown",
        }
    }
}

/// What Lemon Squeezy reported about a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LicenseFacts {
    pub status: KeyStatus,
    pub store_id: u64,
    pub product_id: u64,
    pub variant_id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entitlement {
    /// Licenses the major version this build is.
    Entitled {
        major: u32,
    },
    /// A genuine GridMode license, but for a different major version. Not an
    /// error and not "expired": the customer may keep using that version.
    OtherMajor {
        license_major: u32,
    },
    /// Belongs to another store, another product, or an unknown variant.
    WrongProduct,
    Expired,
    Disabled,
    /// This build has no Lemon Squeezy identifiers yet.
    NotConfigured,
}

/// The major version a variant grants, if it is one of GridMode's.
pub fn major_for_variant(config: &LemonSqueezyConfig, variant_id: u64) -> Option<u32> {
    if variant_id == 0 {
        return None;
    }
    config
        .major_variants
        .iter()
        .find(|(_, variants)| variants.contains(&variant_id))
        .map(|(major, _)| *major)
}

pub fn evaluate(facts: &LicenseFacts, config: &LemonSqueezyConfig, app_major: u32) -> Entitlement {
    if !config.is_configured() {
        return Entitlement::NotConfigured;
    }
    if facts.store_id != config.store_id || facts.product_id != config.product_id {
        return Entitlement::WrongProduct;
    }
    let Some(license_major) = major_for_variant(config, facts.variant_id) else {
        return Entitlement::WrongProduct;
    };
    // Only report expiry or revocation for keys that are GridMode's.
    match facts.status {
        KeyStatus::Expired => return Entitlement::Expired,
        KeyStatus::Disabled => return Entitlement::Disabled,
        KeyStatus::Inactive | KeyStatus::Active | KeyStatus::Unknown => {}
    }
    if license_major == app_major {
        Entitlement::Entitled {
            major: license_major,
        }
    } else {
        Entitlement::OtherMajor { license_major }
    }
}

/// The major component of a semantic version such as `1.8.4`.
#[cfg_attr(not(test), allow(dead_code))] // For builds that derive their major from the version.
pub fn major_version(version: &str) -> Option<u32> {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .next()?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: u64 = 111;
    const V2: u64 = 222;

    const V1_BUILD: LemonSqueezyConfig = LemonSqueezyConfig {
        mode_label: "Test",
        store_id: 10,
        product_id: 20,
        major_variants: &[(1, &[V1])],
        checkout_url: "https://example.lemonsqueezy.com/checkout",
    };

    /// What a future GridMode 2.x build's configuration will look like.
    const V2_BUILD: LemonSqueezyConfig = LemonSqueezyConfig {
        major_variants: &[(1, &[V1]), (2, &[V2])],
        ..V1_BUILD
    };

    fn facts(variant_id: u64, status: KeyStatus) -> LicenseFacts {
        LicenseFacts {
            status,
            store_id: 10,
            product_id: 20,
            variant_id,
        }
    }

    #[test]
    fn every_1x_version_accepts_the_1x_variant() {
        for version in ["1.0.0", "1.1.0", "1.8.4", "1.99.0"] {
            let major = major_version(version).unwrap();
            assert_eq!(
                evaluate(&facts(V1, KeyStatus::Active), &V1_BUILD, major),
                Entitlement::Entitled { major: 1 },
                "{version}"
            );
        }
    }

    #[test]
    fn a_2x_build_requires_the_2x_variant() {
        let major = major_version("2.0.0").unwrap();
        assert_eq!(
            evaluate(&facts(V2, KeyStatus::Active), &V2_BUILD, major),
            Entitlement::Entitled { major: 2 }
        );
    }

    #[test]
    fn a_2x_build_recognizes_a_valid_1x_license_as_other_major() {
        assert_eq!(
            evaluate(&facts(V1, KeyStatus::Active), &V2_BUILD, 2),
            Entitlement::OtherMajor { license_major: 1 }
        );
    }

    #[test]
    fn a_1x_build_rejects_variants_it_does_not_know() {
        // A 1.x build has never heard of the 2.x variant.
        assert_eq!(
            evaluate(&facts(V2, KeyStatus::Active), &V1_BUILD, 1),
            Entitlement::WrongProduct
        );
    }

    #[test]
    fn other_stores_and_products_are_rejected_even_when_valid() {
        let mut other_store = facts(V1, KeyStatus::Active);
        other_store.store_id = 99;
        assert_eq!(
            evaluate(&other_store, &V1_BUILD, 1),
            Entitlement::WrongProduct
        );

        let mut other_product = facts(V1, KeyStatus::Active);
        other_product.product_id = 99;
        assert_eq!(
            evaluate(&other_product, &V1_BUILD, 1),
            Entitlement::WrongProduct
        );
    }

    #[test]
    fn expiry_is_reported_only_when_lemon_squeezy_says_so() {
        assert_eq!(
            evaluate(&facts(V1, KeyStatus::Expired), &V1_BUILD, 1),
            Entitlement::Expired
        );
        assert_eq!(
            evaluate(&facts(V1, KeyStatus::Disabled), &V1_BUILD, 1),
            Entitlement::Disabled
        );
        // An older-major license is never called expired on its own.
        assert_eq!(
            evaluate(&facts(V1, KeyStatus::Inactive), &V2_BUILD, 2),
            Entitlement::OtherMajor { license_major: 1 }
        );
    }

    #[test]
    fn unconfigured_builds_accept_nothing() {
        const EMPTY: LemonSqueezyConfig = LemonSqueezyConfig {
            mode_label: "Test",
            store_id: 0,
            product_id: 0,
            major_variants: &[(1, &[0])],
            checkout_url: "",
        };
        let zero = LicenseFacts {
            status: KeyStatus::Active,
            store_id: 0,
            product_id: 0,
            variant_id: 0,
        };
        assert_eq!(evaluate(&zero, &EMPTY, 1), Entitlement::NotConfigured);
    }

    #[test]
    fn parses_major_versions() {
        assert_eq!(major_version("1.0.0"), Some(1));
        assert_eq!(major_version("v2.3.4-beta.1"), Some(2));
        assert_eq!(major_version("x.y"), None);
    }

    #[test]
    fn the_configured_major_matches_the_app_version() {
        assert_eq!(
            major_version(env!("CARGO_PKG_VERSION")),
            Some(super::super::config::GRIDMODE_MAJOR_VERSION)
        );
    }
}
