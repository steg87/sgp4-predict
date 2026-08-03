from datetime import datetime
from typing import Protocol, Union, runtime_checkable

from sgp4_predict._sgp4_predict import (
    AoiIter,
    AoiWindow,
    Apsis,
    ApsisEvent,
    ApsisIter,
    Classification,
    Elements,
    Ellipse,
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
    Polygon,
    PredictionIter,
    Predictor,
    Rectangle,
    Refinement,
    StateVectorEcef,
    StateVectorEnu,
    StateVectorTeme,
    Tle,
    Transit,
    TransitIter,
    Vec3,
)

#: A region on the ground, accepted by `Predictor.aoi_iter` and `detect_aoi`.
Area = Union[Polygon, Rectangle, Ellipse]

#: Anywhere a point is taken, a `(latitude_deg, longitude_deg)` tuple works too.
LatLonLike = Union[LatLon, Geodetic, tuple[float, float]]


@runtime_checkable
class IntervalRange(Protocol):
    @property
    def start(self) -> datetime: ...

    @property
    def end(self) -> datetime: ...


class Interval:
    """Concrete datetime interval satisfying IntervalRange."""

    def __init__(self, start: datetime, end: datetime) -> None:
        self._start = start
        self._end = end

    @property
    def start(self) -> datetime:
        return self._start

    @property
    def end(self) -> datetime:
        return self._end

    def __repr__(self) -> str:
        return f"Interval(start={self._start}, end={self._end})"


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
