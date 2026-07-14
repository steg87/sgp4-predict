use chrono::{DateTime, Duration, Utc};
use ouroboros::self_referencing;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::*;

use crate::{
    elements::Elements,
    errors::to_py_err,
    observer::GroundObserver,
    tle::Tle,
    types::{
        Apsis, ApsisEvent, Illumination, IlluminationState, Observation, PoleApproach, PoleEvent,
        Transit,
    },
    vectors::StateVectorTeme,
};

// ── Refinement ─────────────────────────────────────────────────────────────────

/// Root-finder configuration used by transit detection and peak-elevation search.
///
/// Newton-Raphson is attempted first; Brent's method is the bracketed fallback.
#[gen_stub_pyclass]
#[pyclass(module = "sgp4_predict._sgp4_predict")]
pub struct Refinement {
    pub(crate) inner: sgp4_predict::Refinement,
}

#[gen_stub_pymethods]
#[pymethods]
impl Refinement {
    #[new]
    fn new() -> Self {
        Self {
            inner: sgp4_predict::Refinement::default(),
        }
    }

    /// Newton-Raphson convergence tolerance (radians).
    #[getter]
    fn nr_tolerance(&self) -> f64 {
        self.inner.newton_raphson.tolerance
    }
    #[setter]
    fn set_nr_tolerance(&mut self, v: f64) {
        self.inner.newton_raphson.tolerance = v;
    }

    /// Newton-Raphson maximum iteration count.
    #[getter]
    fn nr_max_iter(&self) -> usize {
        self.inner.newton_raphson.max_iter
    }
    #[setter]
    fn set_nr_max_iter(&mut self, v: usize) {
        self.inner.newton_raphson.max_iter = v;
    }

    /// Brent convergence tolerance.
    #[getter]
    fn brent_tolerance(&self) -> f64 {
        self.inner.brent.tolerance
    }
    #[setter]
    fn set_brent_tolerance(&mut self, v: f64) {
        self.inner.brent.tolerance = v;
    }

    /// Brent maximum iteration count.
    #[getter]
    fn brent_max_iter(&self) -> usize {
        self.inner.brent.max_iter
    }
    #[setter]
    fn set_brent_max_iter(&mut self, v: usize) {
        self.inner.brent.max_iter = v;
    }
}

// ── PredictionIter ─────────────────────────────────────────────────────────────

/// Lazy iterator yielding `(datetime, StateVectorTeme)` at regular intervals.
#[gen_stub_pyclass]
#[pyclass(module = "sgp4_predict._sgp4_predict")]
pub struct PredictionIter {
    inner: sgp4_predict::PredictionIter,
}

#[gen_stub_pymethods]
#[pymethods]
impl PredictionIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> PyResult<Option<(DateTime<Utc>, StateVectorTeme)>> {
        match self.inner.next() {
            None => Ok(None),
            Some(Ok((t, sv))) => Ok(Some((t, StateVectorTeme::from_inner(sv)))),
            Some(Err(e)) => Err(to_py_err(e)),
        }
    }
}

// ── ApsisIter ──────────────────────────────────────────────────────────────────

/// Lazy iterator yielding apogee and perigee events within a time interval.
#[gen_stub_pyclass]
#[pyclass(module = "sgp4_predict._sgp4_predict")]
pub struct ApsisIter {
    inner: sgp4_predict::ApsisIter,
}

#[gen_stub_pymethods]
#[pymethods]
impl ApsisIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> PyResult<Option<Apsis>> {
        match self.inner.next() {
            None => Ok(None),
            Some(Ok(a)) => Ok(Some(Apsis {
                time: a.time,
                event: match a.event {
                    sgp4_predict::ApsisEvent::Apogee => ApsisEvent::Apogee,
                    sgp4_predict::ApsisEvent::Perigee => ApsisEvent::Perigee,
                },
                altitude: a.altitude,
            })),
            Some(Err(e)) => Err(to_py_err(e)),
        }
    }
}

// ── PoleApproachIter ───────────────────────────────────────────────────────────

