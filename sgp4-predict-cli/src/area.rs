//! Turning a config [`AreaDef`] into a library area, and resolving `--area`.
//!
//! Mirrors `observer.rs`: `validate` enforces that the flag names a usable
//! entry, `resolve` returns the built value.

use anyhow::Context as _;
use std::path::Path;

use crate::{
    cli::AreaArgs,
    config::{self, AreaDef},
};
use sgp4_predict::{Degrees, Ellipse, Polygon, Rectangle};

/// A built area, ready to hand to `aoi_iter`.
///
/// `Predictor::aoi_iter` is generic over one `Area`, so the three shapes need
/// a single type to travel in. There is deliberately no `impl Area for
/// AreaShape`: callers match once and pass the concrete shape to a generic
/// function, which keeps the dispatch at the call site instead of on every
/// sample.
pub enum AreaShape {
    Rectangle(Rectangle),
    Ellipse(Ellipse),
    Polygon(Polygon),
}

impl AreaDef {
    /// Build the library shape, validating the geometry.
    pub fn build(&self) -> anyhow::Result<AreaShape> {
        Ok(match self {
            // Stored as a centre and extents, so the corners are derived here.
            // A latitude bound past a pole is rejected by `Rectangle::new`.
            AreaDef::Box(b) => AreaShape::Rectangle(Rectangle::new(
                (
                    Degrees(b.latitude - b.height / 2.0),
                    Degrees(b.longitude - b.width / 2.0),
                ),
                (
                    Degrees(b.latitude + b.height / 2.0),
                    Degrees(b.longitude + b.width / 2.0),
                ),
            )?),
            AreaDef::Ellipse(e) => AreaShape::Ellipse(Ellipse::new(
                (Degrees(e.latitude), Degrees(e.longitude)),
                Degrees(e.semi_major),
                Degrees(e.semi_minor),
                Degrees(e.bearing),
            )?),
            AreaDef::Circle(c) => AreaShape::Ellipse(Ellipse::circle(
                (Degrees(c.latitude), Degrees(c.longitude)),
                Degrees(c.radius),
            )?),
            AreaDef::Polygon(p) => AreaShape::Polygon(Polygon::new(
                p.vertices
                    .iter()
                    .map(|v| (Degrees(v.latitude), Degrees(v.longitude))),
            )?),
        })
    }

