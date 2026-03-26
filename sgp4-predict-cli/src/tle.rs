use anyhow::Context as _;
use sgp4_predict::{HasId, HasTle};
use std::io::{BufRead as _, Write as _};

pub struct TleSat {
    pub name: String,
    pub line1: String,
    pub line2: String,
}

impl HasId for TleSat {
    fn id(&self) -> &str {
        &self.name
    }
}

impl HasTle for TleSat {
    fn line_1(&self) -> &str {
        &self.line1
    }

    fn line_2(&self) -> &str {
        &self.line2
    }
}

pub fn parse_tle_file(path: &std::path::Path) -> anyhow::Result<TleSat> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read TLE file {}", path.display()))?;
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    match lines.as_slice() {
        [name, line1, line2] => Ok(TleSat {
            name: name.to_string(),
            line1: line1.to_string(),
            line2: line2.to_string(),
        }),
        [line1, line2] => {
            let norad_id = line1.get(2..7).unwrap_or("").trim();
            let name = if norad_id.is_empty() {
                String::new()
            } else {
                format!("NORAD-{norad_id}")
            };
            Ok(TleSat {
                name,
                line1: line1.to_string(),
                line2: line2.to_string(),
            })
        }
        _ => anyhow::bail!(
            "TLE file must contain 2 or 3 non-empty lines, found {}",
            lines.len()
        ),
    }
}

pub fn prompt_tle() -> anyhow::Result<TleSat> {
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

    Ok(TleSat { name, line1, line2 })
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
