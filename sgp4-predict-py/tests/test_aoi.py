"""Area-of-interest tests for the sgp4_predict Python bindings.

Uses the canonical SENTINEL-2C TLE from tests/common/mod.rs.
"""

from datetime import datetime, timedelta, timezone

import pytest
from sgp4_predict import (
    AoiWindow,
    Ellipse,
    FillRule,
    Geodetic,
    Interval,
    IntervalRange,
    LatLon,
    Polygon,
    Predictor,
    Rectangle,
    Tle,
)

TLE_ID = "SENTINEL-2C"
TLE_L1 = "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990"
TLE_L2 = "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740"

START = datetime(2025, 12, 22, tzinfo=timezone.utc)
END = START + timedelta(days=1)
INTERVAL = Interval(START, END)

# Wide enough to be overflown several times a day, so a single day suffices.
EUROPE_CORNERS = [(40.0, -10.0), (40.0, 30.0), (65.0, 30.0), (65.0, -10.0)]


def make_predictor() -> Predictor:
    return Predictor.from_tle(Tle(TLE_ID, TLE_L1, TLE_L2))


def europe_polygon() -> Polygon:
    return Polygon(EUROPE_CORNERS)


# ── LatLon / Geodetic ──────────────────────────────────────────────────────────


def test_lat_lon_round_trip():
    p = LatLon(55.86, -4.25)
    assert abs(p.latitude_deg - 55.86) < 1e-12
    assert abs(p.longitude_deg - -4.25) < 1e-12
    assert p == LatLon(55.86, -4.25)


def test_geodetic_round_trip():
    g = Geodetic(55.86, -4.25, 40.0)
    assert abs(g.latitude_deg - 55.86) < 1e-12
    assert abs(g.longitude_deg - -4.25) < 1e-12
    assert g.altitude == 40.0


def test_sub_point_is_beneath_the_satellite():
    p = make_predictor()
    t = datetime(2025, 12, 22, 9, 55, 0, tzinfo=timezone.utc)
    point = p.sub_point(t)
    assert -90.0 <= point.latitude_deg <= 90.0
    assert -180.0 <= point.longitude_deg <= 180.0
    # A sun-synchronous LEO sits a few hundred km up.
    assert 500e3 < point.altitude < 900e3


def test_ground_track_iter_matches_sub_point():
    p = make_predictor()
    step = timedelta(minutes=1)
    track = list(
        p.ground_track_iter(Interval(START, START + timedelta(minutes=10)), step)
    )
    # The interval is end-exclusive.
    assert len(track) == 10
    for t, point in track:
        direct = p.sub_point(t)
        assert abs(point.latitude_deg - direct.latitude_deg) < 1e-12
        assert abs(point.longitude_deg - direct.longitude_deg) < 1e-12


# ── Area construction ──────────────────────────────────────────────────────────


def test_polygon_accepts_tuples_and_lat_lon():
    from_tuples = Polygon(EUROPE_CORNERS)
    from_objects = Polygon([LatLon(lat, lon) for lat, lon in EUROPE_CORNERS])
    assert len(from_tuples.vertices) == len(from_objects.vertices) == 4
    for a, b in zip(from_tuples.vertices, from_objects.vertices):
        assert abs(a.latitude_deg - b.latitude_deg) < 1e-12
        assert abs(a.longitude_deg - b.longitude_deg) < 1e-12


def test_polygon_fill_rule_defaults_to_non_zero():
    assert europe_polygon().fill_rule == FillRule.NonZero
    assert Polygon(EUROPE_CORNERS, FillRule.EvenOdd).fill_rule == FillRule.EvenOdd


def test_polygon_rejects_too_few_vertices():
    with pytest.raises(ValueError):
        Polygon([(0.0, 0.0), (1.0, 1.0)])


def test_polygon_rejects_out_of_range_latitude():
    with pytest.raises(ValueError):
        Polygon([(0.0, 0.0), (1.0, 1.0), (91.0, 2.0)])


def test_polygon_rejects_area_larger_than_a_hemisphere():
    with pytest.raises(ValueError):
        Polygon([(0.0, 0.0), (0.0, 120.0), (0.0, -120.0), (80.0, 0.0)])


def test_rectangle_bounds_round_trip():
    rect = Rectangle((54.0, -8.0), (60.0, -1.0))
    south, north = rect.latitudes_deg
    west, span = rect.longitudes_deg
    assert abs(south - 54.0) < 1e-12
    assert abs(north - 60.0) < 1e-12
    assert abs(west - -8.0) < 1e-12
    assert abs(span - 7.0) < 1e-12


def test_rectangle_latitude_band_spans_every_longitude():
    arctic = Rectangle.latitude_band(66.5, 90.0)
    assert abs(arctic.longitudes_deg[1] - 360.0) < 1e-12
    assert arctic.signed_angular_offset_deg((80.0, 120.0)) > 0.0
    assert arctic.signed_angular_offset_deg((60.0, 120.0)) < 0.0


def test_rectangle_rejects_empty_box():
    with pytest.raises(ValueError):
        Rectangle((60.0, -8.0), (54.0, -1.0))


def test_ellipse_accessors_round_trip():
    e = Ellipse((56.0, 2.0), 2.7, 1.1, 45.0)
    assert abs(e.centre.latitude_deg - 56.0) < 1e-12
    assert abs(e.centre.longitude_deg - 2.0) < 1e-12
    assert abs(e.semi_major_deg - 2.7) < 1e-12
    assert abs(e.semi_minor_deg - 1.1) < 1e-12
    assert abs(e.bearing_deg - 45.0) < 1e-12
    first, second = e.foci
    assert first != second


