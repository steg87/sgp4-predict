from datetime import datetime
from typing import Protocol, runtime_checkable

from sgp4_predict._sgp4_predict import (
    Apsis,
    ApsisEvent,
    ApsisIter,
    Classification,
    Elements,
    GroundObserver,
    Illumination,
    IlluminationIter,
    IlluminationState,
    Observation,
    ObservationIter,
    PredictionIter,
    Predictor,
    Refinement,
    StateVectorEcef,
    StateVectorEnu,
    StateVectorTeme,
    Tle,
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
    "Apsis",
    "ApsisEvent",
    "ApsisIter",
    "Classification",
    "Elements",
    "GroundObserver",
    "Illumination",
    "IlluminationIter",
    "IlluminationState",
    "Interval",
    "IntervalRange",
    "Observation",
    "ObservationIter",
    "PredictionIter",
    "Predictor",
    "Refinement",
    "StateVectorEcef",
    "StateVectorEnu",
    "StateVectorTeme",
    "Tle",
    "Transit",
    "TransitIter",
    "Vec3",
]
