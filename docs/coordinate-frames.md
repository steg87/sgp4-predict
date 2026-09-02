# Coordinate Frames

SGP4 returns a state vector in the **TEME** frame; a ground observer needs **ENU**, and a
satellite pointing at the ground needs **LVLH**. Mixing frames produces plausible-looking but
completely wrong numbers, so this library makes a wrong conversion a compile error.

## The four frames

**TEME — True Equator, Mean Equinox.** The native output frame of SGP4. Origin at Earth's centre of
mass, X toward the mean vernal equinox, Z along Earth's rotation axis. It does not rotate with
Earth's surface — suitable for propagation, not for ground geometry.

**ECEF — Earth-Centred, Earth-Fixed.** Same origin, but X is fixed to the prime meridian, so the
frame rotates with Earth and a point on the ground has constant coordinates.

**ENU — East-North-Up.** Local to the observer: E and N tangent to the ellipsoid, U along its
normal. This is the natural basis for azimuth and elevation.

**LVLH — Local-Vertical, Local-Horizontal.** Local to the *satellite*: Z along nadir, Y along the
negative orbit normal, X completing the right-handed triad along-track. This is the natural basis
for pointing an instrument or antenna at a target.

## Conversion pipelines

Looking up, from the ground:

```mermaid
flowchart LR
    TEME["StateVector&lt;Teme&gt;"]
    -->|"to_ecef(t)"| ECEF["StateVector&lt;Ecef&gt;"]
    -->|"to_enu(observer)"| ENU["StateVector&lt;Enu&gt;"]
    -->|"to_observation()"| OBS["Observation"]
```

TEME → ECEF is a Z-rotation by the GMST angle at `t`; ECEF → ENU translates to the observer and
rotates into the local tangent plane; ENU → `Observation` is `atan2` for azimuth, `asin` for
elevation, magnitude for range.

Looking down, from the satellite:

```mermaid
flowchart LR
    GP["GeodeticPoint"]
    -->|"to_ecef()"| ECEF2["StateVector&lt;Ecef&gt;"]
    -->|"to_teme(t)"| TEME2["StateVector&lt;Teme&gt;"]
    -->|"to_lvlh(target)"| LVLH["StateVector&lt;Lvlh&gt;"]
    -->|"to_pointing()"| PT["Pointing"]
```

`to_teme` is the inverse of `to_ecef`, frame-drag term included — the LVLH triad is built from
inertial velocity, so a ground point's TEME motion has to be right. `to_lvlh` is called on the
satellite's own state with the target as its argument.

`Predictor::observe_at` and `Predictor::point_at` run their whole chain. The individual steps are
public for callers who need an intermediate frame.

`StateVector<F>` carries its frame as a zero-sized type parameter, and each conversion is
implemented only on the frame it starts from, so skipping or reordering one is a compile error
rather than a plausible-looking wrong number.
