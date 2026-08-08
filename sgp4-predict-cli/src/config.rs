//! Config file (`~/.sgp4-predict/config.yaml` by default) holding named ground
//! stations and areas of interest.

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use sgp4_predict::{Degrees, Observer};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Config directory under the user's home directory.
const CONFIG_DIR: &str = ".sgp4-predict";
const CONFIG_FILE: &str = "config.yaml";

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Ground stations keyed by the id passed to `--gs`.
    /// Omitted when empty, so a config with only AOIs does not grow a stub.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub groundstations: BTreeMap<String, GroundStation>,
    /// Areas of interest keyed by the id passed to `--aoi`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aois: BTreeMap<String, AoiDef>,
}

/// A region on the ground, as written in the config file.
///
/// Internally tagged on `shape`, so each AOI is a flat map of named fields:
///
/// ```yaml
/// aois:
///   scotland:
///     shape: box
///     south: 54.0
///     north: 60.0
///     west: -8.0
///     east: -1.0
/// ```
///
/// Externally tagged (`box: { ... }`) would read as well, but serde_yaml
/// represents that with a `!Box` YAML tag rather than a nested map, which is
/// not something to hand-write.
///
/// This is the *stored* form. [`AoiDef::build`] turns it into the library
/// shape, which is where the geometry is validated.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "shape", rename_all = "lowercase")]
pub enum AoiDef {
    /// A latitude/longitude box, given by its bounds.
    Box(BoxDef),
    Ellipse(EllipseDef),
    Circle(CircleDef),
    /// A ring of at least three vertices, closing implicitly.
    Polygon(PolygonDef),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoxDef {
    /// Southern latitude bound in degrees.
    pub south: f64,
    /// Northern latitude bound in degrees.
    pub north: f64,
    /// Western longitude bound in degrees.
    pub west: f64,
    /// Eastern longitude bound in degrees. The box runs **eastward** from
    /// `west`, so an `east` at a smaller longitude wraps the antimeridian.
    pub east: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EllipseDef {
    /// Centre latitude in degrees.
    pub latitude: f64,
    /// Centre longitude in degrees.
    pub longitude: f64,
    /// Semi-axis along `bearing`, in degrees of arc (about 111.2 km per
    /// degree). Either semi-axis may be the longer.
    pub semi_axis_a: f64,
    /// Semi-axis across `bearing`, in degrees of arc.
    pub semi_axis_b: f64,
    /// Bearing of `semi_axis_a`, degrees clockwise from north.
    #[serde(default)]
    pub bearing: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CircleDef {
    /// Centre latitude in degrees.
    pub latitude: f64,
    /// Centre longitude in degrees.
    pub longitude: f64,
    /// Radius in degrees of arc (about 111.2 km per degree).
    pub radius: f64,
}

/// Wraps the vertex list in a struct because an internally tagged enum cannot
/// carry a bare sequence — the tag has nowhere to live.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolygonDef {
    pub vertices: Vec<Vertex>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Vertex {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GroundStation {
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres above the ellipsoid; defaults to 0.
    #[serde(default)]
    pub altitude: f64,
}

/// Default config path: `<home>/.sgp4-predict/config.yaml`, on every platform.
/// `None` when the home directory cannot be determined.
pub fn default_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(CONFIG_DIR).join(CONFIG_FILE))
}

/// Starter config written to [`default_path`] the first time it is needed.
const TEMPLATE: &str = "\
# sgp4-predict ground stations. Select one with `--gs <id>`.
# `altitude` is metres above the ellipsoid and defaults to 0.
groundstations:
  glasgow:
    location:
      latitude: 55.86
      longitude: -4.25
      altitude: 40

# Areas of interest. Select one with `--aoi <id>`.
# All coordinates are in degrees — about 111.2 km per degree of arc.
aois:
  scotland:
    shape: box
    south: 54.0
    north: 60.0
    west: -8.0
    east: -1.0
";

/// Load the config from `path`, or from [`default_path`] when `path` is `None`.
///
/// A missing file at the *default* path is seeded with [`TEMPLATE`] — you did
/// not name it, so it cannot be a typo. A missing `--config` path is an error:
/// creating it would let a mistyped path succeed against a fresh empty config
/// while the real stations sit unread somewhere else. Use `gs add` to create
/// one deliberately.
pub fn load(path: Option<&Path>) -> anyhow::Result<Config> {
    match path {
        Some(p) if p.is_file() => read(p),
        Some(p) => Err(missing_config_error(p)),
        None => match default_path() {
            Some(p) if p.is_file() => read(&p),
            Some(p) => create_default(&p),
            None => Ok(Config::default()),
        },
    }
}

fn missing_config_error(path: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "config file {} does not exist\n       create one with `sgp4-predict gs add --config {}`",
        path.display(),
        path.display()
    )
}

fn read(path: &Path) -> anyhow::Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    serde_yaml::from_str(&text)
        .with_context(|| format!("failed to parse config file {}", path.display()))
}

