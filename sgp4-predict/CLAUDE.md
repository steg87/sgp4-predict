# Area-of-interest detection (`aoi.rs`)

`AoiIter` finds the windows in which the sub-satellite point is inside an `Area`. `Polygon`, `Rectangle` and `Ellipse` are the built-in implementations; anything else goes behind the same trait.

**The event function is one signed scalar and a pure function of `t`.** This is the load-bearing decision — the alternative, tracking which edge was crossed with an inside/outside flag, fails at a vertex, where the in-arc test is exactly zero for _both_ adjoining edges and one desync inverts the flag permanently. Do not introduce state into `GroundTrackInside::sample`, and in particular do not make it sub-sample internally to "improve" resolution: refinement samples out of order, so any call-history dependence breaks it. `walk_to_crossing` already guarantees a crossing is never re-detected.

**`Area`'s contract is a bound, not an equality**, and deliberately does not require continuity: `|value|` must never _exceed_ the true angular distance to the boundary. Under-reporting is always safe; over-reporting breaks the step guarantee. That is what legitimises the bounding-cap early return, which jumps discontinuously — `detect.rs` only ever tests `value < 0.0`, so a sign-preserving magnitude jump is harmless. But the cap branch must never return exactly `0.0`, since zero counts as inside.

**The hemisphere restriction is a correctness requirement, not a convenience.** The tangent-plane signed-angle sum measures degree on `S² ∖ {p, −p}`, not a planar winding number, so without the bounding-cap gate the _antipode_ of the polygon also winds to ±1 and reads as inside — roughly half the emitted windows would be on the far side of the Earth. The cap is only sound if the region fits inside it, hence `Error::LargerThanHemisphere`. Do not "simplify" the cap away as a mere fast path. The restriction is narrower than it sounds: equator-crossing, antimeridian-spanning and pole-containing areas are all fine, and a full-longitude ring is accepted as the polar cap on its centroid's side. The centroid axis is not the minimal enclosing cap, so the check is conservative.

**Vertex order is ignored** (`NonZero`/`EvenOdd` both derive from `|k|`), which is what makes a reversed ring identical. Orientation-sensitivity and the cap prefilter are mutually incompatible: resolving "which side is inside" by winding order requires admitting regions the cap cannot contain.

**`ProximityStep` is what makes narrow areas safe.** Step `|value| / ω_max` and the boundary cannot be reached within the step, so no chord is ever jumped. `max_sub_point_rate` derives `ω_max` in closed form from the element set — perigee angular rate `n√(1−e²)/(1−e)²`, plus `ω_E` because the ground point is in ECEF, times `1/(1−e²_WGS84)` for the geodetic-latitude stretch, times 1.05. Deriving beats sampling the orbit, which costs propagations and can miss the maximum; the empirical cross-check lives in `tests/aoi.rs` instead.

The `min_step` floor is a knob, not a fixed limit: it is floored at `MIN_AOI_STEP` (1 ms), deliberately **not** at `detect::MIN_POSITIVE_STEP` (1 s), because it bounds the shortest crossing the scan can see and a 1 s floor would cap that at ~6.6 km of track. `max_step` is raised to the resolved `min` rather than to a constant, so a wholly sub-second pair is honoured. Do not "make it consistent" with the coarse scans that clamp at a second — the same distinction `DateTimeIter` draws (see the root `CLAUDE.md`).

Known limits: the `min_step` floor voids the step guarantee below its own scale, and `WindowIter`'s boundary walk uses a fixed `walk_step`, so a concave notch crossed in under `walk_step` is absorbed into the surrounding window. Fixing the walk needs a signed walk strategy in `detect.rs` (the walk runs both directions while `StepStrategy::next_time` returns an absolute forward time), which is a public API break under `generics` for little gain. Revisit only if a real notch bug appears.

## `Rectangle`

A separate `Area` impl rather than a four-vertex `Polygon`, because a polygon's great-circle edges bow toward the nearer pole — _both_ horizontal edges move the same way, so the region is displaced poleward, not merely enlarged. Containment is four inequalities, so the antipodal-winding problem does not arise: no bounding cap, and no hemisphere restriction.

Two non-obvious details:

- A bound at a pole contributes no edge, since the parallel there is a single interior point. Counting it would peg the reported distance to zero at the pole and collapse the step size.
- Each meridian edge is tested against its own half of its plane (`dot(foot, equator) > 0` plus the foot's latitude), because a meridian plane wraps round the globe and its antipodal half would otherwise report a near-zero distance for points on the far side. This looks like it should break at a pole and does not — `dot(foot, m.equator)` is identically zero there so both meridians are skipped, but a pole is only interior in latitude when the bound _is_ ±90°, and then both corners **are** the pole, the corner term falls under `ON_BOUNDARY`, and the answer is `0.0`. Correct: the pole is where the two meridian edges meet. `test_pole_to_pole_wedge` pins it.

The whole-sphere band (`latitude_band(-90°, 90°)`) is the one box with no boundary at all — `sides` is `None`, both parallel terms are gated off, and `d` is left at its `f64::INFINITY` seed. It is clamped to π, the widest separation on a sphere, because only the _magnitude_ is unconstrained there; the sign is still right, and under-reporting is what the contract allows. The clamp is inert for every other box. It does not stop a whole-Earth aoi hitting `WindowTooLong` — the track never leaves, true of any near-global area.

Relatedly, `build` widens `lon_span` to exactly `TAU` whenever it drops the sides, rather than storing the span it was given. The two have to agree: a span within `COINCIDENT` of full has no meridian edges but would still leave `contains` excluding a sliver up to 1e-9 rad wide, and a point there has no edge to be measured against.

## `Ellipse`

The two-foci definition (`d(F₁,p) + d(F₂,p) <= 2a`), not a projected planar ellipse. The value returned is `a − (d₁ + d₂)/2`, and **the halving is what makes it legal**: each distance is 1-Lipschitz along the surface, so the sum is 2-Lipschitz, and only half the shortfall is guaranteed not to exceed the distance to the boundary. Do not drop the `/2` to "tighten" it — the tighter value is the local gradient `|û₁ + û₂|`, which is not a bound along the whole path to the boundary. The under-estimate costs nothing but smaller steps, and for a circle the two foci coincide and the formula is exact.

Focal separation comes from `cos a = cos b cos c`, the spherical right triangle at a minor-axis endpoint. `semi_major < 90°` is required to keep that ratio in `[0, 1]`; it also rules out an antipodal component, hence no bounding cap and no hemisphere restriction, unlike `Polygon`. `local_frame` falls back to the prime-meridian direction at a pole, where north is undefined.

## Non-finite input

**Every constructor rejects a non-finite argument** (`Error::NotFinite`), because nothing downstream will: a NaN slips past every comparison that would otherwise catch it, gets baked into the shape, and makes every `signed_angular_offset` NaN. `ProximityStep` floors NaN to `min_step`, so the symptom is the whole interval ground through at 1 ms with no error ever raised. `checked_latitude` needs no separate test (its range check already fails NaN and both infinities); `checked_angle` covers the longitudes, the ellipse bearing and the semi-axes — which is what `NotFinite`'s free-form `what` field is for, so each new site names itself rather than adding a variant.

Test-sizing gotcha: a 7°-wide box at 57°N is only overflown on some days, so `tests/aoi.rs` searches a month for the small `scotland()` area and reserves the 1-second `dense_scan` cross-checks for larger areas over a single day.
