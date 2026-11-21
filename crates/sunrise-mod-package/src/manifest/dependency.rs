use super::{Side, both_sides, https_url, version_or_details};
use serde::{Deserialize, Deserializer};

/// A `[dependencies]` entry: a semver requirement or detailed policy fields.
#[derive(Clone, Debug, PartialEq)]
pub enum Dependency {
    Version(semver::VersionReq),
    Detailed(DependencyDetails),
}

impl<'de> Deserialize<'de> for Dependency {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        version_or_details(deserializer, Self::Version, Self::Detailed)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DependencyDetails {
    pub version: semver::VersionReq,
    #[serde(default)]
    pub optional: bool,
    /// The installation on which this dependency declaration applies.
    #[serde(default = "both_sides")]
    pub side: Side,
    /// A mod-provided explanation, bound only by the complete manifest's byte limit.
    #[serde(default)]
    pub reason: Option<String>,
    /// An HTTPS link with more information, checked at the URL's value in the manifest.
    #[serde(default, deserialize_with = "https_url")]
    pub url: Option<String>,
    /// Dispatch ordering only; `none` does not remove a code link requirement.
    #[serde(default)]
    pub order: mod_vocab::DependencyOrder,
}

impl Dependency {
    /// The dependency's typed declaration, with bare-version defaults filled in.
    pub fn details(&self) -> DependencyDetails {
        match self {
            Self::Version(version) => DependencyDetails {
                version: version.clone(),
                optional: false,
                side: Side::Both,
                reason: None,
                url: None,
                order: mod_vocab::DependencyOrder::After,
            },
            Self::Detailed(details) => details.clone(),
        }
    }
}
