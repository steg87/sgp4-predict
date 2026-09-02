use std::path::Path;

use crate::{
    cli::ObserverArgs,
    config::{self, GroundStation},
};

impl ObserverArgs {
    /// Enforce that `--gs` is given and names a usable station in the config.
    ///
    /// Returns the resolved id, so callers do not have to unwrap it again.
    pub fn validate<'a>(&'a self, config: &config::Config) -> anyhow::Result<&'a str> {
        let id = self
            .gs
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--gs is required{}", config.ids_hint()))?;
        config.groundstation(id)?;
        Ok(id)
    }

    /// Validate the flags and take ownership of the named ground station.
    pub fn resolve(&self, config_path: Option<&Path>) -> anyhow::Result<GroundStation> {
        let mut config = config::load(config_path)?;
        let id = self.validate(&config)?;
        Ok(config
            .groundstations
            .remove(id)
            .expect("validate checked the id"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sgp4_predict::GeodeticPoint;

    fn args(gs: Option<&str>) -> ObserverArgs {
        ObserverArgs {
            gs: gs.map(str::to_owned),
        }
    }

    fn config() -> config::Config {
        serde_yaml::from_str(
            r"
groundstations:
  glasgow:
    location: { latitude: 55.86, longitude: -4.25, altitude: 40 }
  svalbard:
    location: { latitude: 78.23, longitude: 15.39 }
",
        )
        .unwrap()
    }

    #[test]
    fn test_validate_accepts_known_id() {
        assert_eq!(
            args(Some("glasgow")).validate(&config()).unwrap(),
            "glasgow"
        );
    }

    #[test]
    fn test_validate_rejects_missing_gs() {
        let err = args(None).validate(&config()).unwrap_err().to_string();
        assert!(err.contains("--gs is required"), "{err}");
        assert!(err.contains("glasgow, svalbard"), "{err}");
    }

    #[test]
    fn test_validate_rejects_unknown_id() {
        let err = args(Some("nowhere"))
            .validate(&config())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown ground station 'nowhere'"), "{err}");
        assert!(err.contains("glasgow, svalbard"), "{err}");
    }

    #[test]
    fn test_validate_missing_gs_with_empty_config() {
        let err = args(None)
            .validate(&config::Config::default())
            .unwrap_err()
            .to_string();
        assert_eq!(err, "--gs is required");
    }

    #[test]
    fn test_resolved_station_converts_to_a_geodetic_point() {
        let mut config = config();
        let args = args(Some("glasgow"));
        let id = args.validate(&config).unwrap();
        let gs = config.groundstations.remove(id).unwrap();
        let point = GeodeticPoint::from(&gs);
        assert_eq!(point.latitude.to_f64(), 55.86);
        assert_eq!(point.longitude.to_f64(), -4.25);
        assert_eq!(point.altitude, 40.0);
    }
}