/// Seed the default config with an example station, then read it back.
///
/// Best-effort: an unwritable home directory falls back to an empty config
/// rather than failing the command outright.
fn create_default(path: &Path) -> anyhow::Result<Config> {
    match write_template(path) {
        Ok(()) => {
            eprintln!("created example config at {}", path.display());
            read(path)
        }
        Err(e) => {
            eprintln!(
                "warning: could not create config at {}: {e:#}",
                path.display()
            );
            Ok(Config::default())
        }
    }
}

fn write_template(path: &Path) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
    }
    std::fs::write(path, TEMPLATE).with_context(|| format!("failed to write {}", path.display()))
}

/// Header re-emitted on every save, since serialising drops YAML comments.
const SAVED_HEADER: &str = "\
# sgp4-predict ground stations (`--gs <id>`) and areas of interest (`--aoi <id>`).
# Managed by `sgp4-predict gs add|remove|list` and `sgp4-predict aoi add|remove|list`;
# hand edits are preserved, but comments are not.
";

/// Whether a `gs` subcommand may create the config it was pointed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Missing {
    /// `gs add` — a missing file is the empty starting point to write to.
    Create,
    /// `gs list` / `gs remove` — a missing file means the wrong path, not an
    /// empty station list, so say so rather than printing nothing.
    Reject,
}

/// Open the config for editing by the `gs` subcommands, with the path to save to.
///
/// Unlike [`load`], a missing file is never seeded with the example station:
/// `gs add` starts from empty. Parse errors always fail, so a broken config is
/// not silently overwritten.
pub fn open_for_edit(path: Option<&Path>, missing: Missing) -> anyhow::Result<(Config, PathBuf)> {
    match path {
        Some(p) if p.is_file() => Ok((read(p)?, p.to_path_buf())),
        // An explicit path that does not exist: only `gs add` may create it.
        Some(p) if missing == Missing::Create => Ok((Config::default(), p.to_path_buf())),
        Some(p) => Err(missing_config_error(p)),
        None => {
            let p = default_path().context("cannot locate a home directory for the config file")?;
            // The default path cannot be mistyped, so a missing file is just
            // an empty station list rather than an error.
            let config = if p.is_file() {
                read(&p)?
            } else {
                Config::default()
            };
            Ok((config, p))
        }
    }
}

impl Config {
    /// Write the config back, creating parent directories as needed.
    ///
    /// Writes to a sibling temporary file and renames, so an interrupted or
    /// failed write cannot truncate an existing config.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }

        let body = serde_yaml::to_string(self).context("failed to serialise config")?;
        // Process-unique, so two concurrent runs cannot clobber each other's
        // temporary file.
        let temp = path.with_extension(format!("yaml.{}.tmp", std::process::id()));

