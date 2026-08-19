//! Normative resolver for the appearance-profiles standard.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;
pub const PACKAGED_PROFILE: &str = "/usr/share/appearance-profiles/default.toml";
pub const SYSTEM_PROFILE: &str = "/etc/appearance-profiles/default.toml";
pub const PUBLISHED_ROOT: &str = "/var/lib/appearance-profiles/users";
pub const BUNDLE_VERSION: u32 = 1;
pub const BUNDLE_FILE: &str = "bundle.toml";
const PIXEL_MAGIC: &[u8; 8] = b"APRGBA1\0";
const XRGB_MAGIC: &[u8; 8] = b"APXRGB1\0";

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
    #[error("write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid prepared appearance asset {path}: {message}")]
    Asset { path: PathBuf, message: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// Atomically published, producer-owned appearance bundle. LMTT is the
/// producer; greeters, lockers, and shells are read-only consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreparedBundle {
    pub version: u32,
    pub tokens: Option<PathBuf>,
    pub backgrounds: Vec<PreparedBackground>,
}

impl Default for PreparedBundle {
    fn default() -> Self {
        Self {
            version: BUNDLE_VERSION,
            tokens: None,
            backgrounds: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedBackground {
    pub selectors: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub fit: Fit,
    #[serde(default)]
    pub format: PixelFormat,
    pub asset: PathBuf,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PixelFormat {
    #[default]
    Rgba8,
    Xrgb8888Le,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedPixels {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
}

impl PreparedBundle {
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::Read {
                    path: path.into(),
                    source,
                });
            }
        };
        let mut bundle: Self = toml::from_str(&source).map_err(|source| Error::Parse {
            path: path.into(),
            source,
        })?;
        if bundle.version != BUNDLE_VERSION {
            return Err(Error::Version(bundle.version));
        }
        let root = path.parent().unwrap_or_else(|| Path::new("."));
        if let Some(tokens) = &bundle.tokens
            && tokens.is_relative()
        {
            bundle.tokens = Some(root.join(tokens));
        }
        for background in &mut bundle.backgrounds {
            if background.asset.is_relative() {
                background.asset = root.join(&background.asset);
            }
        }
        Ok(Some(bundle))
    }

    pub fn load_published(user: &str) -> Result<Option<Self>> {
        Self::load(&published_bundle_path(user)?)
    }

    pub fn resolve(
        &self,
        output: &OutputIdentity,
        width: u32,
        height: u32,
        fit: Fit,
    ) -> Option<&PreparedBackground> {
        let selectors: Vec<_> = output.selectors().collect();
        self.backgrounds.iter().find(|background| {
            background.width == width
                && background.height == height
                && background.fit == fit
                && selectors
                    .iter()
                    .any(|selector| background.selectors.contains(selector))
        })
    }
}

pub fn read_prepared_asset(background: &PreparedBackground) -> Result<PreparedPixels> {
    let path = &background.asset;
    let bytes = std::fs::read(path).map_err(|source| Error::Read {
        path: path.clone(),
        source,
    })?;
    let expected = background.width as usize * background.height as usize * 4;
    let magic = match background.format {
        PixelFormat::Rgba8 => PIXEL_MAGIC,
        PixelFormat::Xrgb8888Le => XRGB_MAGIC,
    };
    if bytes.len() != magic.len() + expected || !bytes.starts_with(magic) {
        return Err(Error::Asset {
            path: path.clone(),
            message: format!("expected {} RGBA bytes", expected),
        });
    }
    Ok(PreparedPixels {
        bytes: bytes[magic.len()..].to_vec(),
        width: background.width,
        height: background.height,
        format: background.format,
    })
}

pub fn read_prepared_pixels(background: &PreparedBackground) -> Result<Vec<u8>> {
    let prepared = read_prepared_asset(background)?;
    if prepared.format != PixelFormat::Rgba8 {
        return Err(Error::Asset {
            path: background.asset.clone(),
            message: "asset is not RGBA8".into(),
        });
    }
    Ok(prepared.bytes)
}

pub fn write_prepared_xrgb(path: &Path, xrgb: &[u8], width: u32, height: u32) -> Result<()> {
    write_prepared_bytes(path, xrgb, width, height, XRGB_MAGIC)
}

fn write_prepared_bytes(
    path: &Path,
    pixels: &[u8],
    width: u32,
    height: u32,
    magic: &[u8; 8],
) -> Result<()> {
    let expected = width as usize * height as usize * 4;
    if pixels.len() != expected {
        return Err(Error::Asset {
            path: path.into(),
            message: format!("got {} bytes, expected {expected}", pixels.len()),
        });
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Write {
            path: parent.into(),
            source,
        })?;
    }
    let temporary = path.with_extension("pixels.tmp");
    let mut bytes = Vec::with_capacity(magic.len() + pixels.len());
    bytes.extend_from_slice(magic);
    bytes.extend_from_slice(pixels);
    std::fs::write(&temporary, bytes).map_err(|source| Error::Write {
        path: temporary.clone(),
        source,
    })?;
    std::fs::rename(&temporary, path).map_err(|source| Error::Write {
        path: path.into(),
        source,
    })
}

pub fn rgba_to_xrgb8888_le(rgba: &[u8]) -> Vec<u8> {
    let mut xrgb = Vec::with_capacity(rgba.len());
    for pixel in rgba.chunks_exact(4) {
        xrgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 0]);
    }
    xrgb
}

