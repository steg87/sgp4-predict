use anyhow::Context as _;
use sgp4_predict::{Degrees, GroundObserver};
use std::io::{BufRead as _, Write as _};

fn validate_observer(lat_deg: f64, lon_deg: f64) -> anyhow::Result<()> {
    anyhow::ensure!(
        (-90.0..=90.0).contains(&lat_deg),
        "latitude must be in [-90, 90], got {lat_deg}"
    );
    anyhow::ensure!(
        (-180.0..=180.0).contains(&lon_deg),
        "longitude must be in [-180, 180], got {lon_deg}"
    );
    Ok(())
}

pub fn parse_observer(s: &str) -> anyhow::Result<GroundObserver> {
    let parts: Vec<&str> = s.split(',').collect();
    match parts.as_slice() {
        [lat, lon, alt] => {
            let lat_deg = lat
                .trim()
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("invalid latitude: {lat}"))?;
            let lon_deg = lon
                .trim()
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("invalid longitude: {lon}"))?;
            let alt_m = alt
                .trim()
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("invalid altitude: {alt}"))?;
            validate_observer(lat_deg, lon_deg)?;
            Ok(GroundObserver::new(
                Degrees(lat_deg),
                Degrees(lon_deg),
                alt_m,
            ))
        }
        _ => anyhow::bail!("observer must be 'lat,lon,alt' — got: {s}"),
    }
}

pub fn prompt_observer() -> anyhow::Result<GroundObserver> {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    let lat_deg = prompt_f64(&mut lines, "Observer latitude (degrees): ")?;
    let lon_deg = prompt_f64(&mut lines, "Observer longitude (degrees): ")?;
    let alt_m = prompt_f64(&mut lines, "Observer altitude (metres): ")?;

    validate_observer(lat_deg, lon_deg)?;
    Ok(GroundObserver::new(
        Degrees(lat_deg),
        Degrees(lon_deg),
        alt_m,
    ))
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
