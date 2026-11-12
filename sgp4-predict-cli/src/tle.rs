use anyhow::Context as _;
use sgp4_predict::Tle;
use std::io::{IsTerminal as _, Read as _};

pub fn parse_tle_file(path: &std::path::Path) -> anyhow::Result<Tle> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read TLE file {}", path.display()))?;
    content
        .parse()
        .with_context(|| format!("in TLE file {}", path.display()))
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
    content.parse().context("in TLE from stdin")
}
