# ruff: noqa: E501, F401, F403, F405

from datetime import datetime, timedelta
from typing import Protocol, runtime_checkable

from sgp4_predict._sgp4_predict import *
from sgp4_predict._sgp4_predict import (
    Apsis,
    ApsisEvent,
    ApsisIter,
    Classification,
    Elements,
    Observer,
    Illumination,
    IlluminationIter,
    IlluminationState,
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

class Predictor:
    @property
    def epoch(self) -> datetime: ...
    def __new__(cls, elements: Elements) -> Predictor: ...
    @staticmethod
    def from_tle(sat: Tle) -> Predictor: ...
    def with_refinement(self, refinement: Refinement) -> Predictor: ...
    def propagate(self, t: datetime) -> StateVectorTeme: ...
    def observe_at(self, t: datetime, observer: Observer) -> Observation: ...
    def prediction_iter(self, interval: IntervalRange, step: timedelta) -> PredictionIter: ...
    def observation_iter(self, observer: Observer, interval: IntervalRange, step: timedelta) -> ObservationIter: ...
    def transits_iter(self, observer: Observer, interval: IntervalRange, min_elevation_deg: float) -> TransitIter: ...
    def apsis_iter(self, interval: IntervalRange) -> ApsisIter: ...
    def illumination_iter(self, interval: IntervalRange) -> IlluminationIter: ...
    def detect_transit(self, t: datetime, observer: Observer, min_elevation_deg: float) -> Transit | None: ...
    def max_elevation(self, observer: Observer, interval: IntervalRange) -> tuple[datetime, Observation]: ...
    def illumination_state(self, t: datetime) -> IlluminationState: ...
    def tle_age_seconds(self, now: datetime) -> float: ...

__all__ = [
    "Apsis",
    "ApsisEvent",
    "ApsisIter",
    "Classification",
    "Elements",
    "Observer",
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
    "Tle",
    "StateVectorEcef",
    "StateVectorEnu",
    "StateVectorTeme",
    "Transit",
    "TransitIter",
    "Vec3",
]
