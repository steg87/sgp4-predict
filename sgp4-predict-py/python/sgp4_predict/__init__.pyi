# ruff: noqa: E501, F401, F403, F405

from datetime import datetime
from typing import Protocol, TypeAlias, runtime_checkable

from sgp4_predict._sgp4_predict import *
from sgp4_predict._sgp4_predict import (
    Ellipse,
    Geodetic,
    LatLon,
    Polygon,
    Rectangle,
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

# A region on the ground, accepted by Predictor.aoi_iter and detect_aoi.
Area: TypeAlias = Polygon | Rectangle | Ellipse

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
