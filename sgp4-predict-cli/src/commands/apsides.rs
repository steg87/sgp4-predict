use anyhow::Context as _;
use chrono::{DateTime, Utc};
use sgp4_predict::Predictor;
use std::io::BufWriter;

use crate::{cli::ApsidesArgs, output, tle};

pub fn run(args: ApsidesArgs) -> anyhow::Result<()> {
    let start: DateTime<Utc> = match &args.common.start {
        Some(s) => {
            let st = humantime::parse_rfc3339_weak(s)
                .map_err(|e| anyhow::anyhow!("invalid start time {s:?}: {e}"))?;
            DateTime::<Utc>::from(st)
        }
        None => Utc::now(),
    };

    let chrono_duration =
        chrono::Duration::from_std(args.common.duration).context("duration out of range")?;
    let interval = start..start + chrono_duration;

    let sat = match &args.common.tle_file {
        Some(p) => tle::parse_tle_file(p)?,
        None => tle::prompt_tle()?,
    };

    let predictor = Predictor::new(&sat)?;
    let age_days = predictor.tle_age(start).num_days();
    tracing::info!(tle_age_days = age_days, "predictor ready");
    if age_days > 7 {
        eprintln!("warning: TLE is {age_days} days old; SGP4 accuracy may be degraded");
    }

    let writer: Box<dyn std::io::Write> = match &args.common.out {
        Some(path) => Box::new(BufWriter::new(
            std::fs::File::create(path)
                .with_context(|| format!("failed to create {}", path.display()))?,
        )),
        None => Box::new(BufWriter::new(std::io::stdout())),
    };

    output::write_apsides(writer, predictor.apsis_iter(interval))
}
