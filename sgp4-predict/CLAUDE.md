# Area-of-interest detection (`aoi.rs`)

`AoiIter` finds the windows in which an `Area` is within the payload's reach. `Polygon`, `Rectangle` and `Circle` are the built-in implementations; anything else goes behind the same trait.

**The event function is one signed scalar and a pure function of `t`.** This is the load-bearing decision — the alternative, tracking which edge was crossed with an inside/outside flag, fails at a vertex, where the in-arc test is exactly zero for _both_ adjoining edges and one desync inverts the flag permanently. Do not introduce state into `AreaInView::sample`, and in particular do not make it sub-sample internally to "improve" resolution: refinement samples out of order, so any call-history dependence breaks it. `walk_to_crossing` already guarantees a crossing is never re-detected.

**`Area`'s contract is a bound, not an equality**, and deliberately does not require continuity: `|value|` must never _exceed_ the true angular distance to the boundary. Under-reporting is always safe; over-reporting breaks the step guarantee. `detect.rs` only ever tests `value < 0.0`, so a sign-preserving magnitude jump is harmless there — but never return exactly `0.0` for a point outside, since zero counts as inside. All three built-ins report the exact distance anyway, which a non-zero `max_off_nadir` needs: it compares the magnitude against the field of regard rather than against zero, so slack there becomes error at the window edges rather than costing nothing.

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

## Field of regard (`max_off_nadir`)

`max_central_angle` converts the off-nadir half-angle `η` into the central angle `λ` the payload reaches from the sub-satellite point: `λ = asin((r/re) sin η) − η`, the standard coverage relation re-parameterised from elevation (`λ = acos((re/r) cos ε) − ε`) via `90° − asin x = acos x`. Past grazing incidence, `asin` has no solution and the result clamps to the horizon `acos(re/r)` — which is also the line-of-sight check, so there is deliberately no separate one.

**The whole feature is a change of threshold, not of geometry.** `Coverage::Any` samples `offset + λ` instead of `offset`, so the area is untouched and no dilated approximation is built. This is what keeps `ProximityStep`'s no-skip guarantee intact, and the argument is the reason the step strategy needed no change:

- `Any`: for a point inside the area every path out of the dilated region crosses `∂A` first, so `dist(p, ∂(A ⊕ λ)) ≥ dist(p, ∂A) + λ`; outside it is `dist(p, ∂A) − λ`. `offset + λ` is exactly that in both signs.
- `Full`: `λ − G(p)` for any 1-Lipschitz `G` satisfies `|λ − G(p)| ≤ dist(p, {G = λ})` automatically, which is what `max_angular_distance`'s "no faster than the point moves" clause buys.

`λ` itself drifts with altitude at ~7e-7 rad/s for LEO against a sub-point rate of ~1.1e-3 rad/s, three orders down and absorbed by `max_sub_point_rate`'s existing 1.05 `SAFETY` factor.

**Two small frame mismatches are accepted and documented rather than corrected.** Both are an order below TLE error, and both are invisible at `max_off_nadir` zero, where only the offset's sign matters:

- `unit_from_lat_lon` uses geodetic latitude as spherical latitude, so `offset` is an angle on that pseudo-sphere while `λ` is a true geocentric central angle. The north-south stretch is `(1−e²)/(cos²φ + (1−e²)² sin²φ)` — 0.9933 at the equator, 1.0067 at a pole, none east-west. Up to 0.67%, about 0.015° (1.6 km) at a 2.2° reach. Correcting it would mean scaling `λ` by a latitude-dependent factor, which buys less than the nadir convention below costs.
- `re = r − altitude` is the local radius along the geodetic normal rather than the radius vector; ~2.3 m at LEO.

Neither shows up in the 0.17° round-trip cross-check at 52°N: the stretch factor there is 1.0016, worth 0.0035° at a 2.2° reach.