pub fn write_prepared_pixels(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<()> {
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(Error::Asset {
            path: path.into(),
            message: format!("got {} bytes, expected {expected}", rgba.len()),
        });
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Write {
            path: parent.into(),
            source,
        })?;
    }
    let temporary = path.with_extension("rgba.tmp");
    let mut bytes = Vec::with_capacity(PIXEL_MAGIC.len() + rgba.len());
    bytes.extend_from_slice(PIXEL_MAGIC);
    bytes.extend_from_slice(rgba);
    std::fs::write(&temporary, bytes).map_err(|source| Error::Write {
        path: temporary.clone(),
        source,
    })?;
    std::fs::rename(&temporary, path).map_err(|source| Error::Write {
        path: path.into(),
        source,
    })
}

#[cfg(feature = "builder")]
pub fn prepare_background(path: &Path, fit: Fit, width: u32, height: u32) -> Result<Vec<u8>> {
    use image::{RgbaImage, imageops};
    let source = image::open(path).map_err(|error| Error::Asset {
        path: path.into(),
        message: error.to_string(),
    })?;
    let source = source.to_rgba8();
    let (in_w, in_h) = source.dimensions();
    let mut output = RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 255]));
    let output = match fit {
        Fit::Stretch => imageops::resize(&source, width, height, imageops::FilterType::Triangle),
        Fit::Fill | Fit::Fit => {
            let scale_w = width as f64 / in_w as f64;
            let scale_h = height as f64 / in_h as f64;
            let scale = if fit == Fit::Fill {
                scale_w.max(scale_h)
            } else {
                scale_w.min(scale_h)
            };
            let resized = imageops::resize(
                &source,
                (in_w as f64 * scale).round().max(1.0) as u32,
                (in_h as f64 * scale).round().max(1.0) as u32,
                imageops::FilterType::Triangle,
            );
            let x = (i64::from(width) - i64::from(resized.width())).div_euclid(2);
            let y = (i64::from(height) - i64::from(resized.height())).div_euclid(2);
            imageops::overlay(&mut output, &resized, x, y);
            output
        }
        Fit::Center => {
            let x = (i64::from(width) - i64::from(in_w)).div_euclid(2);
            let y = (i64::from(height) - i64::from(in_h)).div_euclid(2);
            imageops::overlay(&mut output, &source, x, y);
            output
        }
        Fit::Tile => {
            for y in (0..height).step_by(in_h as usize) {
                for x in (0..width).step_by(in_w as usize) {
                    imageops::overlay(&mut output, &source, i64::from(x), i64::from(y));
                }
            }
            output
        }
    };
    Ok(output.into_raw())
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

pub fn published_bundle_path(user: &str) -> Result<PathBuf> {
    Ok(published_profile_path(user)?
        .parent()
        .expect("published profile has a parent")
        .join(BUNDLE_FILE))
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
        let packaged = Profile {
            background: bg("/packaged", Some(Fit::Fit)),
            ..Profile::default()
        };
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

    #[test]
    fn prepared_bundle_resolves_exact_output_geometry() {
        let bundle = PreparedBundle {
            backgrounds: vec![PreparedBackground {
                selectors: vec!["DP-1".into(), "desc:Dell Panel".into()],
                width: 3840,
                height: 2160,
                fit: Fit::Fill,
                format: PixelFormat::Rgba8,
                asset: "backgrounds/dell.rgba".into(),
            }],
            ..PreparedBundle::default()
        };
        let output = OutputIdentity::new("DP-1", Some("Dell Panel".into()));
        assert!(bundle.resolve(&output, 3840, 2160, Fit::Fill).is_some());
        assert!(bundle.resolve(&output, 1920, 1080, Fit::Fill).is_none());
    }

    #[test]
    fn prepared_pixel_file_validates_size_and_magic() {
        let root = std::env::temp_dir().join(format!("appearance-assets-{}", std::process::id()));
        let path = root.join("test.rgba");
        let pixels = vec![7; 4 * 3 * 2];
        write_prepared_pixels(&path, &pixels, 3, 2).unwrap();
        let background = PreparedBackground {
            selectors: vec!["DP-1".into()],
            width: 3,
            height: 2,
            fit: Fit::Fill,
            format: PixelFormat::Rgba8,
            asset: path.clone(),
        };
        assert_eq!(read_prepared_pixels(&background).unwrap(), pixels);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn xrgb_asset_round_trips_and_converts_channel_order() {
        let root = std::env::temp_dir().join(format!("appearance-xrgb-{}", std::process::id()));
        let path = root.join("test.xrgb");
        let xrgb = rgba_to_xrgb8888_le(&[0x11, 0x22, 0x33, 0xff]);
        assert_eq!(xrgb, [0x33, 0x22, 0x11, 0]);
        write_prepared_xrgb(&path, &xrgb, 1, 1).unwrap();
        let background = PreparedBackground {
            selectors: vec!["DP-1".into()],
            width: 1,
            height: 1,
            fit: Fit::Fill,
            format: PixelFormat::Xrgb8888Le,
            asset: path.clone(),
        };
        assert_eq!(read_prepared_asset(&background).unwrap().bytes, xrgb);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
