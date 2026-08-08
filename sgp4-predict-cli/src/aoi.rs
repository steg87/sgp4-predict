//! Turning a config [`AoiDef`] into a library shape, and resolving `--aoi`.
//!
//! Mirrors `observer.rs`: `validate` enforces that the flag names a usable
//! entry, `resolve` returns the built value.

use anyhow::Context as _;
use std::path::Path;

use crate::{
    cli::AoiArgs,
    config::{self, AoiDef},
};
use sgp4_predict::{Degrees, Ellipse, Polygon, Rectangle};

/// A built AOI, ready to hand to `aoi_iter`.
///
/// `Predictor::aoi_iter` is generic over one `Area`, so the three shapes need
/// a single type to travel in. There is deliberately no `impl Area for
/// AoiShape`: callers match once and pass the concrete shape to a generic
/// function, which keeps the dispatch at the call site instead of on every
/// sample.
#[derive(Debug, Clone, PartialEq)]
pub enum AoiShape {
    Rectangle(Rectangle),
    Ellipse(Ellipse),
    Polygon(Polygon),
}

impl AoiDef {
    /// Build the library shape, validating the geometry.
    pub fn build(&self) -> anyhow::Result<AoiShape> {
        Ok(match self {
            // The stored bounds are the corners `Rectangle` takes, so an
            // out-of-range latitude or an empty box names the field it came
            // from without any translation here.
            AoiDef::Box(b) => AoiShape::Rectangle(Rectangle::new(
                (Degrees(b.south), Degrees(b.west)),
                (Degrees(b.north), Degrees(b.east)),
            )?),
            AoiDef::Ellipse(e) => AoiShape::Ellipse(Ellipse::new(
                (Degrees(e.latitude), Degrees(e.longitude)),
                Degrees(e.semi_axis_a),
                Degrees(e.semi_axis_b),
                Degrees(e.bearing),
            )?),
            AoiDef::Circle(c) => AoiShape::Ellipse(Ellipse::circle(
                (Degrees(c.latitude), Degrees(c.longitude)),
                Degrees(c.radius),
            )?),
            AoiDef::Polygon(p) => AoiShape::Polygon(Polygon::new(
                p.vertices
                    .iter()
                    .map(|v| (Degrees(v.latitude), Degrees(v.longitude))),
            )?),
        })
    }

