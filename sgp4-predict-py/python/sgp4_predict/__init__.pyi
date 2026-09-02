
from datetime import datetime
from typing import Protocol, TypeAlias, runtime_checkable

from sgp4_predict._sgp4_predict import *
from sgp4_predict._sgp4_predict import (
    Circle,
    Coverage,
    GeodeticPoint,
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

# A LatLon, a GeodeticPoint, or a plain (latitude_deg, longitude_deg) tuple.
LatLonLike: TypeAlias = LatLon | GeodeticPoint | tuple[float, float]

# A region on the ground, accepted by Predictor.aoi_iter and detect_aoi.
Area: TypeAlias = Polygon | Rectangle | Circle

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
    "GeodeticPoint",
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
    "Pointing",
    "Polygon",
    "PredictionIter",
    "Predictor",
    "Rectangle",
    "Refinement",
    "StateVectorEcef",
    "StateVectorEnu",
    "StateVectorLvlh",
    "StateVectorTeme",
    "Tle",
    "Transit",
    "TransitIter",
    "Vec3",
]