**Nadir is geocentric**, measured from the position vector rather than the ellipsoid normal. `Pointing::off_nadir` uses the same convention deliberately, so a target's off-nadir angle and this field of regard compare directly without a correction; `tests/pointing.rs` pins the difference from the geodetic convention rather than letting it drift. The two differ by up to 0.19° of tilt at mid-latitudes, ~2.3 km of ground reach — the largest term under our control when reconciling against another tool, so it is documented on the field rather than left implicit. `re` is taken as `r − altitude`, along the geodetic normal rather than the radius vector; ~3 m at LEO.

**The accepted ceiling is that the field of regard must be a circular cone about nadir.** An asymmetric one — separate across-track and along-track slew limits — breaks the reduction to "distance from the sub-point" and would need Orekit's sample-the-zone-and-project approach instead. `FootprintOverlapDetector` is the reference implementation of the general problem; it discretises the zone's boundary and interior and is accurate only to its `samplingStep`, whereas the reduction here is exact and `O(edges)`. That trade is the reason for the restriction, not an oversight.

`Coverage::Full` means "every part of the area is reachable at once", **not** "one image covers it". The latter is a field-of-view question needing strip decomposition, which is out of scope; the doc comment says so because the two are easy to conflate.

## `max_angular_distance`

The supplied trait implementation is `π − d(antipode)`: the farthest point of an area from `p` is the nearest one to `p`'s antipode. It needs no override for an area whose offset is exact and continuous, which all three built-ins are — so there are no hand-written versions to keep in step.

**The continuity requirement is real and does not come free from `Area`'s contract.** `signed_angular_offset` explicitly waives continuity, so a legal-but-discontinuous custom area inherits a discontinuous `G = π − d(antipode)`. `Coverage::Full`'s bound `|λ − G| ≤ dist(p, {G = λ})` needs `G` 1-Lipschitz, not merely an over-estimate, so `ProximityStep` could step over a `Full` crossing for such an area. No built-in is affected — the waiver exists for bounds like the old polygon cap return, which is gone. The trait doc says a discontinuous area must supply its own. Two things it gets right that a max-over-vertices implementation would not: an area containing the antipode (the `.min(0.0)` clamp caps it at π), and `Rectangle`'s parallel edges, whose farthest point is mid-parallel rather than at a corner.

The default is `π − d(antipode)` rather than `PI` because a `PI` default would let a downstream `Area` compile and then silently never report a `Coverage::Full` window.

## `Circle`

A spherical cap: `radius − angle_between(centre, p)`, exact everywhere in both directions. It replaced a two-foci `Ellipse` whose offset was `a − (d₁+d₂)/2`, where the halving was required for the step guarantee and cost up to a factor of two in accuracy. Against a zero threshold that was free, since only the sign mattered; against `λ` the magnitude is load-bearing, and an eccentric ellipse would have reported access that did not exist. Every built-in area is now exact, which is what keeps the accuracy caveat out of the docs entirely. An elongated or oriented region is a `Polygon`.

`Polygon`'s bounding cap changed for the same reason: it still gates `winding` (see the hemisphere restriction above) but no longer supplies the magnitude, which now always comes from the edge loop. The old `-(from_axis - cap_radius)` was a legal bound and a loose one for an elongated ring.

## Non-finite input

**Every constructor rejects a non-finite argument** (`Error::NotFinite`), because nothing downstream will: a NaN slips past every comparison that would otherwise catch it, gets baked into the shape, and makes every `signed_angular_offset` NaN. `ProximityStep` floors NaN to `min_step`, so the symptom is the whole interval ground through at 1 ms with no error ever raised. `checked_latitude` needs no separate test (its range check already fails NaN and both infinities); `checked_angle` covers the longitudes and the circle radius — which is what `NotFinite`'s free-form `what` field is for, so each new site names itself rather than adding a variant.

Test-sizing gotcha: a 7°-wide box at 57°N is only overflown on some days, so `tests/aoi.rs` searches a month for the small `scotland()` area and reserves the 1-second `dense_scan` cross-checks for larger areas over a single day.
