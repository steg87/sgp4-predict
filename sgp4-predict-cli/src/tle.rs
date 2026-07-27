use anyhow::Context as _;
use sgp4_predict::Tle;
use std::io::{IsTerminal as _, Read as _};

pub fn parse_tle_file(path: &std::path::Path) -> anyhow::Result<Tle> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read TLE file {}", path.display()))?;
    parse_tle(&content).with_context(|| format!("in TLE file {}", path.display()))
}

/// Read a TLE from stdin, consuming it whole — piped or typed.
pub fn read_tle_stdin() -> anyhow::Result<Tle> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        let eof = if cfg!(windows) {
            "Ctrl-Z then Enter"
        } else {
            "Ctrl-D"
        };
        // Hint goes to stderr so it never contaminates piped output.
        eprintln!("Paste TLE to stdin; {eof} when done:");
    }

    let mut content = String::new();
    stdin
        .lock()
        .read_to_string(&mut content)
        .context("failed to read TLE from stdin")?;
    parse_tle(&content).context("in TLE from stdin")
}

/// Parse 2- or 3-line TLE text. A missing name line is filled from the NORAD id.
fn parse_tle(content: &str) -> anyhow::Result<Tle> {
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    match lines.as_slice() {
        [name, line1, line2] => Ok(Tle::new(*name, *line1, *line2)),
        [line1, line2] => Ok(Tle::new(derive_name(line1), *line1, *line2)),
        _ => anyhow::bail!(
            "expected 2 or 3 non-empty lines (optional name, line 1, line 2), found {}",
            lines.len()
        ),
    }
}

fn derive_name(line1: &str) -> String {
    let norad_id = line1.get(2..7).unwrap_or("").trim();
    if norad_id.is_empty() {
        "Unknown".to_string()
    } else {
        format!("NORAD-{norad_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE1: &str = "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990";
    const LINE2: &str = "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740";

    #[test]
    fn test_parses_three_line_tle() {
        let tle = parse_tle(&format!("SENTINEL-2C\n{LINE1}\n{LINE2}\n")).unwrap();
        assert_eq!(tle.satellite_name, "SENTINEL-2C");
        assert_eq!(tle.line_1, LINE1);
        assert_eq!(tle.line_2, LINE2);
    }

    #[test]
    fn test_derives_name_from_norad_id() {
        let tle = parse_tle(&format!("{LINE1}\n{LINE2}\n")).unwrap();
        assert_eq!(tle.satellite_name, "NORAD-60989");
    }

    #[test]
    fn test_ignores_blank_lines_and_surrounding_whitespace() {
        let tle = parse_tle(&format!("\n  SENTINEL-2C  \n\n  {LINE1}  \n{LINE2}\n\n")).unwrap();
        assert_eq!(tle.satellite_name, "SENTINEL-2C");
        assert_eq!(tle.line_1, LINE1);
    }

    #[test]
    fn test_rejects_wrong_line_count() {
        let err = parse_tle(LINE1).unwrap_err().to_string();
        assert!(err.contains("found 1"), "{err}");

        let err = parse_tle("").unwrap_err().to_string();
        assert!(err.contains("found 0"), "{err}");

        let err = parse_tle(&format!("a\nb\n{LINE1}\n{LINE2}"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("found 4"), "{err}");
    }
}
