//! Turns the tuning flags into the library's `*Opts` structs.
//!
//! Kept out of `cli.rs`, which holds clap declarations only. Every `build`
//! here fills every field of its target struct, so a knob added to the library
//! fails to compile until it is either exposed or explicitly defaulted.

use anyhow::Context as _;
use std::time::Duration;

use crate::cli::{
    AoiTuningArgs, ApsisTuningArgs, IlluminationTuningArgs, RefinementArgs, TransitTuningArgs,
};
use sgp4_predict::{
    AoiIterOpts, ApsisIterOpts, Degrees, IlluminationIterOpts, MaxElevationOpts, Refinement,
    TransitIterOpts,
};

/// One `# key: value` line of the `--output-args` header.
pub type HeaderPair = (&'static str, String);

/// `humantime::parse_duration` accepts spans no `chrono::Duration` can hold
/// (`--max-step 1000y`), so the conversion is fallible and names the flag.
fn chrono(duration: Duration, flag: &str) -> anyhow::Result<chrono::Duration> {
    chrono::Duration::from_std(duration).with_context(|| format!("--{flag} is out of range"))
}

fn duration_str(duration: Duration) -> String {
    humantime::format_duration(duration).to_string()
}

impl RefinementArgs {
    pub fn build(&self) -> Refinement {
        Refinement {
            time_tolerance: self.time_tolerance,
            max_iter: self.max_iter,
        }
    }

    pub fn header_pairs(&self) -> Vec<HeaderPair> {
        vec![
            ("time-tolerance", self.time_tolerance.to_string()),
            ("max-iter", self.max_iter.to_string()),
        ]
    }
}

impl TransitTuningArgs {
    pub fn build(&self) -> anyhow::Result<TransitIterOpts> {
        Ok(TransitIterOpts {
            min_step: chrono(self.min_step, "min-step")?,
            max_step: chrono(self.max_step, "max-step")?,
            walk_step: chrono(self.walk_step, "walk-step")?,
            max_transit_duration: chrono(self.max_transit_duration, "max-transit-duration")?,
            skip_leading_partial: self.skip_leading_partial,
            clamp_to_interval: self.clamp_to_interval,
        })
    }

    /// The TCA scan, which `transits` runs per transit to find peak elevation.
    pub fn build_max_elevation(&self) -> anyhow::Result<MaxElevationOpts> {
        Ok(MaxElevationOpts {
            scan_step: chrono(self.tca_scan_step, "tca-scan-step")?,
        })
    }

    pub fn header_pairs(&self) -> Vec<HeaderPair> {
        vec![
            ("min-step", duration_str(self.min_step)),
            ("max-step", duration_str(self.max_step)),
            ("walk-step", duration_str(self.walk_step)),
            (
                "max-transit-duration",
                duration_str(self.max_transit_duration),
            ),
            (
                "skip-leading-partial",
                self.skip_leading_partial.to_string(),
            ),
            ("clamp-to-interval", self.clamp_to_interval.to_string()),
            ("tca-scan-step", duration_str(self.tca_scan_step)),
        ]
    }
}

impl AoiTuningArgs {
    pub fn build(&self) -> anyhow::Result<AoiIterOpts> {
        Ok(AoiIterOpts {
            max_off_nadir: Degrees(self.max_off_nadir).into(),
            coverage: self.coverage.into(),
            min_step: chrono(self.min_step, "min-step")?,
            max_step: chrono(self.max_step, "max-step")?,
            walk_step: chrono(self.walk_step, "walk-step")?,
            max_window_duration: chrono(self.max_window_duration, "max-window-duration")?,
            skip_leading_partial: self.skip_leading_partial,
            clamp_to_interval: self.clamp_to_interval,
        })
    }

    pub fn header_pairs(&self) -> Vec<HeaderPair> {
        vec![
            ("max-off-nadir", self.max_off_nadir.to_string()),
            ("coverage", format!("{:?}", self.coverage).to_lowercase()),
            ("min-step", duration_str(self.min_step)),
            ("max-step", duration_str(self.max_step)),
            ("walk-step", duration_str(self.walk_step)),
            (
                "max-window-duration",
                duration_str(self.max_window_duration),
            ),
            (
                "skip-leading-partial",
                self.skip_leading_partial.to_string(),
            ),
            ("clamp-to-interval", self.clamp_to_interval.to_string()),
        ]
    }
}

impl ApsisTuningArgs {
    pub fn build(&self) -> anyhow::Result<ApsisIterOpts> {
        Ok(ApsisIterOpts {
            step: chrono(self.step, "step")?,
        })
    }

    pub fn header_pairs(&self) -> Vec<HeaderPair> {
        vec![("step", duration_str(self.step))]
    }
}

impl IlluminationTuningArgs {
    pub fn build(&self) -> anyhow::Result<IlluminationIterOpts> {
        Ok(IlluminationIterOpts {
            step: chrono(self.step, "step")?,
            walk_step: chrono(self.walk_step, "walk-step")?,
            max_window_duration: chrono(self.max_window_duration, "max-window-duration")?,
        })
    }

    pub fn header_pairs(&self) -> Vec<HeaderPair> {
        vec![
            ("step", duration_str(self.step)),
            ("walk-step", duration_str(self.walk_step)),
            (
                "max-window-duration",
                duration_str(self.max_window_duration),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;

    use crate::cli::{AoiWindowsArgs, ApsidesArgs, IlluminationArgs, TransitsArgs};

    /// Parse a subcommand's args from flags alone, with no TLE or interval.
    fn parse<T: clap::Args>(flags: &[&str]) -> T {
        #[derive(clap::Parser)]
        struct Wrapper<T: clap::Args> {
            #[command(flatten)]
            inner: T,
        }
        let mut argv = vec!["test"];
        argv.extend_from_slice(flags);
        Wrapper::<T>::parse_from(argv).inner
    }

    // The library's `*Opts` do not derive `PartialEq`, so these compare field
    // by field. That is also what makes them fail loudly on a new field: the
    // struct literal in `build` stops compiling first.

    #[test]
    fn test_transit_defaults_match_the_library() {
        let args: TransitsArgs = parse(&[]);
        let (opts, want) = (args.tuning.build().unwrap(), TransitIterOpts::default());
        assert_eq!(opts.min_step, want.min_step);
        assert_eq!(opts.max_step, want.max_step);
        assert_eq!(opts.walk_step, want.walk_step);
        assert_eq!(opts.max_transit_duration, want.max_transit_duration);
        assert_eq!(opts.skip_leading_partial, want.skip_leading_partial);
        assert_eq!(opts.clamp_to_interval, want.clamp_to_interval);

        let tca = args.tuning.build_max_elevation().unwrap();
        assert_eq!(tca.scan_step, MaxElevationOpts::default().scan_step);
    }

    #[test]
    fn test_aoi_defaults_match_the_library() {
        let args: AoiWindowsArgs = parse(&["--aoi", "x"]);
        let (opts, want) = (args.tuning.build().unwrap(), AoiIterOpts::default());
        assert_eq!(opts.max_off_nadir, want.max_off_nadir);
        assert_eq!(opts.coverage, want.coverage);
        assert_eq!(opts.min_step, want.min_step);
        assert_eq!(opts.max_step, want.max_step);
        assert_eq!(opts.walk_step, want.walk_step);
        assert_eq!(opts.max_window_duration, want.max_window_duration);
        assert_eq!(opts.skip_leading_partial, want.skip_leading_partial);
        assert_eq!(opts.clamp_to_interval, want.clamp_to_interval);
    }

    #[test]
    fn test_apsis_defaults_match_the_library() {
        let args: ApsidesArgs = parse(&[]);
        assert_eq!(
            args.tuning.build().unwrap().step,
            ApsisIterOpts::default().step
        );
    }

    #[test]
    fn test_illumination_defaults_match_the_library() {
        let args: IlluminationArgs = parse(&[]);
        let (opts, want) = (
            args.tuning.build().unwrap(),
            IlluminationIterOpts::default(),
        );
        assert_eq!(opts.step, want.step);
        assert_eq!(opts.walk_step, want.walk_step);
        assert_eq!(opts.max_window_duration, want.max_window_duration);
    }

    #[test]
    fn test_refinement_defaults_match_the_library() {
        let args: TransitsArgs = parse(&[]);
        let (r, want) = (args.refinement.build(), Refinement::default());
        assert_eq!(r.time_tolerance, want.time_tolerance);
        assert_eq!(r.max_iter, want.max_iter);
    }

    #[test]
    fn test_flags_override_the_defaults() {
        let args: AoiWindowsArgs = parse(&[
            "--aoi",
            "x",
            "--min-step",
            "250ms",
            "--max-window-duration",
            "6h",
            "--clamp-to-interval",
            "true",
            "--max-off-nadir",
            "30",
            "--coverage",
            "full",
        ]);
        let opts = args.tuning.build().unwrap();
        assert_eq!(opts.max_off_nadir, Degrees(30.0).into());
        assert_eq!(opts.coverage, sgp4_predict::Coverage::Full);
        assert_eq!(opts.min_step, chrono::Duration::milliseconds(250));
        assert_eq!(opts.max_window_duration, chrono::Duration::hours(6));
        assert!(opts.clamp_to_interval);
        // Untouched flags keep the library default.
        assert_eq!(opts.max_step, AoiIterOpts::default().max_step);
    }

    /// The two bools take a value rather than being presence flags, so every
    /// header line can be pasted straight back onto the command line.
    #[test]
    fn test_bool_flags_take_a_value() {
        let args: AoiWindowsArgs = parse(&["--aoi", "x", "--skip-leading-partial", "false"]);
        assert!(!args.tuning.build().unwrap().skip_leading_partial);
        assert!(
            args.tuning
                .header_pairs()
                .contains(&("skip-leading-partial", "false".to_string()))
        );
    }

    #[test]
    fn test_header_pairs_render_durations_the_way_they_are_typed() {
        let args: AoiWindowsArgs = parse(&["--aoi", "x"]);
        let pairs = args.tuning.header_pairs();
        assert!(
            pairs.contains(&("max-step", "10m".to_string())),
            "{pairs:?}"
        );
        assert!(
            pairs.contains(&("max-window-duration", "1h".to_string())),
            "{pairs:?}"
        );
    }

    /// `chrono::Duration` tops out around 292 million years, well above any
    /// span humantime rejects, so the flag has to be named in the error.
    #[test]
    fn test_out_of_range_duration_is_reported_against_its_flag() {
        let args: AoiWindowsArgs = parse(&["--aoi", "x", "--max-step", "1000000000y"]);
        let err = args.tuning.build().unwrap_err().to_string();
        assert!(err.contains("--max-step is out of range"), "{err}");
    }
}
