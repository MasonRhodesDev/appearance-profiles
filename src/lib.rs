//! Normative resolver for the appearance-profiles standard.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;
pub const PACKAGED_PROFILE: &str = "/usr/share/appearance-profiles/default.toml";
pub const SYSTEM_PROFILE: &str = "/etc/appearance-profiles/default.toml";
pub const PUBLISHED_ROOT: &str = "/var/lib/appearance-profiles/users";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("parse {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("appearance profile version {0} is unsupported")]
    Version(u32),
    #[error("invalid user name {0:?}")]
    User(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    #[default]
    Fill,
    Fit,
    Stretch,
    Center,
    Tile,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Background {
    pub path: Option<PathBuf>,
    pub fit: Option<Fit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub version: u32,
    pub background: Background,
    pub output: BTreeMap<String, Background>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            background: Background::default(),
            output: BTreeMap::new(),
        }
    }
}

impl Profile {
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        let mut value: Self = toml::from_str(&source).map_err(|source| Error::Parse {
            path: path.to_owned(),
            source,
        })?;
        if value.version != SCHEMA_VERSION {
            return Err(Error::Version(value.version));
        }
        relativize(&mut value, path.parent().unwrap_or_else(|| Path::new(".")));
        Ok(Some(value))
    }
}

fn relativize(profile: &mut Profile, base: &Path) {
    let resolve = |value: &mut Background| {
        if let Some(path) = &value.path
            && path.is_relative()
        {
            value.path = Some(base.join(path));
        }
    };
    resolve(&mut profile.background);
    profile.output.values_mut().for_each(resolve);
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputIdentity {
    pub connector: String,
    pub description: Option<String>,
}

impl OutputIdentity {
    pub fn new(connector: impl Into<String>, description: Option<String>) -> Self {
        Self {
            connector: connector.into(),
            description,
        }
    }

    fn selectors(&self) -> impl Iterator<Item = String> + '_ {
        std::iter::once(self.connector.clone()).chain(
            self.description
                .as_ref()
                .map(|value| format!("desc:{value}")),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Runtime,
    UserOutput,
    User,
    SystemOutput,
    System,
    PackagedOutput,
    Packaged,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedBackground {
    pub path: Option<PathBuf>,
    pub fit: Fit,
    pub path_source: Source,
    pub fit_source: Source,
}

#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub packaged: Option<Profile>,
    pub system: Option<Profile>,
    pub user: Option<Profile>,
}

impl Registry {
    pub fn load(user_path: Option<&Path>) -> Result<Self> {
        Ok(Self {
            packaged: Profile::load(Path::new(PACKAGED_PROFILE))?,
            system: Profile::load(Path::new(SYSTEM_PROFILE))?,
            user: match user_path {
                Some(path) => Profile::load(path)?,
                None => None,
            },
        })
    }

    pub fn load_current_user() -> Result<Self> {
        Self::load(user_profile_path().as_deref())
    }

    pub fn load_published(user: &str) -> Result<Self> {
        Self::load(Some(&published_profile_path(user)?))
    }

    pub fn resolve(
        &self,
        output: &OutputIdentity,
        runtime: Option<&Background>,
    ) -> ResolvedBackground {
        let mut path = None;
        let mut fit = None;
        let mut path_source = Source::Default;
        let mut fit_source = Source::Default;
        let mut apply = |rule: Option<&Background>, source| {
            if let Some(rule) = rule {
                if rule.path.is_some() {
                    path = rule.path.clone();
                    path_source = source;
                }
                if rule.fit.is_some() {
                    fit = rule.fit;
                    fit_source = source;
                }
            }
        };
        apply(
            self.packaged.as_ref().map(|v| &v.background),
            Source::Packaged,
        );
        apply(
            matching(self.packaged.as_ref(), output),
            Source::PackagedOutput,
        );
        apply(self.system.as_ref().map(|v| &v.background), Source::System);
        apply(matching(self.system.as_ref(), output), Source::SystemOutput);
        apply(self.user.as_ref().map(|v| &v.background), Source::User);
        apply(matching(self.user.as_ref(), output), Source::UserOutput);
        apply(runtime, Source::Runtime);
        ResolvedBackground {
            path,
            fit: fit.unwrap_or_default(),
            path_source,
            fit_source,
        }
    }
}

fn matching<'a>(profile: Option<&'a Profile>, output: &OutputIdentity) -> Option<&'a Background> {
    let profile = profile?;
    output
        .selectors()
        .find_map(|selector| profile.output.get(&selector))
}

pub fn user_profile_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".config")))
        .map(|root| root.join("appearance-profiles/default.toml"))
}

pub fn published_profile_path(user: &str) -> Result<PathBuf> {
    if user.is_empty()
        || !user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::User(user.to_owned()));
    }
    Ok(Path::new(PUBLISHED_ROOT).join(user).join("default.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bg(path: &str, fit: Option<Fit>) -> Background {
        Background {
            path: Some(path.into()),
            fit,
        }
    }

    #[test]
    fn resolution_is_fieldwise_and_layered() {
        let mut packaged = Profile::default();
        packaged.background = bg("/packaged", Some(Fit::Fit));
        let mut system = Profile::default();
        system.background.path = Some("/system".into());
        let mut user = Profile::default();
        user.output.insert(
            "DP-1".into(),
            Background {
                path: None,
                fit: Some(Fit::Tile),
            },
        );
        let result = Registry {
            packaged: Some(packaged),
            system: Some(system),
            user: Some(user),
        }
        .resolve(&OutputIdentity::new("DP-1", None), None);
        assert_eq!(result.path, Some("/system".into()));
        assert_eq!(result.fit, Fit::Tile);
        assert_eq!(result.path_source, Source::System);
        assert_eq!(result.fit_source, Source::UserOutput);
    }

    #[test]
    fn description_and_runtime_precedence() {
        let mut user = Profile::default();
        user.background.path = Some("/global".into());
        user.output
            .insert("desc:Dell Panel".into(), bg("/dell", None));
        let registry = Registry {
            user: Some(user),
            ..Registry::default()
        };
        let identity = OutputIdentity::new("DP-2", Some("Dell Panel".into()));
        assert_eq!(registry.resolve(&identity, None).path, Some("/dell".into()));
        let runtime = bg("/cli", Some(Fit::Stretch));
        let result = registry.resolve(&identity, Some(&runtime));
        assert_eq!(result.path, Some("/cli".into()));
        assert_eq!(result.fit, Fit::Stretch);
    }

    #[test]
    fn user_name_is_safe_for_a_path_component() {
        assert!(published_profile_path("mason").is_ok());
        assert!(published_profile_path("../root").is_err());
    }
}
