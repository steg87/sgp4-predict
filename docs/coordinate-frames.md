# Coordinate Frames

SGP4 returns a state vector in the **TEME** frame; a ground observer needs **ENU**. Mixing frames
produces plausible-looking but completely wrong numbers, so this library makes a wrong conversion a
compile error.

## The three frames

**TEME — True Equator, Mean Equinox.** The native output frame of SGP4. Origin at Earth's centre of
mass, X toward the mean vernal equinox, Z along Earth's rotation axis. It does not rotate with
Earth's surface — suitable for propagation, not for ground geometry.

**ECEF — Earth-Centred, Earth-Fixed.** Same origin, but X is fixed to the prime meridian, so the
frame rotates with Earth and a point on the ground has constant coordinates.

**ENU — East-North-Up.** Local to the observer: E and N tangent to the ellipsoid, U along its
normal. This is the natural basis for azimuth and elevation.

## Conversion pipeline

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

`Predictor::observe_at` runs the whole chain. The individual steps are public for callers who need
an intermediate frame.

`StateVector<F>` carries its frame as a zero-sized type parameter, and each conversion is
implemented only on the frame it starts from, so skipping or reordering one is a compile error
rather than a plausible-looking wrong number.