/// Lazy iterator yielding northernmost and southernmost points within a time interval.
#[gen_stub_pyclass]
#[pyclass(module = "sgp4_predict._sgp4_predict")]
pub struct PoleApproachIter {
    inner: sgp4_predict::PoleApproachIter,
}

#[gen_stub_pymethods]
#[pymethods]
impl PoleApproachIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> PyResult<Option<PoleApproach>> {
        match self.inner.next() {
            None => Ok(None),
            Some(Ok(a)) => Ok(Some(PoleApproach {
                time: a.time,
                event: match a.event {
                    sgp4_predict::PoleEvent::North => PoleEvent::North,
                    sgp4_predict::PoleEvent::South => PoleEvent::South,
                },
                latitude: a.latitude,
            })),
            Some(Err(e)) => Err(to_py_err(e)),
        }
    }
}

// ── IlluminationIter ───────────────────────────────────────────────────────────

/// Lazy iterator yielding sunlit and eclipse windows within a time interval.
#[gen_stub_pyclass]
#[pyclass(module = "sgp4_predict._sgp4_predict")]
pub struct IlluminationIter {
    inner: sgp4_predict::IlluminationIter,
}

#[gen_stub_pymethods]
#[pymethods]
impl IlluminationIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> PyResult<Option<Illumination>> {
        match self.inner.next() {
            None => Ok(None),
            Some(Ok(ill)) => Ok(Some(Illumination {
                start: ill.start,
                end: ill.end,
                state: match ill.state {
                    sgp4_predict::IlluminationState::Sunlit => IlluminationState::Sunlit,
                    sgp4_predict::IlluminationState::Eclipse => IlluminationState::Eclipse,
                },
            })),
            Some(Err(e)) => Err(to_py_err(e)),
        }
    }
}

// ── TransitIter ────────────────────────────────────────────────────────────────

// Self-referential struct: owns the GroundObserver and the TransitIter that borrows it.
#[self_referencing]
struct TransitIterOwned {
    observer: GroundObserver,
    #[borrows(observer)]
    #[covariant]
    iter: sgp4_predict::TransitIter<'this, GroundObserver>,
}

/// Lazy iterator yielding satellite passes visible to an observer.
#[gen_stub_pyclass]
#[pyclass(module = "sgp4_predict._sgp4_predict")]
pub struct TransitIter {
    inner: TransitIterOwned,
}

#[gen_stub_pymethods]
#[pymethods]
impl TransitIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> PyResult<Option<Transit>> {
        self.inner.with_iter_mut(|iter| match iter.next() {
            None => Ok(None),
            Some(Ok(t)) => Ok(Some(Transit {
                start: t.start,
                end: t.end,
            })),
            Some(Err(e)) => Err(to_py_err(e)),
        })
    }
}

// ── ObservationIter ────────────────────────────────────────────────────────────

// Self-referential struct: owns the GroundObserver and the ObservationIter that borrows it.
#[self_referencing]
struct ObservationIterOwned {
    observer: GroundObserver,
    #[borrows(observer)]
    #[covariant]
    iter: sgp4_predict::ObservationIter<'this, GroundObserver>,
}

/// Lazy iterator yielding time-stamped observations at regular intervals.
#[gen_stub_pyclass]
#[pyclass(module = "sgp4_predict._sgp4_predict")]
pub struct ObservationIter {
    inner: ObservationIterOwned,
}

#[gen_stub_pymethods]
#[pymethods]
impl ObservationIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> PyResult<Option<(DateTime<Utc>, Observation)>> {
        self.inner.with_iter_mut(|iter| match iter.next() {
            None => Ok(None),
            Some(Ok((t, obs))) => Ok(Some((t, Observation::from_inner(obs)))),
            Some(Err(e)) => Err(to_py_err(e)),
        })
    }
}

// ── helpers ────────────────────────────────────────────────────────────────────

