use anyhow::Context as _;
use sgp4_predict::Observer;
use std::io::{BufRead as _, Write as _};

pub struct GroundObserver {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_m: f64,
}

impl Observer for GroundObserver {
    fn latitude_deg(&self) -> f64 {
        self.lat_deg
    }

    fn longitude_deg(&self) -> f64 {
        self.lon_deg
    }

    fn altitude(&self) -> f64 {
        self.alt_m
    }
}

pub fn parse_observer(s: &str) -> anyhow::Result<GroundObserver> {
    let parts: Vec<&str> = s.split(',').collect();
    match parts.as_slice() {
        [lat, lon, alt] => Ok(GroundObserver {
            lat_deg: lat
                .trim()
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("invalid latitude: {lat}"))?,
            lon_deg: lon
                .trim()
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("invalid longitude: {lon}"))?,
            alt_m: alt
                .trim()
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("invalid altitude: {alt}"))?,
        }),
        _ => anyhow::bail!("observer must be 'lat,lon,alt' — got: {s}"),
    }
}

pub fn prompt_observer() -> anyhow::Result<GroundObserver> {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    let lat_deg = prompt_f64(&mut lines, "Observer latitude (degrees): ")?;
    let lon_deg = prompt_f64(&mut lines, "Observer longitude (degrees): ")?;
    let alt_m = prompt_f64(&mut lines, "Observer altitude (metres): ")?;

    Ok(GroundObserver {
        lat_deg,
        lon_deg,
        alt_m,
    })
}

fn prompt_f64(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    prompt: &str,
) -> anyhow::Result<f64> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    let s = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?
        .context("reading stdin")?;
    s.trim()
        .parse::<f64>()
        .map_err(|e| anyhow::anyhow!("expected a number: {e}"))
}
