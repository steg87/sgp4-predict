# ruff: noqa: E501, F401, F403, F405

from collections.abc import Iterable
from datetime import datetime, timedelta
from typing import Protocol, TypeAlias, runtime_checkable

from sgp4_predict._sgp4_predict import *
from sgp4_predict._sgp4_predict import (
    AoiIter,
    AoiWindow,
    Apsis,
    ApsisEvent,
    ApsisIter,
    Classification,
    Elements,
    FillRule,
    Geodetic,
    GroundObserver,
    GroundTrackIter,
    Illumination,
    IlluminationIter,
    IlluminationState,
    LatLon,
    Observation,
    ObservationIter,
    PredictionIter,
    Refinement,
    Tle,
    StateVectorEcef,
    StateVectorEnu,
    StateVectorTeme,
    Transit,
    TransitIter,
    Vec3,
)

@runtime_checkable
class IntervalRange(Protocol):
    @property
    def start(self) -> datetime: ...
    @property
    def end(self) -> datetime: ...

class Interval:
    """Concrete datetime interval satisfying IntervalRange."""
    def __init__(self, start: datetime, end: datetime) -> None: ...
    @property
    def start(self) -> datetime: ...
    @property
    def end(self) -> datetime: ...
    def __repr__(self) -> str: ...

# A LatLon, a Geodetic, or a plain (latitude_deg, longitude_deg) tuple.
LatLonLike: TypeAlias = LatLon | Geodetic | tuple[float, float]

class Elements:
    # from_dict is not emitted by pyo3-stub-gen (PyAny limitation); defined here manually.
    @staticmethod
    def from_dict(data: dict) -> Elements:
        """Parse an OMM dict into orbital elements (Celestrak / Space-Track format)."""
        ...

# The three area shapes take LatLonLike arguments, which pyo3-stub-gen widens to
# Any; they are redeclared here so the tuple form type-checks.

class Polygon:
    """A closed polygon on Earth's surface whose edges are great-circle arcs."""
    def __new__(cls, vertices: Iterable[LatLonLike], fill_rule: FillRule = ...) -> Polygon: ...
    @property
    def vertices(self) -> list[LatLon]: ...
    @property
    def fill_rule(self) -> FillRule: ...
    def signed_angular_offset_deg(self, point: LatLonLike) -> float: ...
    def __repr__(self) -> str: ...

class Rectangle:
    """A latitude/longitude box, whose north and south edges follow their parallels exactly."""
    def __new__(cls, south_west: LatLonLike, north_east: LatLonLike) -> Rectangle: ...
    @staticmethod
    def latitude_band(south_deg: float, north_deg: float) -> Rectangle: ...
    @property
    def latitudes_deg(self) -> tuple[float, float]: ...
    @property
    def longitudes_deg(self) -> tuple[float, float]: ...
    def signed_angular_offset_deg(self, point: LatLonLike) -> float: ...
    def __repr__(self) -> str: ...

class Ellipse:
    """An ellipse on Earth's surface, given as a centre, angular semi-axes and a bearing."""
    def __new__(cls, centre: LatLonLike, semi_major_deg: float, semi_minor_deg: float, bearing_deg: float = 0.0) -> Ellipse: ...
    @staticmethod
    def circle(centre: LatLonLike, radius_deg: float) -> Ellipse: ...
    @property
    def centre(self) -> LatLon: ...
    @property
    def semi_major_deg(self) -> float: ...
    @property
    def semi_minor_deg(self) -> float: ...
    @property
    def bearing_deg(self) -> float: ...
    @property
    def foci(self) -> tuple[LatLon, LatLon]: ...
    def signed_angular_offset_deg(self, point: LatLonLike) -> float: ...
    def __repr__(self) -> str: ...

# A region on the ground, accepted by Predictor.aoi_iter and detect_aoi.
Area: TypeAlias = Polygon | Rectangle | Ellipse

class Predictor:
    @property
    def epoch(self) -> datetime: ...
    def __new__(cls, elements: Elements) -> Predictor: ...
    @staticmethod
    def from_tle(tle: Tle) -> Predictor: ...
    def with_refinement(self, refinement: Refinement) -> Predictor: ...
    def propagate(self, t: datetime) -> StateVectorTeme: ...
    def observe_at(self, t: datetime, observer: GroundObserver) -> Observation: ...
    def sub_point(self, t: datetime) -> Geodetic: ...
    def prediction_iter(self, interval: IntervalRange, step: timedelta) -> PredictionIter: ...
    def observation_iter(self, observer: GroundObserver, interval: IntervalRange, step: timedelta) -> ObservationIter: ...
    def ground_track_iter(self, interval: IntervalRange, step: timedelta) -> GroundTrackIter: ...
    def transits_iter(self, observer: GroundObserver, interval: IntervalRange, min_elevation_deg: float) -> TransitIter: ...
    def apsis_iter(self, interval: IntervalRange) -> ApsisIter: ...
    def illumination_iter(self, interval: IntervalRange) -> IlluminationIter: ...
    def aoi_iter(self, area: Area, interval: IntervalRange, *, min_step: timedelta | None = None, max_window_duration: timedelta | None = None) -> AoiIter: ...
    def detect_transit(self, t: datetime, observer: GroundObserver, min_elevation_deg: float) -> Transit | None: ...
    def detect_aoi(self, t: datetime, area: Area, *, max_window_duration: timedelta | None = None) -> AoiWindow | None: ...
    def max_elevation(self, observer: GroundObserver, interval: IntervalRange) -> tuple[datetime, Observation]: ...
    def illumination_state(self, t: datetime) -> IlluminationState: ...
    def tle_age_seconds(self, now: datetime) -> float: ...

__all__ = [
    "AoiIter",
    "AoiWindow",
    "Apsis",
    "ApsisEvent",
    "ApsisIter",
    "Area",
    "Classification",
    "Elements",
    "Ellipse",
    "FillRule",
    "Geodetic",
    "GroundObserver",
    "GroundTrackIter",
    "Illumination",
    "IlluminationIter",
    "IlluminationState",
    "Interval",
    "IntervalRange",
    "LatLon",
    "LatLonLike",
    "Observation",
    "ObservationIter",
    "Polygon",
    "PredictionIter",
    "Predictor",
    "Rectangle",
    "Refinement",
    "StateVectorEcef",
    "StateVectorEnu",
    "StateVectorTeme",
    "Tle",
    "Transit",
    "TransitIter",
    "Vec3",
]