    /// The shape's name, as written in the config file.
    pub fn kind(&self) -> &'static str {
        match self {
            AreaDef::Box(_) => "box",
            AreaDef::Ellipse(_) => "ellipse",
            AreaDef::Circle(_) => "circle",
            AreaDef::Polygon(_) => "polygon",
        }
    }

    /// One-line rendering for `aoi list`, `aoi remove` and `--output-args`.
    ///
    /// Uses the config file's own field names rather than the flag's
    /// positional syntax, so a listing reads the same way the YAML does.
    pub fn describe(&self) -> String {
        match self {
            AreaDef::Box(b) => format!(
                "latitude={} longitude={} width={} height={}",
                b.latitude, b.longitude, b.width, b.height
            ),
            AreaDef::Ellipse(e) => format!(
                "latitude={} longitude={} semi_major={} semi_minor={} bearing={}",
                e.latitude, e.longitude, e.semi_major, e.semi_minor, e.bearing
            ),
            AreaDef::Circle(c) => format!(
                "latitude={} longitude={} radius={}",
                c.latitude, c.longitude, c.radius
            ),
            AreaDef::Polygon(p) => p
                .vertices
                .iter()
                .map(|v| format!("({}, {})", v.latitude, v.longitude))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

impl AreaArgs {
    /// Enforce that `--area` is given and names an area the config can build.
    ///
    /// Returns the resolved id, so callers do not have to unwrap it again.
    pub fn validate<'a>(&'a self, config: &config::Config) -> anyhow::Result<&'a str> {
        let id = self
            .area
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--area is required{}", config.area_ids_hint()))?;
        config
            .find_area(id)?
            .build()
            .with_context(|| format!("area '{id}'"))?;
        Ok(id)
    }

    /// Validate the flags and build the named area.
    pub fn resolve(&self, config_path: Option<&Path>) -> anyhow::Result<(AreaDef, AreaShape)> {
        let mut config = config::load(config_path)?;
        let id = self.validate(&config)?;
        let def = config.areas.remove(id).expect("validate checked the id");
        let shape = def.build().expect("validate already built it");
        Ok((def, shape))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_approx_eq::assert_approx_eq;
    use sgp4_predict::Area as _;

    fn args(area: Option<&str>) -> AreaArgs {
        AreaArgs {
            area: area.map(str::to_owned),
        }
    }

    fn config() -> config::Config {
        serde_yaml::from_str(
            r"
areas:
  scotland:
    shape: box
    latitude: 57.0
    longitude: -4.5
    width: 7.0
    height: 6.0
  north-sea:
    shape: ellipse
    latitude: 56.0
    longitude: 2.0
    semi_major: 2.7
    semi_minor: 1.1
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

    fn offset(shape: &AreaShape, lat: f64, lon: f64) -> f64 {
        let point = (Degrees(lat), Degrees(lon)).into();
        match shape {
            AreaShape::Rectangle(a) => a.signed_angular_offset(point),
            AreaShape::Ellipse(a) => a.signed_angular_offset(point),
            AreaShape::Polygon(a) => a.signed_angular_offset(point),
        }
        .to_f64()
    }

    #[test]
    fn test_box_is_centred_on_its_coordinates() {
        let shape = config().find_area("scotland").unwrap().build().unwrap();
        let AreaShape::Rectangle(rect) = &shape else {
            panic!("expected a rectangle");
        };
        // Centre 57,-4.5 with extents 7x6 spans 54..60 by -8..-1. The bounds
        // round-trip through radians, so compare to a tolerance.
        assert_approx_eq!(rect.latitudes().0.to_f64(), 54.0, 1e-12);
        assert_approx_eq!(rect.latitudes().1.to_f64(), 60.0, 1e-12);
        assert_approx_eq!(rect.longitudes().0.to_f64(), -8.0, 1e-12);
        assert_approx_eq!(rect.longitudes().1.to_f64(), 7.0, 1e-12);

        assert!(offset(&shape, 57.0, -4.5) > 0.0);
        assert!(offset(&shape, 61.0, -4.5) < 0.0);
    }

    #[test]
    fn test_every_shape_builds() {
        let config = config();
        for id in ["scotland", "north-sea", "cape-town", "corridor"] {
            let shape = config.find_area(id).unwrap().build().unwrap();
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
areas:
  plain:
    shape: ellipse
    latitude: 0.0
    longitude: 0.0
    semi_major: 10.0
    semi_minor: 2.0
",
        )
        .unwrap();
        let AreaDef::Ellipse(e) = config.find_area("plain").unwrap() else {
            panic!("expected an ellipse");
        };
        assert_eq!(e.bearing, 0.0);
        // Bearing 0 points the major axis at the pole.
        let shape = config.find_area("plain").unwrap().build().unwrap();
        assert!(offset(&shape, 8.0, 0.0) > 0.0);
        assert!(offset(&shape, 0.0, 8.0) < 0.0);
    }

    /// The listing uses the config file's own field names, so a `describe`
    /// line and the YAML it came from name the same things.
    #[test]
    fn test_describe_uses_the_config_field_names() {
        let config = config();
        let describe = |id: &str| config.find_area(id).unwrap().describe();

        assert_eq!(
            describe("scotland"),
            "latitude=57 longitude=-4.5 width=7 height=6"
        );
        assert_eq!(
            describe("north-sea"),
            "latitude=56 longitude=2 semi_major=2.7 semi_minor=1.1 bearing=45"
        );
        assert_eq!(
            describe("cape-town"),
            "latitude=-33.9 longitude=18.4 radius=2.25"
        );
        assert_eq!(describe("corridor"), "(54, -8) (54, -1) (60, -1)");
    }

    /// Every shape survives a save/load cycle unchanged, so `area add` writes
    /// something the prediction commands can read back.
    #[test]
    fn test_areas_round_trip_through_yaml() {
        let original = config();
        let yaml = serde_yaml::to_string(&original).unwrap();
        let reloaded: config::Config = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(reloaded.area_ids(), original.area_ids());
        for id in original.area_ids() {
            let (before, after) = (
                original.find_area(id).unwrap(),
                reloaded.find_area(id).unwrap(),
            );
            assert_eq!(before.kind(), after.kind(), "{id} changed shape");
            assert_eq!(before.describe(), after.describe(), "{id} changed");
        }
    }

    #[test]
    fn test_validate_rejects_missing_area() {
        let err = args(None).validate(&config()).unwrap_err().to_string();
        assert!(err.contains("--area is required"), "{err}");
        assert!(
            err.contains("cape-town, corridor, north-sea, scotland"),
            "{err}"
        );
    }

    #[test]
    fn test_validate_rejects_unknown_area() {
        let err = args(Some("nowhere"))
            .validate(&config())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown area 'nowhere'"), "{err}");
        assert!(err.contains("known ids: cape-town"), "{err}");
    }

    #[test]
    fn test_validate_missing_area_with_empty_config() {
        let err = args(None)
            .validate(&config::Config::default())
            .unwrap_err()
            .to_string();
        assert_eq!(err, "--area is required");
    }

    /// A hand-edited area that no longer builds must name itself in the error.
    #[test]
    fn test_validate_reports_an_unbuildable_area() {
        let config: config::Config = serde_yaml::from_str(
            r"
areas:
  broken:
    shape: ellipse
    latitude: 0.0
    longitude: 0.0
    semi_major: 1.0
    semi_minor: 5.0
",
        )
        .unwrap();
        let err = args(Some("broken")).validate(&config).unwrap_err();
        assert!(err.to_string().contains("area 'broken'"), "{err}");
        assert!(format!("{err:#}").contains("semi-minor"), "{err:#}");
        // find_area still works, so `area remove` can delete it.
        assert!(config.find_area("broken").is_ok());
    }
}
