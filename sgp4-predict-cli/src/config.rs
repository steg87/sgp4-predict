//! Config file (`~/.sgp4-predict/config.yaml` by default) holding named ground stations.

use anyhow::Context as _;
use serde::Deserialize;
use sgp4_predict::{Degrees, Observer};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Config directory under the user's home directory.
const CONFIG_DIR: &str = ".sgp4-predict";
const CONFIG_FILE: &str = "config.yaml";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Ground stations keyed by the id passed to `--gs`.
    #[serde(default)]
    pub groundstations: BTreeMap<String, GroundStation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundStation {
    pub location: Location,
}

#[derive(Debug, Deserialize)]
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
";

/// Load the config from `path`, or from [`default_path`] when `path` is `None`.
///
/// An explicit `--config` path that does not exist is an error. A missing config
/// at the default path is not — it is seeded with [`TEMPLATE`] and read back.
pub fn load(path: Option<&Path>) -> anyhow::Result<Config> {
    match path {
        Some(p) => read(p),
        None => match default_path() {
            Some(p) if p.is_file() => read(&p),
            Some(p) => create_default(&p),
            None => Ok(Config::default()),
        },
    }
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
    std::fs::write(path, TEMPLATE)
        .with_context(|| format!("failed to write {}", path.display()))
}

impl Config {
    /// Look up a ground station by the id given to `--gs`, checking its coordinates.
    pub fn groundstation(&self, id: &str) -> anyhow::Result<&GroundStation> {
        let station = self.groundstations.get(id).ok_or_else(|| {
            if self.groundstations.is_empty() {
                anyhow::anyhow!("unknown ground station '{id}'; the config defines none")
            } else {
                anyhow::anyhow!(
                    "unknown ground station '{id}'; known ids: {}",
                    self.ids().join(", ")
                )
            }
        })?;
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
    fn validate(&self) -> anyhow::Result<()> {
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
        // Only the default path is seeded; --config must point at a real file.
        assert!(load(Some(Path::new("/nonexistent/sgp4-predict.yaml"))).is_err());
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
