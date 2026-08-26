# ruff: noqa: E501, F401, F403, F405

from datetime import datetime
from typing import Protocol, TypeAlias, runtime_checkable

from sgp4_predict._sgp4_predict import *
from sgp4_predict._sgp4_predict import (
    Circle,
    Coverage,
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

# A LatLon, a Geodetic, or a plain (latitude_deg, longitude_deg) tuple.
LatLonLike: TypeAlias = LatLon | Geodetic | tuple[float, float]

# A region on the ground, accepted by Predictor.aoi_iter and detect_aoi.
Area: TypeAlias = Polygon | Rectangle | Circle

__all__ = [
    "AoiIter",
    "AoiWindow",
    "Apsis",
    "ApsisEvent",
    "ApsisIter",
    "Area",
    "Classification",
    "Elements",
    "Circle",
    "Coverage",
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
