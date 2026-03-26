use anyhow::Context as _;
use sgp4_predict::Tle;
use std::io::{BufRead as _, Write as _};

pub fn parse_tle_file(path: &std::path::Path) -> anyhow::Result<Tle> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read TLE file {}", path.display()))?;
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    match lines.as_slice() {
        [name, line1, line2] => Ok(Tle::new(*name, *line1, *line2)),
        [line1, line2] => {
            let norad_id = line1.get(2..7).unwrap_or("").trim();
            let name = if norad_id.is_empty() {
                "Unknown".to_string()
            } else {
                format!("NORAD-{norad_id}")
            };
            Ok(Tle::new(name, *line1, *line2))
        }
        _ => anyhow::bail!(
            "TLE file must contain 2 or 3 non-empty lines, found {}",
            lines.len()
        ),
    }
}

pub fn prompt_tle() -> anyhow::Result<Tle> {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    let name_input = prompt_line(&mut lines, "Satellite name (leave blank to skip): ")?;
    let line1 = prompt_line(&mut lines, "TLE line 1: ")?;
    let line2 = prompt_line(&mut lines, "TLE line 2: ")?;

    let name = if name_input.is_empty() {
        let norad_id = line1.get(2..7).unwrap_or("").trim().to_string();
        if norad_id.is_empty() {
            "Unknown".to_string()
        } else {
            format!("NORAD-{norad_id}")
        }
    } else {
        name_input
    };

    Ok(Tle::new(name, line1, line2))
}

fn prompt_line(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    prompt: &str,
) -> anyhow::Result<String> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("unexpected EOF"))?
        .context("reading stdin")
        .map(|s| s.trim().to_string())
}
