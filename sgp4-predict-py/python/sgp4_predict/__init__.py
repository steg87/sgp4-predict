from datetime import datetime, timedelta
from typing import Protocol, runtime_checkable

from sgp4_predict._sgp4_predict import (
    AoiIter,
    AoiWindow,
    Apsis,
    ApsisEvent,
    ApsisIter,
    Circle,
    Classification,
    Coverage,
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
Area = Polygon | Rectangle | Circle

#: Anywhere a point is taken, a `(latitude_deg, longitude_deg)` tuple works too.
LatLonLike = LatLon | Geodetic | tuple[float, float]


@runtime_checkable
class IntervalRange(Protocol):
    @property
    def start(self) -> datetime: ...

    @property
    def end(self) -> datetime: ...


class _IntervalMixin:
    """Derived quantities shared by everything satisfying `IntervalRange`."""

    @property
    def duration(self) -> timedelta:
        """Length of the interval."""
        return self.end - self.start

    @property
    def mid_point(self) -> datetime:
        """The instant halfway between `start` and `end`."""
        return self.start + self.duration / 2

    def intersection(self, other: IntervalRange) -> "Interval | None":
        """The overlap with `other`, or None if the two are disjoint."""
        start = max(self.start, other.start)
        end = min(self.end, other.end)
        return Interval(start, end) if start < end else None


class Interval(_IntervalMixin):
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


# The window classes come from Rust and cannot gain a base class after the fact,
# but pyo3 builds them as heap types, so the mixin's members graft on directly.
for _cls in (AoiWindow, Illumination, Transit):
    for _name, _member in vars(_IntervalMixin).items():
        if not _name.startswith("_"):
            setattr(_cls, _name, _member)


__all__ = [
    "AoiIter",
    "AoiWindow",
    "Apsis",
    "ApsisEvent",
    "ApsisIter",
    "Area",
    "Circle",
    "Classification",
    "Coverage",
    "Elements",
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
