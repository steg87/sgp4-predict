use anyhow::Context as _;
use sgp4_predict::{AoiIterOpts, Refinement};
use std::path::Path;

use super::{Context, pairs, prepare};
use crate::{aoi::AoiShape, cli::AoiWindowsArgs, output};

pub fn run(args: AoiWindowsArgs, config_path: Option<&Path>) -> anyhow::Result<()> {
    // Resolve the AOI first: an unknown --aoi must fail before the user is
    // asked for a TLE on stdin.
    let (def, shape) = args.aoi.resolve(config_path)?;
    let opts = args.tuning.build()?;
    let refinement = args.refinement.build();
    let mut ctx = prepare(&args.common)?;

    let aoi_id = args.aoi.id.as_deref().expect("resolve requires --aoi");
    let definition = def.describe();
    let tuning = args.tuning.header_pairs();
    let refinement_pairs = args.refinement.header_pairs();

    let mut extra = vec![
        ("aoi", aoi_id),
        ("aoi-shape", def.kind()),
        ("aoi-definition", definition.as_str()),
    ];
    extra.extend(pairs(&[&tuning, &refinement_pairs]));
    ctx.write_args_header("aoi-windows", &args.common, &extra)?;

    // Matched once here rather than behind a trait object, so the per-sample
    // geometry call stays static.
    match &shape {
        AoiShape::Rectangle(aoi) => windows(ctx, aoi, opts, refinement),
        AoiShape::Ellipse(aoi) => windows(ctx, aoi, opts, refinement),
        AoiShape::Polygon(aoi) => windows(ctx, aoi, opts, refinement),
    }
}

fn windows(
    mut ctx: Context,
    aoi: &impl sgp4_predict::Area,
    opts: AoiIterOpts,
    refinement: Refinement,
) -> anyhow::Result<()> {
    let predictor = &ctx.predictor;
    let rows = predictor
        .aoi_iter_with_opts(aoi, ctx.interval.clone(), opts, refinement)
        .map(|result| {
            let window = result.context("AOI detection error")?;
            let entry = predictor
                .sub_point(window.start)
                .context("entry sub-point error")?;
            let exit = predictor
                .sub_point(window.end)
                .context("exit sub-point error")?;
            Ok((window, entry, exit))
        });

    output::write_aoi(&mut ctx.writer, ctx.format, rows)
}