fn extract_interval(interval: &Bound<'_, PyAny>) -> PyResult<(DateTime<Utc>, DateTime<Utc>)> {
    let start: DateTime<Utc> = interval.getattr("start")?.extract()?;
    let end: DateTime<Utc> = interval.getattr("end")?.extract()?;
    Ok((start, end))
}

// ── Predictor ──────────────────────────────────────────────────────────────────

/// Parsed TLE with pre-computed SGP4 constants, ready for propagation.
///
/// Construct from a `Tle` or `Elements`; then use its methods to propagate state vectors,
/// compute ground observations, detect passes, find apsides, and query illumination.
#[gen_stub_pyclass]
#[pyclass(frozen, module = "sgp4_predict._sgp4_predict")]
pub struct Predictor {
    inner: sgp4_predict::Predictor,
}

#[gen_stub_pymethods]
#[pymethods]
impl Predictor {
    /// Initialise SGP4 constants from pre-parsed orbital elements.
    ///
    /// Pass an `Elements` object — constructed manually, from `Elements.from_json`,
    /// or obtained from `Tle.to_elements`.
    ///
    /// Raises `ValueError` if element initialisation fails.
    #[new]
    fn new(elements: &Elements) -> PyResult<Self> {
        sgp4_predict::Predictor::new(elements.inner.clone())
            .map(|p| Self { inner: p })
            .map_err(to_py_err)
    }

    /// Parse TLE string lines and initialise SGP4 constants.
    ///
    /// Raises `ValueError` if the TLE is malformed.
    #[staticmethod]
    fn from_tle(tle: &Tle) -> PyResult<Self> {
        sgp4_predict::Predictor::from_tle(tle)
            .map(|p| Self { inner: p })
            .map_err(to_py_err)
    }

    /// Return a new `Predictor` with the given root-finder configuration.
    fn with_refinement(&self, refinement: &Refinement) -> Self {
        Self {
            inner: self.inner.clone().with_refinement(refinement.inner),
        }
    }

    /// Propagate the TLE to the given UTC time.
    ///
    /// Returns a state vector in the TEME frame.
    fn propagate(&self, t: DateTime<Utc>) -> PyResult<StateVectorTeme> {
        self.inner
            .propagate(t)
            .map(StateVectorTeme::from_inner)
            .map_err(to_py_err)
    }

    /// Calculate the observation from an observer at the given UTC time.
    fn observe_at(&self, t: DateTime<Utc>, observer: &GroundObserver) -> PyResult<Observation> {
        self.inner
            .observe_at(t, observer)
            .map(Observation::from_inner)
            .map_err(to_py_err)
    }

    /// Iterate over state vectors in the TEME frame at regular intervals.
    ///
    /// `interval` must expose `.start` and `.end` datetime properties.
    /// Pass an `Interval`, `Transit`, or `Illumination` object.
    fn prediction_iter(
        &self,
        interval: &Bound<'_, PyAny>,
        step: Duration,
    ) -> PyResult<PredictionIter> {
        let (start, end) = extract_interval(interval)?;
        Ok(PredictionIter {
            inner: self.inner.prediction_iter(start..end, step),
        })
    }

    /// Iterate over observations from the given observer at regular intervals.
    ///
    /// `interval` must expose `.start` and `.end` datetime properties.
    /// Pass an `Interval`, `Transit`, or `Illumination` object.
    fn observation_iter(
        &self,
        observer: &GroundObserver,
        interval: &Bound<'_, PyAny>,
        step: Duration,
    ) -> PyResult<ObservationIter> {
        let (start, end) = extract_interval(interval)?;
        let obs_clone = observer.clone();
        let predictor = self.inner.clone();
        Ok(ObservationIter {
            inner: ObservationIterOwnedBuilder {
                observer: obs_clone,
                iter_builder: move |obs| predictor.observation_iter(obs, start..end, step),
            }
            .build(),
        })
    }