        let written = std::fs::write(&temp, format!("{SAVED_HEADER}{body}"))
            .with_context(|| format!("failed to write {}", temp.display()))
            .and_then(|()| {
                std::fs::rename(&temp, path)
                    .with_context(|| format!("failed to replace {}", path.display()))
            });
        if written.is_err() {
            // Do not leave the partial file next to the real config.
            let _ = std::fs::remove_file(&temp);
        }
        written
    }

    /// Look up a ground station by id, without checking its coordinates.
    ///
    /// `gs remove` uses this: a hand-edited station with an out-of-range
    /// latitude is still listed by `gs list`, so it must be removable too, or
    /// the only way to get rid of it is to edit the YAML by hand.
    pub fn find(&self, id: &str) -> anyhow::Result<&GroundStation> {
        self.groundstations.get(id).ok_or_else(|| {
            if self.groundstations.is_empty() {
                anyhow::anyhow!("unknown ground station '{id}'; the config defines none")
            } else {
                anyhow::anyhow!(
                    "unknown ground station '{id}'; known ids: {}",
                    self.ids().join(", ")
                )
            }
        })
    }

    /// Look up a ground station by the id given to `--gs`, checking its coordinates.
    ///
    /// Used by the prediction commands, where propagating from a nonsensical
    /// location would produce silently wrong results.
    pub fn groundstation(&self, id: &str) -> anyhow::Result<&GroundStation> {
        let station = self.find(id)?;
        station
            .location
            .validate()
            .with_context(|| format!("ground station '{id}'"))?;
        Ok(station)
    }

    /// Ground station ids in sorted order.
    pub fn ids(&self) -> Vec<&str> {
        self.groundstations.keys().map(String::as_str).collect()
    }

    /// `" (known ground stations: a, b)"`, or empty when the config defines none.
    pub fn ids_hint(&self) -> String {
        if self.groundstations.is_empty() {
            String::new()
        } else {
            format!(" (known ground stations: {})", self.ids().join(", "))
        }
    }

    /// Look up an AOI by id, without building or validating its geometry.
    ///
    /// `aoi remove` uses this, for the same reason [`Config::find`] exists: a
    /// hand-edited AOI that no longer builds is still listed, so it must be
    /// removable without editing the YAML by hand.
    pub fn find_aoi(&self, id: &str) -> anyhow::Result<&AoiDef> {
        self.aois.get(id).ok_or_else(|| {
            if self.aois.is_empty() {
                anyhow::anyhow!("unknown aoi '{id}'; the config defines none")
            } else {
                anyhow::anyhow!(
                    "unknown aoi '{id}'; known ids: {}",
                    self.aoi_ids().join(", ")
                )
            }
        })
    }

    /// AOI ids in sorted order.
    pub fn aoi_ids(&self) -> Vec<&str> {
        self.aois.keys().map(String::as_str).collect()
    }

    /// `" (known aois: a, b)"`, or empty when the config defines none.
    pub fn aoi_ids_hint(&self) -> String {
        if self.aois.is_empty() {
            String::new()
        } else {
            format!(" (known aois: {})", self.aoi_ids().join(", "))
        }
    }
}

/// A ground station is itself an observer — no conversion to `GroundObserver` needed.
/// Coordinates are range-checked by [`Config::groundstation`], the only way to obtain one.
impl Observer for GroundStation {
    fn latitude(&self) -> Degrees {
        Degrees(self.location.latitude)
    }

    fn longitude(&self) -> Degrees {
        Degrees(self.location.longitude)
    }

    fn altitude(&self) -> f64 {
        self.location.altitude
    }
}

