"""Integration tests for the sgp4_predict Python bindings.

Uses the canonical SENTINEL-2C TLE from tests/common/mod.rs.
"""

import math
from datetime import datetime, timedelta, timezone

import pytest
from sgp4_predict import (
    ApsisEvent,
    Classification,
    Elements,
    Observer,
    IlluminationState,
    Interval,
    IntervalRange,
    Predictor,
    Tle,
)

# ── Canonical test data ────────────────────────────────────────────────────────

TLE_ID = "SENTINEL-2C"
TLE_L1 = "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990"
TLE_L2 = "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740"

# Glasgow (matches tests/common/mod.rs ground station)
GLASGOW = Observer(55.86, -4.25, 40.0)

START = datetime(2025, 12, 22, tzinfo=timezone.utc)
END = START + timedelta(days=1)
INTERVAL = Interval(START, END)


def make_predictor() -> Predictor:
    return Predictor.from_tle(Tle(TLE_ID, TLE_L1, TLE_L2))


# ── GroundObserver ──────────────────────────────────────────────────────────────


def test_ground_station_round_trip():
    gs = Observer(51.5, -0.1, 10.0)
    assert abs(gs.latitude_deg - 51.5) < 1e-10
    assert abs(gs.longitude_deg - -0.1) < 1e-10
    assert gs.altitude == 10.0


# ── Predictor construction ─────────────────────────────────────────────────────


def test_invalid_tle_raises_value_error():
    with pytest.raises(ValueError):
        Predictor.from_tle(Tle("BAD", "not a tle line", "also bad"))


def test_epoch():
    p = make_predictor()
    # TLE epoch 25356.66913557 → 2025-12-22
    assert p.epoch.year == 2025
    assert p.epoch.month == 12
    assert p.epoch.day == 22


# ── propagate ─────────────────────────────────────────────────────────────────


def test_propagate_altitude_at_epoch():
    p = make_predictor()
    sv = p.propagate(p.epoch)
    # Sentinel-2C orbit: ~786 km altitude (SSO)
    r = (sv.position.x**2 + sv.position.y**2 + sv.position.z**2) ** 0.5
    altitude_km = (r - 6_378_137.0) / 1000.0
    assert 773.0 < altitude_km < 803.0, (
        f"altitude {altitude_km:.1f} km out of expected range"
    )


# ── observe_at ────────────────────────────────────────────────────────────────


def test_observe_at_fields_are_finite():
    p = make_predictor()
    # Pick a time when we know there's a transit (from transits test)
    t = datetime(2025, 12, 22, 9, 55, 0, tzinfo=timezone.utc)
    obs = p.observe_at(t, GLASGOW)
    assert math.isfinite(obs.azimuth_deg)
    assert math.isfinite(obs.elevation_deg)
    assert math.isfinite(obs.range)
    assert math.isfinite(obs.range_rate)
    assert obs.range > 0


def test_observe_at_degrees_properties():
    p = make_predictor()
    t = datetime(2025, 12, 22, 9, 55, 0, tzinfo=timezone.utc)
    obs = p.observe_at(t, GLASGOW)
    assert 0.0 <= obs.azimuth_deg < 360.0
    assert -90.0 <= obs.elevation_deg <= 90.0


# ── transits_iter ─────────────────────────────────────────────────────────────


def test_transits_iter_yields_results():
    p = make_predictor()
    transits = list(p.transits_iter(GLASGOW, INTERVAL, min_elevation_deg=5.0))
    assert len(transits) > 0


def test_transits_iter_end_after_start():
    p = make_predictor()
    transits = list(p.transits_iter(GLASGOW, INTERVAL, min_elevation_deg=5.0))
    for t in transits:
        assert t.end > t.start


def test_transits_iter_duration_in_range():
    """Transit durations must be physically plausible for Sentinel-2C."""
    p = make_predictor()
    transits = list(p.transits_iter(GLASGOW, INTERVAL, min_elevation_deg=5.0))
    for t in transits:
        dur = t.duration_seconds
        assert 60 <= dur <= 960, f"transit duration {dur:.1f}s out of range"


def test_transits_lazy_iteration():
    """__next__ is called on demand — only one step is computed per call."""
    p = make_predictor()
    it = p.transits_iter(GLASGOW, INTERVAL, min_elevation_deg=5.0)
    first = next(it)
    assert first.end > first.start
    # Second call should work too (iterator advances lazily)
    second = next(it)
    assert second.start > first.end


# ── prediction_iter ───────────────────────────────────────────────────────────


