use anyhow::Context as _;
use std::path::Path;

use super::{Context, effective_config_path, prepare};
use crate::{aoi::AoiShape, cli::AoiWindowsArgs, output};

pub fn run(args: AoiWindowsArgs, config_path: Option<&Path>) -> anyhow::Result<()> {
    // Resolve the AOI first: an unknown --aoi must fail before the user is
    // asked for a TLE on stdin.
    let (def, shape) = args.aoi.resolve(config_path)?;
    let mut ctx = prepare(&args.common)?;

    let aoi_id = args.aoi.id.as_deref().expect("resolve requires --aoi");
    let definition = def.describe();
    let config = effective_config_path(config_path);
    ctx.write_args_header(
        "aoi-windows",
        &args.common,
        config.as_deref(),
        &[
            ("aoi", aoi_id),
            ("aoi-shape", def.kind()),
            ("aoi-definition", &definition),
        ],
    )?;

    // Matched once here rather than behind a trait object, so the per-sample
    // geometry call stays static.
    match &shape {
        AoiShape::Rectangle(aoi) => windows(ctx, aoi),
        AoiShape::Ellipse(aoi) => windows(ctx, aoi),
        AoiShape::Polygon(aoi) => windows(ctx, aoi),
    }
}

fn windows(mut ctx: Context, aoi: &impl sgp4_predict::Area) -> anyhow::Result<()> {
    let predictor = &ctx.predictor;
    let rows = predictor.aoi_iter(aoi, ctx.interval.clone()).map(|result| {
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
