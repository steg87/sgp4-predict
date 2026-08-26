from datetime import datetime
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
    Interval,
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
