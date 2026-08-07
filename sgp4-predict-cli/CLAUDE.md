# CLI (`sgp4-predict-cli/`)

The `sgp4-predict` binary. `cli.rs` holds clap declarations only; logic lives in sibling modules. Errors are `anyhow`.

## Structure

Each subcommand's `Args` flattens `CommonArgs`; the observer-taking ones (`observations`, `transits`) also flatten `ObserverArgs`. `--config`, `--verbose` and `--quiet` are `global = true`, so they may appear on either side of the subcommand.

`commands::prepare()` builds the shared `Context` (interval, TLE, predictor, writer, format) — add new subcommands through it rather than repeating the sequence. Its ordering is deliberate: `--output-args`/format compatibility is checked first, then the TLE is loaded *before* the writer is opened, so a bad TLE leaves no empty `--out` file behind.

`main.rs` returns `ExitCode`, not `anyhow::Result`: a broken pipe (`… | head`) exits 141 silently, so piping is not reported as failure. Warnings go through `tracing` to **stderr**, never stdout.

## Tuning flags (`tuning.rs`)

Maps the detection-tuning flags onto the library's `*Opts` and `Refinement`; a separate module because `cli.rs` holds clap declarations only. Every `build()` writes a full struct literal rather than `..Default::default()`, so a knob added to the library breaks the build until it is either exposed or deliberately defaulted — that is the mechanism keeping the CLI in step with the library, and `src/tuning.rs`'s unit tests pin each default against the library's own `Default`.

The bool knobs (`--skip-leading-partial`, `--clamp-to-interval`) **take a value** rather than being presence flags: presence flags are always false-by-default, so a default-*true* knob would have to be spelled `--no-skip-leading-partial`, and the `--output-args` line would then name a flag that does not exist. Taking a value keeps every header line pasteable back onto the command line.

The caps use `parse_positive_duration` rather than `parse_step` only for the error wording; both reject zero, which would otherwise reject every window or hang the scan.

## Output (`output.rs`)

Column-driven: each `write_*` declares a `&[Column]` and emits `Cell::Str`/`Cell::Num` rows, which `RowWriter` renders as text, NDJSON, or CSV. Adding a format means adding a `Format` variant and a match arm, not touching the five commands. The text underline is derived from the rendered header (`"-".repeat(header.chars().count())`) — do not reintroduce hand-computed widths.

`--output-args` records *every* resolved knob, not just overridden ones, so the header has a fixed shape and fully reproduces a run. It is rejected for JSON/CSV, where `#` lines would make the output unparseable. Commands bind their `header_pairs()` to a local before calling `commands::pairs()`, which borrows from them.

Every recorded line is an *input* that changes the output. Deliberately absent: `format`, `out`, `tle-source` (the TLE is on the two lines above) and `config` (its values are already recorded literally). Do not re-add them — the last two also leak a local path into a file that gets shared.

## Config data is never a CLI coordinate

Ground locations and areas of interest come from the config file: `--gs <id>` names an entry in `groundstations`, `aoi-windows --aoi <id>` names one in `aois`. There is deliberately no inline `--observer "lat,lon,alt"`, and no `--box`/`--circle` anywhere — not on the prediction commands, not on `aoi add`. Both inline forms existed and were removed; do not reintroduce them. Config data is entered at a prompt or written into the YAML by hand.

**Naming split** (easy to "tidy" wrongly): `aoi` is the *management* group (`aoi add|remove|list`, mirroring `gs`); `aoi-windows` is the *prediction* command (mirroring `transits`). The stored data is an **aoi** everywhere — `aois:`, `--aoi`, `AoiDef`, `AoiShape`, `src/aoi.rs`, `Config::find_aoi`. The library's term stays `Area`, so `AoiShape::Rectangle(aoi) => windows(ctx, aoi)` is the boundary, not an inconsistency. Prose spells out "area of interest" where it is the acronym's expansion, and uses "AOI" as the noun.

`commands/aoi_windows.rs` matches on `AoiShape` once and calls a generic `windows(ctx, aoi: &impl Area)` rather than giving `AoiShape` an `impl Area` — dispatching at the call site keeps the per-sample geometry call static. (The Python bindings solve this the other way; a pyclass cannot be monomorphised per shape.)

## Config file (`config.rs`)

Schema: `groundstations: {id: {location: {latitude, longitude, altitude}}}`; `altitude` defaults to 0, and every struct is `deny_unknown_fields` so typos error rather than being silently dropped. The path is `--config`, else `dirs::home_dir().join(".sgp4-predict").join("config.yaml")` — one expression covering both Unix and Windows, so keep new path handling `PathBuf`-based rather than string-formatted.

**Creation is deliberately asymmetric.** A missing file at the *default* path is created and seeded with `TEMPLATE` — the user never named it, so it cannot be a typo. A missing `--config` path is an **error** everywhere except `gs add`: creating it would let a mistyped path succeed against a fresh empty config while the real stations sit unread, and the resulting `unknown ground station` error would point at the wrong file. Do not "simplify" this into one rule.

Two entry points: `load()` for the prediction commands; `open_for_edit(path, Missing)` for the `gs` commands, which never seeds (`gs add` passes `Missing::Create`, `gs list`/`gs remove` pass `Missing::Reject`; the reject applies only to an explicit path). Both propagate parse errors, so a broken config is never silently overwritten. `Config::save()` writes to a sibling `.yaml.tmp` and renames, so a failed write cannot truncate an existing config; it re-emits a fixed header because **serialising drops YAML comments** — the known cost of `gs add`/`gs remove` on a hand-annotated file.