impl Location {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (-90.0..=90.0).contains(&self.latitude),
            "latitude must be in [-90, 90], got {}",
            self.latitude
        );
        anyhow::ensure!(
            (-180.0..=180.0).contains(&self.longitude),
            "longitude must be in [-180, 180], got {}",
            self.longitude
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> anyhow::Result<Config> {
        serde_yaml::from_str(yaml).map_err(Into::into)
    }

    #[test]
    fn test_parses_groundstations() {
        let config = parse(
            r"
groundstations:
  glasgow:
    location:
      latitude: 55.86
      longitude: -4.25
      altitude: 40
  svalbard:
    location:
      latitude: 78.23
      longitude: 15.39
",
        )
        .unwrap();

        assert_eq!(config.ids(), ["glasgow", "svalbard"]);

        let gs = config.groundstation("glasgow").unwrap();
        assert_eq!(gs.latitude().to_f64(), 55.86);
        assert_eq!(gs.longitude().to_f64(), -4.25);
        assert_eq!(gs.altitude(), 40.0);
    }

    #[test]
    fn test_altitude_defaults_to_zero() {
        let config = parse(
            r"
groundstations:
  svalbard:
    location:
      latitude: 78.23
      longitude: 15.39
",
        )
        .unwrap();
        assert_eq!(config.groundstation("svalbard").unwrap().altitude(), 0.0);
    }

    #[test]
    fn test_unknown_id_lists_known_ids() {
        let config = parse(
            r"
groundstations:
  glasgow:
    location: { latitude: 55.86, longitude: -4.25 }
",
        )
        .unwrap();
        let err = config.groundstation("nope").unwrap_err().to_string();
        assert!(err.contains("known ids: glasgow"), "{err}");
    }

    #[test]
    fn test_unknown_id_with_empty_config() {
        let err = Config::default()
            .groundstation("nope")
            .unwrap_err()
            .to_string();
        assert!(err.contains("defines none"), "{err}");
    }

    #[test]
    fn test_out_of_range_latitude_rejected() {
        let config = parse(
            r"
groundstations:
  bad:
    location: { latitude: 91.0, longitude: 0.0 }
",
        )
        .unwrap();
        // Range checks happen on lookup, so a bad station cannot be observed from.
        let err = config.groundstation("bad").unwrap_err();
        assert!(err.to_string().contains("ground station 'bad'"), "{err}");
        assert!(
            format!("{err:#}").contains("latitude must be in [-90, 90]"),
            "{err:#}"
        );
    }

    #[test]
    fn test_unknown_field_rejected() {
        let err = parse(
            r"
groundstations:
  glasgow:
    location: { latitude: 55.86, longitude: -4.25 }
    antenna: dish
",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("antenna"), "{err}");
    }

    #[test]
    fn test_ids_hint() {
        assert_eq!(Config::default().ids_hint(), "");
        let config = parse(
            r"
groundstations:
  glasgow:
    location: { latitude: 55.86, longitude: -4.25 }
",
        )
        .unwrap();
        assert_eq!(config.ids_hint(), " (known ground stations: glasgow)");
    }

    #[test]
    fn test_explicit_missing_path_is_an_error() {
        // Creating it would let a typo'd --config succeed against a fresh
        // empty config while the real stations sit unread elsewhere.
        let err = load(Some(Path::new("/nonexistent/sgp4-predict.yaml")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not exist"), "{err}");
        assert!(err.contains("gs add"), "{err}");
        assert!(!Path::new("/nonexistent/sgp4-predict.yaml").exists());
    }

    #[test]
    fn test_open_for_edit_creates_only_for_add() {
        let dir = std::env::temp_dir().join("sgp4-predict-open-edit-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("stations.yaml");

        // Reject: a missing explicit path is the wrong path, not an empty list.
        assert!(open_for_edit(Some(&path), Missing::Reject).is_err());

        // Create: editing starts from empty, not from the example station, and
        // touches nothing on disk until save.
        let (config, resolved) = open_for_edit(Some(&path), Missing::Create).unwrap();
        assert!(config.groundstations.is_empty());
        assert_eq!(resolved, path);
        assert!(!path.exists());

        config.save(&path).unwrap();
        assert!(path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_save_round_trips() {
        let dir = std::env::temp_dir().join("sgp4-predict-save-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("stations.yaml");

        let original = parse(
            r"
groundstations:
  glasgow:
    location: { latitude: 55.86, longitude: -4.25, altitude: 40 }
  svalbard:
    location: { latitude: 78.23, longitude: 15.39 }
",
        )
        .unwrap();
        original.save(&path).unwrap();

        let reloaded = read(&path).unwrap();
        assert_eq!(reloaded.ids(), ["glasgow", "svalbard"]);
        assert_eq!(reloaded.groundstation("glasgow").unwrap().altitude(), 40.0);
        assert_eq!(reloaded.groundstation("svalbard").unwrap().altitude(), 0.0);

        // No stray temp file is left next to it.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n.to_string_lossy().contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_find_skips_validation_but_groundstation_does_not() {
        let config = parse(
            r"
groundstations:
  bad:
    location: { latitude: 91.0, longitude: 0.0 }
",
        )
        .unwrap();
        // find() must succeed so `gs remove` can delete a bad entry.
        assert!(config.find("bad").is_ok());
        assert!(config.groundstation("bad").is_err());
        // Both still report an unknown id the same way.
        assert!(
            config
                .find("nope")
                .unwrap_err()
                .to_string()
                .contains("known ids: bad"),
            "find lost the known-ids hint"
        );
    }

    #[test]
    fn test_create_default_seeds_glasgow() {
        let dir = std::env::temp_dir().join("sgp4-predict-create-default-test");
        let _ = std::fs::remove_dir_all(&dir);
        // Nested, so the parent directory has to be created too.
        let path = dir.join(CONFIG_DIR).join(CONFIG_FILE);

        let config = create_default(&path).unwrap();
        assert_eq!(config.ids(), ["glasgow"]);

        let gs = config.groundstation("glasgow").unwrap();
        assert_eq!(gs.latitude().to_f64(), 55.86);
        assert_eq!(gs.longitude().to_f64(), -4.25);
        assert_eq!(gs.altitude(), 40.0);

        // What was returned is what landed on disk.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_create_default_falls_back_when_unwritable() {
        // A path under a plain file cannot be created; the command still runs.
        let file = std::env::temp_dir().join("sgp4-predict-unwritable-test");
        std::fs::write(&file, "not a directory").unwrap();

        let config = create_default(&file.join("config.yaml")).unwrap();
        assert!(config.groundstations.is_empty());
        std::fs::remove_file(&file).unwrap();
    }
}
