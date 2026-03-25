use anyhow::Context as _;
use chrono::{DateTime, Utc};
use sgp4_predict::Predictor;
use std::io::BufWriter;

use crate::{cli::TransitsArgs, observer, output, tle};

pub fn run(args: TransitsArgs) -> anyhow::Result<()> {
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

    let observer = match &args.common.observer {
        Some(s) => observer::parse_observer(s)?,
        None => observer::prompt_observer()?,
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

    let transits = predictor
        .transits_iter(&observer, interval, args.min_elevation)
        .map(|result| {
            let transit = result.context("transit detection error")?;
            let aos_obs = predictor
                .observe_at(transit.start, &observer)
                .context("AoS observation error")?;
            let los_obs = predictor
                .observe_at(transit.end, &observer)
                .context("LoS observation error")?;
            let (_, tca_obs) = predictor
                .max_elevation(transit, &observer)
                .context("TCA error")?;
            Ok((transit, aos_obs, los_obs, tca_obs))
        });

    output::write_transits(writer, transits)
}
