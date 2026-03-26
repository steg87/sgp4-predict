use sgp4_predict::Predictor;

use super::{load_tle, open_writer, resolve_interval, warn_stale_tle, write_args_header};
use crate::{cli::ApsidesArgs, output};

pub fn run(args: ApsidesArgs) -> anyhow::Result<()> {
    let (start, interval) = resolve_interval(&args.common)?;
    let tle = load_tle(&args.common)?;
    let predictor = Predictor::new(&tle)?;
    warn_stale_tle(&predictor, start);
    let mut writer = open_writer(&args.common.out)?;

    if args.common.output_args {
        let start_str = start.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let duration_str = humantime::format_duration(args.common.duration).to_string();
        write_args_header(
            &mut *writer,
            &[
                ("command", "apsides"),
                ("satellite", &tle.satellite_name),
                ("tle-line1", &tle.line_1),
                ("tle-line2", &tle.line_2),
                ("start", &start_str),
                ("duration", &duration_str),
            ],
        )?;
    }

    output::write_apsides(writer, predictor.apsis_iter(interval))
}
