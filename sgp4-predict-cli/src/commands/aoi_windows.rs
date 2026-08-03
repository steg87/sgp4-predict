use anyhow::Context as _;
use std::path::Path;

use super::{Context, effective_config_path, prepare};
use crate::{area::AreaShape, cli::AoiWindowsArgs, output};

pub fn run(args: AoiWindowsArgs, config_path: Option<&Path>) -> anyhow::Result<()> {
    // Resolve the area first: an unknown --area must fail before the user is
    // asked for a TLE on stdin.
    let (def, shape) = args.area.resolve(config_path)?;
    let mut ctx = prepare(&args.common)?;

    let area_id = args.area.area.as_deref().expect("resolve requires --area");
    let definition = def.describe();
    let config = effective_config_path(config_path);
    ctx.write_args_header(
        "aoi-windows",
        &args.common,
        config.as_deref(),
        &[
            ("area", area_id),
            ("area-shape", def.kind()),
            ("area-definition", &definition),
        ],
    )?;

    // Matched once here rather than behind a trait object, so the per-sample
    // geometry call stays static.
    match &shape {
        AreaShape::Rectangle(area) => windows(ctx, area),
        AreaShape::Ellipse(area) => windows(ctx, area),
        AreaShape::Polygon(area) => windows(ctx, area),
    }
}

fn windows(mut ctx: Context, area: &impl sgp4_predict::Area) -> anyhow::Result<()> {
    let predictor = &ctx.predictor;
    let rows = predictor
        .aoi_iter(area, ctx.interval.clone())
        .map(|result| {
            let window = result.context("area detection error")?;
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