    /// The shape's name, as written in the config file.
    pub fn kind(&self) -> &'static str {
        match self {
            AoiDef::Box(_) => "box",
            AoiDef::Ellipse(_) => "ellipse",
            AoiDef::Circle(_) => "circle",
            AoiDef::Polygon(_) => "polygon",
        }
    }

    /// One-line rendering for `aoi list`, `aoi remove` and `--output-args`.
    ///
    /// Uses the config file's own field names rather than the flag's
    /// positional syntax, so a listing reads the same way the YAML does.
    pub fn describe(&self) -> String {
        match self {
            AoiDef::Box(b) => format!(
                "south={} north={} west={} east={}",
                b.south, b.north, b.west, b.east
            ),
            AoiDef::Ellipse(e) => format!(
                "latitude={} longitude={} semi_axis_a={} semi_axis_b={} bearing={}",
                e.latitude, e.longitude, e.semi_axis_a, e.semi_axis_b, e.bearing
            ),
            AoiDef::Circle(c) => format!(
                "latitude={} longitude={} radius={}",
                c.latitude, c.longitude, c.radius
            ),
            AoiDef::Polygon(p) => p
                .vertices
                .iter()
                .map(|v| format!("({}, {})", v.latitude, v.longitude))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

impl AoiArgs {
    /// Enforce that `--aoi` is given and names an AOI the config can build.
    ///
    /// Returns the resolved id, so callers do not have to unwrap it again.
    pub fn validate<'a>(&'a self, config: &config::Config) -> anyhow::Result<&'a str> {
        let id = self
            .id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--aoi is required{}", config.aoi_ids_hint()))?;
        config
            .find_aoi(id)?
            .build()
            .with_context(|| format!("aoi '{id}'"))?;
        Ok(id)
    }

    /// Validate the flags and build the named AOI.
    pub fn resolve(&self, config_path: Option<&Path>) -> anyhow::Result<(AoiDef, AoiShape)> {
        let mut config = config::load(config_path)?;
        let id = self.validate(&config)?;
        let def = config.aois.remove(id).expect("validate checked the id");
        let shape = def.build().expect("validate already built it");
        Ok((def, shape))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_approx_eq::assert_approx_eq;
    use sgp4_predict::Area as _;

    fn args(aoi: Option<&str>) -> AoiArgs {
        AoiArgs {
            id: aoi.map(str::to_owned),
        }
    }

    fn config() -> config::Config {
        serde_yaml::from_str(
            r"
aois:
  scotland:
    shape: box
    south: 54.0
    north: 60.0
    west: -8.0
    east: -1.0
  north-sea:
    shape: ellipse
    latitude: 56.0
    longitude: 2.0
    semi_axis_a: 2.7
    semi_axis_b: 1.1
    bearing: 45.0
  cape-town:
    shape: circle
    latitude: -33.9
    longitude: 18.4
    radius: 2.25
  corridor:
    shape: polygon
    vertices:
      - { latitude: 54.0, longitude: -8.0 }
      - { latitude: 54.0, longitude: -1.0 }
      - { latitude: 60.0, longitude: -1.0 }
",
        )
        .unwrap()
    }

    fn offset(shape: &AoiShape, lat: f64, lon: f64) -> f64 {
        let point = (Degrees(lat), Degrees(lon)).into();
        match shape {
            AoiShape::Rectangle(a) => a.signed_angular_offset(point),
            AoiShape::Ellipse(a) => a.signed_angular_offset(point),
            AoiShape::Polygon(a) => a.signed_angular_offset(point),
        }
        .to_f64()
    }

    /// The stored bounds are the box's bounds, not a centre they are measured
    /// from: `Rectangle` reports back exactly what the config named.
    #[test]
    fn test_box_bounds_are_stored_verbatim() {
        let shape = config().find_aoi("scotland").unwrap().build().unwrap();
        let AoiShape::Rectangle(rect) = &shape else {
            panic!("expected a rectangle");
        };
        // The bounds round-trip through radians, so compare to a tolerance.
        assert_approx_eq!(rect.latitudes().0.to_f64(), 54.0, 1e-12);
        assert_approx_eq!(rect.latitudes().1.to_f64(), 60.0, 1e-12);
        // `longitudes()` reports the west bound and the eastward span.
        assert_approx_eq!(rect.longitudes().0.to_f64(), -8.0, 1e-12);
        assert_approx_eq!(rect.longitudes().1.to_f64(), 7.0, 1e-12);

        assert!(offset(&shape, 57.0, -4.5) > 0.0);
        assert!(offset(&shape, 61.0, -4.5) < 0.0);
    }

    /// An `east` west of `west` wraps the antimeridian rather than erroring.
    #[test]
    fn test_box_wraps_the_antimeridian() {
        let config: config::Config = serde_yaml::from_str(
            r"
aois:
  pacific:
    shape: box
    south: -20.0
    north: 20.0
    west: 160.0
    east: -160.0
",
        )
        .unwrap();
        let shape = config.find_aoi("pacific").unwrap().build().unwrap();
        assert!(offset(&shape, 0.0, 180.0) > 0.0);
        assert!(offset(&shape, 0.0, 0.0) < 0.0);
    }

    #[test]
    fn test_every_shape_builds() {
        let config = config();
        for id in ["scotland", "north-sea", "cape-town", "corridor"] {
            let shape = config.find_aoi(id).unwrap().build().unwrap();
            // The stored centre is inside each of the non-polygon shapes.
            match id {
                "north-sea" => assert!(offset(&shape, 56.0, 2.0) > 0.0),
                "cape-town" => assert!(offset(&shape, -33.9, 18.4) > 0.0),
                "corridor" => assert!(offset(&shape, 56.0, -4.0) > 0.0),
                _ => {}
            }
        }
    }

    #[test]
    fn test_bearing_defaults_to_zero() {
        let config: config::Config = serde_yaml::from_str(
            r"
aois:
  plain:
    shape: ellipse
    latitude: 0.0
    longitude: 0.0
    semi_axis_a: 10.0
    semi_axis_b: 2.0
",
        )
        .unwrap();
        let AoiDef::Ellipse(e) = config.find_aoi("plain").unwrap() else {
            panic!("expected an ellipse");
        };
        assert_eq!(e.bearing, 0.0);
        // Bearing 0 points the major axis at the pole.
        let shape = config.find_aoi("plain").unwrap().build().unwrap();
        assert!(offset(&shape, 8.0, 0.0) > 0.0);
        assert!(offset(&shape, 0.0, 8.0) < 0.0);
    }

    /// The listing uses the config file's own field names, so a `describe`
    /// line and the YAML it came from name the same things.
    #[test]
    fn test_describe_uses_the_config_field_names() {
        let config = config();
        let describe = |id: &str| config.find_aoi(id).unwrap().describe();

        assert_eq!(describe("scotland"), "south=54 north=60 west=-8 east=-1");
        assert_eq!(
            describe("north-sea"),
            "latitude=56 longitude=2 semi_axis_a=2.7 semi_axis_b=1.1 bearing=45"
        );
        assert_eq!(
            describe("cape-town"),
            "latitude=-33.9 longitude=18.4 radius=2.25"
        );
        assert_eq!(describe("corridor"), "(54, -8) (54, -1) (60, -1)");
    }

    /// Every shape survives a save/load cycle unchanged, so `aoi add` writes
    /// something the prediction commands can read back.
    #[test]
    fn test_aois_round_trip_through_yaml() {
        let original = config();
        let yaml = serde_yaml::to_string(&original).unwrap();
        let reloaded: config::Config = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(reloaded.aoi_ids(), original.aoi_ids());
        for id in original.aoi_ids() {
            let (before, after) = (
                original.find_aoi(id).unwrap(),
                reloaded.find_aoi(id).unwrap(),
            );
            assert_eq!(before.kind(), after.kind(), "{id} changed shape");
            assert_eq!(before.describe(), after.describe(), "{id} changed");
        }
    }

    #[test]
    fn test_validate_rejects_missing_aoi() {
        let err = args(None).validate(&config()).unwrap_err().to_string();
        assert!(err.contains("--aoi is required"), "{err}");
        assert!(
            err.contains("cape-town, corridor, north-sea, scotland"),
            "{err}"
        );
    }

    #[test]
    fn test_validate_rejects_unknown_aoi() {
        let err = args(Some("nowhere"))
            .validate(&config())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown aoi 'nowhere'"), "{err}");
        assert!(err.contains("known ids: cape-town"), "{err}");
    }

    #[test]
    fn test_validate_missing_aoi_with_empty_config() {
        let err = args(None)
            .validate(&config::Config::default())
            .unwrap_err()
            .to_string();
        assert_eq!(err, "--aoi is required");
    }

    /// A hand-edited AOI that no longer builds must name itself in the error.
    #[test]
    fn test_validate_reports_an_unbuildable_aoi() {
        let config: config::Config = serde_yaml::from_str(
            r"
aois:
  broken:
    shape: ellipse
    latitude: 0.0
    longitude: 0.0
    semi_axis_a: 95.0
    semi_axis_b: 5.0
",
        )
        .unwrap();
        let err = args(Some("broken")).validate(&config).unwrap_err();
        assert!(err.to_string().contains("aoi 'broken'"), "{err}");
        assert!(format!("{err:#}").contains("semi_axis_a 95"), "{err:#}");
        // find_aoi still works, so `aoi remove` can delete it.
        assert!(config.find_aoi("broken").is_ok());
    }
}