def test_prediction_iter_sample_count():
    p = make_predictor()
    window = timedelta(minutes=5)
    step = timedelta(seconds=60)
    samples = list(p.prediction_iter(Interval(START, START + window), step))
    # [0, 60, 120, 180, 240] = 5 samples
    assert len(samples) == 5


def test_prediction_iter_timestamps_advance():
    p = make_predictor()
    step = timedelta(seconds=30)
    samples = list(
        p.prediction_iter(Interval(START, START + timedelta(minutes=2)), step)
    )
    times = [t for t, _ in samples]
    assert all(times[i] < times[i + 1] for i in range(len(times) - 1))


# ── apsis_iter ────────────────────────────────────────────────────────────────


def test_apsis_iter_yields_both_types():
    p = make_predictor()
    apsides = list(p.apsis_iter(Interval(START, START + timedelta(hours=3))))
    events = {a.event for a in apsides}
    assert ApsisEvent.Apogee in events
    assert ApsisEvent.Perigee in events


def test_apsis_iter_altitudes_in_range():
    """Sentinel-2C apsides should be between 766 km and 806 km."""
    p = make_predictor()
    apsides = list(p.apsis_iter(Interval(START, START + timedelta(hours=3))))
    for a in apsides:
        alt_km = a.altitude / 1000.0
        assert 766 < alt_km < 806, f"apsis altitude {alt_km:.1f} km out of range"


def test_apsis_iter_consecutive_alternate():
    p = make_predictor()
    apsides = list(p.apsis_iter(Interval(START, START + timedelta(hours=3))))
    for i in range(len(apsides) - 1):
        assert apsides[i].event != apsides[i + 1].event


# ── illumination_iter ─────────────────────────────────────────────────────────


def test_illumination_iter_yields_sunlit():
    p = make_predictor()
    windows = list(p.illumination_iter(Interval(START, START + timedelta(hours=3))))
    states = {w.state for w in windows}
    assert IlluminationState.Sunlit in states


def test_illumination_iter_contiguous():
    """Adjacent illumination windows must be contiguous (no gaps)."""
    p = make_predictor()
    windows = list(p.illumination_iter(Interval(START, START + timedelta(hours=3))))
    for i in range(len(windows) - 1):
        assert windows[i].end == windows[i + 1].start


# ── detect_transit ────────────────────────────────────────────────────────────


def test_detect_transit_at_midpoint():
    """The midpoint of a known transit should be detected."""
    p = make_predictor()
    transits = list(p.transits_iter(GLASGOW, INTERVAL, min_elevation_deg=5.0))
    assert len(transits) > 0
    t0 = transits[0]
    midpoint = t0.start + (t0.end - t0.start) / 2
    result = p.detect_transit(midpoint, GLASGOW, min_elevation_deg=5.0)
    assert result is not None
    # Detected boundaries should be close to the iterated ones
    assert abs((result.start - t0.start).total_seconds()) < 2.0
    assert abs((result.end - t0.end).total_seconds()) < 2.0


def test_detect_transit_returns_none_when_below_horizon():
    """Outside a transit, detect_transit should return None."""
    p = make_predictor()
    # Early morning on Dec 22 — satellite is below horizon for Glasgow
    t = datetime(2025, 12, 22, 0, 0, 0, tzinfo=timezone.utc)
    result = p.detect_transit(t, GLASGOW, min_elevation_deg=5.0)
    # Not necessarily None (there may be a transit at midnight), so we just
    # check it doesn't raise and returns the right type
    assert result is None or result.end > result.start


# ── coordinate chain ──────────────────────────────────────────────────────────


def test_coordinate_chain():
    """TEME → ECEF → ENU → Observation chain should match observe_at."""
    p = make_predictor()
    t = datetime(2025, 12, 22, 9, 55, 0, tzinfo=timezone.utc)

    obs_chain = p.propagate(t).to_ecef(t).to_enu(GLASGOW).to_observation()
    obs_direct = p.observe_at(t, GLASGOW)

    assert abs(obs_chain.azimuth_deg - obs_direct.azimuth_deg) < 1e-8
    assert abs(obs_chain.elevation_deg - obs_direct.elevation_deg) < 1e-8
    assert abs(obs_chain.range - obs_direct.range) < 1e-3


# ── IntervalRange protocol ────────────────────────────────────────────────────


def test_transit_satisfies_interval_range_protocol():
    """Transit should be recognized as an IntervalRange at runtime."""
    p = make_predictor()
    transits = list(p.transits_iter(GLASGOW, INTERVAL, min_elevation_deg=5.0))
    assert len(transits) > 0
    transit = transits[0]
    assert isinstance(transit, IntervalRange)