`AoiDef` is internally tagged on `shape`. Not a style choice: **serde_yaml 0.9 serialises an externally tagged enum as a `!Box` YAML tag** rather than a nested map, and refuses the map form on the way back in, so `{shape: box, south: …}` is the only flat representation that round-trips. `PolygonDef` wraps its `Vec<Vertex>` in a struct for the same reason — an internal tag has nowhere to live on a bare sequence.

**`BoxDef` stores the four bounds (`south`/`north`/`west`/`east`), not a centre with extents**, so it is the same two corners `Rectangle::new` takes and `build()` is a rename rather than a calculation. Nothing is derived, so a bad value is always attributable to the field it was typed into, and the library's `Error::Latitude`/`EmptyRectangle` messages land on real config field names. An earlier centre-plus-extents form was replaced for exactly this; do not reintroduce it.

`AoiDef::build()` is where geometry is validated, so `aoi add` rejects an impossible shape before writing and `--aoi` rejects one hand-edited into the file. `Config::find_aoi` deliberately skips validation, for the same reason `Config::find` exists: `aoi remove` must be able to delete an entry that no longer builds.

Coordinate range checks live in `Location::validate()`, run from `Config::groundstation()` — the only way to get a `&GroundStation`. Deserialization does not validate, so a `GroundStation` obtained any other way (e.g. indexing `groundstations` directly) is unchecked. Validation is per-lookup, not per-load, so one malformed entry does not block the others.

`ObserverArgs` carries this: `validate(&Config)` enforces that `--gs` is present and names a usable station; `resolve(config_path)` loads the config and `remove`s the station to return it owned (the `Config` is local to `resolve`). Errors list the ids the config actually defines, via `Config::ids_hint()` — preserve that wording, which `tests/config.rs` asserts on. Both commands then `.expect()` on `args.observer.gs` when writing the header, sound only because `resolve` ran first.

`GroundStation` implements the library's `Observer` trait directly, so the CLI never constructs a `GroundObserver` — it hands `&GroundStation` straight to `observation_iter`/`transits_iter`/`observe_at`. This is the "implement the trait on your own type" path the library README documents; don't reintroduce a conversion.

## Prompts (`commands/gs.rs`, `commands/aoi.rs`)

`gs add|remove|list` (aliases `rm`, `ls`) over `open_for_edit`/`save`; `commands/aoi.rs` is the same shape for AOIs. `confirm()`, `prompt()`, `prompt_f64()` and `echo()` live in `commands/mod.rs`.

Prompts and confirmations go to **stderr**, so `gs list` stays pipeable and prompts stay visible when stdout is redirected. `confirm()` treats EOF and anything but `y`/`yes` as no, so a non-interactive caller that forgot `--force` cannot delete a station. `gs list` reuses `output.rs`, so it honours `--format`.

**Prompts re-ask on a malformed line rather than aborting** (`prompt_retry`) — a typo five fields in would otherwise discard everything before it. EOF ends a prompt loop (`prompt` errors there), so a scripted caller cannot spin forever; keep that property when adding prompts. The polygon vertex loop re-asks *at the same index* on a bad line, and on a blank line before the third vertex.

Range checks live in the prompts (`prompt_latitude`, `prompt_bounded`, and the pairwise north-above-south / east-off-west's-meridian / semi-minor-under-semi-major checks), not in a value parser — there is no parser left to put them in. Keep them there, because a prompt can *re-ask*; the alternative is `build()` rejecting the shape after every field is entered and discarding all of it. The pairwise checks close over the earlier value, which is why each is inline rather than a shared helper.

`number()` and `prompt_f64` reject non-finite values: `nan`/`inf` parse as `f64` and pass every range check silently, so a non-finite West longitude makes the East prompt unsatisfiable (`(value - west).rem_euclid(360.0)` is NaN and no comparison against it is true).

Consequently `build()` is unreachable for a box or an ellipse — the prompts cover every failure. It still runs, because `--aoi` must reject a hand-edited config, and a **polygon** can still fail there: `AntipodalEdge` and `LargerThanHemisphere` are properties of the assembled ring, not of any one vertex (`tests/aoi.rs::test_invalid_geometry_is_rejected_before_saving`).

An argument that replaces a prompt is `echo()`ed in that prompt's own format, so the transcript is identical whether a value was typed or passed — which is why `tests/aoi.rs` asserts on *what stdin is consumed* rather than on which prompts appear.

The shape prompt accepts each name's initial (`b`/`e`/`c`/`p`) and underlines it with an ANSI escape **only when stderr is a terminal** (`IsTerminal`), otherwise the codes would land in a redirected log and `tests/aoi.rs` reads stderr as plain text. Clap cannot render a selection menu, and an interactive picker would mean a `dialoguer`-style dependency plus a non-TTY fallback.

**`gs add` prompting is deliberate** interactive config management, whereas TLE and observer *data* input prompts were removed so they could be piped. Do not "restore consistency" by removing it.

## TLE input (`tle.rs`)

`--tle-file` reads a file; omitting it reads *all* of stdin so a TLE can be piped in. Both funnel through one `parse_tle(&str)`, so file and pipe accept exactly the same 2-or-3-line text — keep it that way rather than adding a parser per source. When stdin is a terminal, `read_tle_stdin` prints a Ctrl-D hint to **stderr**. The observer-taking commands resolve `--gs` *before* calling `load_tle`, so a bad station id fails immediately instead of after the user has typed a TLE.
