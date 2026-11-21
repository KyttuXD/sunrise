use super::{Side, both_sides, https_url, version_or_details};
use serde::{Deserialize, Deserializer};

/// An `[incompatible]` or `[discouraged]` entry: the versions of the other mod it matches
/// (`"<2"`), or detailed policy fields that also give the player a reason and a link.
#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    Version(semver::VersionReq),
    Detailed(RuleDetails),
}

impl<'de> Deserialize<'de> for Rule {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        version_or_details(deserializer, Self::Version, Self::Detailed)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuleDetails {
    /// Every version when left out.
    #[serde(default = "any_version")]
    pub version: semver::VersionReq,
    /// Why, shown when the rule fails: bound only by the complete manifest's byte limit.
    #[serde(default)]
    pub reason: Option<String>,
    /// Where to read more, shown with the reason: an `https://` link checked at its value.
    #[serde(default, deserialize_with = "https_url")]
    pub url: Option<String>,
    /// Where the rule holds: both sides, or only the client or the dedicated server.
    #[serde(default = "both_sides")]
    pub side: Side,
}

fn any_version() -> semver::VersionReq {
    semver::VersionReq::STAR
}

impl Rule {
    /// The versions, reason, link and side, the bare form's as defaults.
    pub fn details(&self) -> RuleDetails {
        match self {
            Self::Version(version) => RuleDetails {
                version: version.clone(),
                reason: None,
                url: None,
                side: Side::Both,
            },
            Self::Detailed(details) => details.clone(),
        }
    }
}