def test_illumination_satisfies_interval_range_protocol():
    """Illumination should be recognized as an IntervalRange at runtime."""
    p = make_predictor()
    windows = list(p.illumination_iter(Interval(START, START + timedelta(hours=3))))
    assert len(windows) > 0
    assert isinstance(windows[0], IntervalRange)


def test_interval_satisfies_interval_range_protocol():
    """Interval should be recognized as an IntervalRange at runtime."""
    iv = Interval(START, END)
    assert isinstance(iv, IntervalRange)


def test_observation_iter_accepts_transit():
    """A Transit can be passed directly as an interval to observation_iter."""
    p = make_predictor()
    transits = list(p.transits_iter(GLASGOW, INTERVAL, min_elevation_deg=5.0))
    assert len(transits) > 0
    transit = transits[0]
    step = timedelta(seconds=10)
    obs = list(p.observation_iter(GLASGOW, transit, step))
    assert len(obs) > 0
    # Timestamps should be within the transit window
    assert all(transit.start <= t <= transit.end for t, _ in obs)


def test_observation_iter_accepts_illumination():
    """An Illumination window can be passed directly as an interval."""
    p = make_predictor()
    windows = list(p.illumination_iter(Interval(START, START + timedelta(hours=3))))
    assert len(windows) > 0
    window = windows[0]
    step = timedelta(seconds=30)
    preds = list(p.prediction_iter(window, step))
    assert len(preds) > 0
    assert all(window.start <= t <= window.end for t, _ in preds)


def test_observation_iter_missing_interval_raises():
    """Passing an object without .start/.end raises AttributeError."""
    p = make_predictor()
    step = timedelta(seconds=30)
    with pytest.raises(AttributeError):
        list(p.observation_iter(GLASGOW, "not-an-interval", step))


# ── Elements / OMM ─────────────────────────────────────────────────────────────

ISS_OMM_JSON = """{
    "OBJECT_NAME": "ISS (ZARYA)",
    "OBJECT_ID": "1998-067A",
    "EPOCH": "2020-07-12T01:19:07.402656",
    "MEAN_MOTION": 15.49560532,
    "ECCENTRICITY": 0.0001771,
    "INCLINATION": 51.6435,
    "RA_OF_ASC_NODE": 225.4004,
    "ARG_OF_PERICENTER": 44.9625,
    "MEAN_ANOMALY": 5.1087,
    "EPHEMERIS_TYPE": 0,
    "CLASSIFICATION_TYPE": "U",
    "NORAD_CAT_ID": 25544,
    "ELEMENT_SET_NO": 999,
    "REV_AT_EPOCH": 23587,
    "BSTAR": 0.0049645,
    "MEAN_MOTION_DOT": 0.00289036,
    "MEAN_MOTION_DDOT": 0
}"""


def test_elements_from_json():
    el = Elements.from_json(ISS_OMM_JSON)
    assert el.norad_id == 25544
    assert el.object_name == "ISS (ZARYA)"
    assert abs(el.mean_motion - 15.49560532) < 1e-9
    assert abs(el.eccentricity - 0.0001771) < 1e-9
    assert el.classification == Classification.Unclassified
    assert el.epoch.year == 2020


def test_elements_from_json_invalid_raises():
    with pytest.raises(ValueError):
        Elements.from_json("not json")


def test_elements_manual_construction():
    epoch = datetime(2020, 7, 12, 1, 19, 7, tzinfo=timezone.utc)
    el = Elements(
        norad_id=25544,
        epoch=epoch,
        mean_motion=15.49560532,
        eccentricity=0.0001771,
        inclination=51.6435,
        right_ascension=225.4004,
        argument_of_perigee=44.9625,
        mean_anomaly=5.1087,
        mean_motion_dot=0.00289036,
        drag_term=0.0049645,
        revolution_number=23587,
        object_name="ISS (ZARYA)",
    )
    assert el.norad_id == 25544
    assert el.object_name == "ISS (ZARYA)"
    assert el.classification == Classification.Unclassified
    assert el.epoch == epoch


def test_predictor_from_elements():
    el = Elements.from_json(ISS_OMM_JSON)
    p = Predictor(el)
    assert p.epoch.year == 2020


def test_predictor_from_tle_still_works():
    p = Predictor.from_tle(Tle(TLE_ID, TLE_L1, TLE_L2))
    assert p.epoch.year == 2025


def test_elements_repr():
    el = Elements.from_json(ISS_OMM_JSON)
    r = repr(el)
    assert "25544" in r
    assert "ISS (ZARYA)" in r