    /// Iterate over satellite passes visible to the observer within the interval.
    ///
    /// `interval` must expose `.start` and `.end` datetime properties.
    /// `min_elevation_deg`: minimum elevation above the horizon in degrees.
    fn transits_iter(
        &self,
        observer: &GroundObserver,
        interval: &Bound<'_, PyAny>,
        min_elevation_deg: f64,
    ) -> PyResult<TransitIter> {
        let (start, end) = extract_interval(interval)?;
        let obs_clone = observer.clone();
        let predictor = self.inner.clone();
        Ok(TransitIter {
            inner: TransitIterOwnedBuilder {
                observer: obs_clone,
                iter_builder: move |obs| {
                    predictor.transits_iter(obs, start..end, min_elevation_deg)
                },
            }
            .build(),
        })
    }

    /// Iterate over apogee and perigee events within the interval.
    ///
    /// `interval` must expose `.start` and `.end` datetime properties.
    fn apsis_iter(&self, interval: &Bound<'_, PyAny>) -> PyResult<ApsisIter> {
        let (start, end) = extract_interval(interval)?;
        Ok(ApsisIter {
            inner: self.inner.apsis_iter(start..end),
        })
    }

    /// Iterate over northernmost and southernmost points of the orbit within the interval.
    ///
    /// `interval` must expose `.start` and `.end` datetime properties.
    fn pole_approach_iter(&self, interval: &Bound<'_, PyAny>) -> PyResult<PoleApproachIter> {
        let (start, end) = extract_interval(interval)?;
        Ok(PoleApproachIter {
            inner: self.inner.pole_approach_iter(start..end),
        })
    }

    /// Iterate over sunlit and eclipse windows within the interval.
    ///
    /// `interval` must expose `.start` and `.end` datetime properties.
    fn illumination_iter(&self, interval: &Bound<'_, PyAny>) -> PyResult<IlluminationIter> {
        let (start, end) = extract_interval(interval)?;
        Ok(IlluminationIter {
            inner: self.inner.illumination_iter(start..end),
        })
    }

    /// Detect whether a transit is in progress at time `t`.
    ///
    /// Returns `None` if the satellite is below `min_elevation_deg` at `t`.
    /// Searches backward and forward to bracket AoS and LoS.
    fn detect_transit(
        &self,
        t: DateTime<Utc>,
        observer: &GroundObserver,
        min_elevation_deg: f64,
    ) -> PyResult<Option<Transit>> {
        self.inner
            .detect_transit(t, observer, min_elevation_deg)
            .map(|opt| {
                opt.map(|t| Transit {
                    start: t.start,
                    end: t.end,
                })
            })
            .map_err(to_py_err)
    }

    /// Find the peak elevation of the satellite over an observer within the interval.
    ///
    /// `interval` must expose `.start` and `.end` datetime properties.
    /// Returns `(datetime, Observation)` at the peak.
    /// Raises `RuntimeError` if no peak is found in the interval.
    fn max_elevation(
        &self,
        observer: &GroundObserver,
        interval: &Bound<'_, PyAny>,
    ) -> PyResult<(DateTime<Utc>, Observation)> {
        let (start, end) = extract_interval(interval)?;
        self.inner
            .max_elevation(start..end, observer)
            .map(|(t, obs)| (t, Observation::from_inner(obs)))
            .map_err(to_py_err)
    }

    /// Determine whether the satellite is sunlit or in eclipse at time `t`.
    fn illumination_state(&self, t: DateTime<Utc>) -> PyResult<IlluminationState> {
        self.inner
            .illumination_state(t)
            .map(|s| match s {
                sgp4_predict::IlluminationState::Sunlit => IlluminationState::Sunlit,
                sgp4_predict::IlluminationState::Eclipse => IlluminationState::Eclipse,
            })
            .map_err(to_py_err)
    }

    /// The epoch of the TLE (UTC).
    #[getter]
    fn epoch(&self) -> DateTime<Utc> {
        self.inner.epoch()
    }

    /// Age of the TLE relative to `now` in seconds (positive = TLE is in the past).
    fn tle_age_seconds(&self, now: DateTime<Utc>) -> f64 {
        self.inner.tle_age(now).num_milliseconds() as f64 / 1000.0
    }
}