def test_ellipse_circle_has_coincident_foci():
    circle = Ellipse.circle((56.0, 2.0), 2.0)
    first, second = circle.foci
    assert abs(first.latitude_deg - second.latitude_deg) < 1e-9
    assert abs(first.longitude_deg - second.longitude_deg) < 1e-9
    # For a circle the offset is the exact signed distance.
    assert abs(circle.signed_angular_offset_deg((58.0, 2.0)) - 0.0) < 1e-9


def test_ellipse_rejects_minor_axis_above_major():
    with pytest.raises(ValueError):
        Ellipse((56.0, 2.0), 1.0, 2.0, 0.0)


def test_ellipse_bearing_orients_the_major_axis():
    # Major axis east-west: reaches further in longitude than in latitude.
    e = Ellipse((0.0, 0.0), 10.0, 2.0, 90.0)
    assert e.signed_angular_offset_deg((0.0, 8.0)) > 0.0
    assert e.signed_angular_offset_deg((8.0, 0.0)) < 0.0


def test_signed_angular_offset_accepts_every_point_form():
    rect = Rectangle((54.0, -8.0), (60.0, -1.0))
    inside = rect.signed_angular_offset_deg((57.0, -4.5))
    assert inside > 0.0
    assert rect.signed_angular_offset_deg(LatLon(57.0, -4.5)) == inside
    assert rect.signed_angular_offset_deg(Geodetic(57.0, -4.5, 0.0)) == inside


def test_area_rejects_a_non_area():
    p = make_predictor()
    with pytest.raises(TypeError):
        list(p.aoi_iter("not-an-area", INTERVAL))


# ── Detection ──────────────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    "area",
    [
        Polygon(EUROPE_CORNERS),
        Rectangle((40.0, -10.0), (65.0, 30.0)),
        Ellipse((52.0, 10.0), 14.0, 4.0, 60.0),
        Ellipse.circle((52.0, 10.0), 10.0),
    ],
    ids=["polygon", "rectangle", "ellipse", "circle"],
)
def test_aoi_iter_yields_windows_over_every_shape(area):
    p = make_predictor()
    windows = list(p.aoi_iter(area, INTERVAL))
    assert len(windows) > 0
    for w in windows:
        assert isinstance(w, AoiWindow)
        assert START <= w.start < w.end <= END
        assert w.duration_seconds > 0.0


def test_aoi_window_ground_track_is_inside_the_area():
    p = make_predictor()
    area = europe_polygon()
    windows = list(p.aoi_iter(area, INTERVAL))
    assert len(windows) > 0
    for w in windows:
        midpoint = w.start + (w.end - w.start) / 2
        assert area.signed_angular_offset_deg(p.sub_point(midpoint)) > 0.0
        # A second before entry and after exit the track is outside.
        assert (
            area.signed_angular_offset_deg(p.sub_point(w.start - timedelta(seconds=1)))
            < 0.0
        )
        assert (
            area.signed_angular_offset_deg(p.sub_point(w.end + timedelta(seconds=1)))
            < 0.0
        )


def test_aoi_iter_is_lazy():
    p = make_predictor()
    it = p.aoi_iter(europe_polygon(), INTERVAL)
    first = next(it)
    second = next(it)
    assert first.start < second.start


def test_detect_aoi_at_window_midpoint():
    p = make_predictor()
    area = europe_polygon()
    windows = list(p.aoi_iter(area, INTERVAL))
    assert len(windows) > 0
    window = windows[0]
    midpoint = window.start + (window.end - window.start) / 2

    detected = p.detect_aoi(midpoint, area)
    assert detected is not None
    assert abs((detected.start - window.start).total_seconds()) < 1.0
    assert abs((detected.end - window.end).total_seconds()) < 1.0


def test_detect_aoi_returns_none_when_outside():
    p = make_predictor()
    windows = list(p.aoi_iter(europe_polygon(), INTERVAL))
    assert len(windows) > 0
    # A minute before the first entry the track is still outside.
    assert (
        p.detect_aoi(windows[0].start - timedelta(minutes=1), europe_polygon()) is None
    )


def test_aoi_window_satisfies_interval_range_protocol():
    p = make_predictor()
    windows = list(p.aoi_iter(europe_polygon(), INTERVAL))
    assert len(windows) > 0
    assert isinstance(windows[0], IntervalRange)


def test_ground_track_iter_accepts_an_aoi_window():
    """An AoiWindow can be passed directly as an interval."""
    p = make_predictor()
    windows = list(p.aoi_iter(europe_polygon(), INTERVAL))
    assert len(windows) > 0
    window = windows[0]
    track = list(p.ground_track_iter(window, timedelta(seconds=10)))
    assert len(track) > 0
    assert all(window.start <= t <= window.end for t, _ in track)


def test_reversed_ring_gives_the_same_windows():
    p = make_predictor()
    forward = list(p.aoi_iter(Polygon(EUROPE_CORNERS), INTERVAL))
    reverse = list(p.aoi_iter(Polygon(list(reversed(EUROPE_CORNERS))), INTERVAL))
    assert len(forward) == len(reverse)
    for a, b in zip(forward, reverse):
        assert abs((a.start - b.start).total_seconds()) < 1e-6
        assert abs((a.end - b.end).total_seconds()) < 1e-6


def test_aoi_iter_over_an_area_never_overflown_is_empty():
    p = make_predictor()
    # A small circle in the mid-Pacific, well off a single day's ground track.
    area = Ellipse.circle((-40.0, -140.0), 0.5)
    assert list(p.aoi_iter(area, Interval(START, START + timedelta(hours=2)))) == []
