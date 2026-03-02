use pyo3::prelude::*;
use pyo3_stub_gen::define_stub_info_gatherer;

mod errors;
mod predictor;
mod satellite;
mod types;
mod vectors;

use predictor::{
    ApsisIter, IlluminationIter, ObservationIter, PredictionIter, Predictor, Refinement,
    TransitIter,
};
use satellite::{GroundStation, Satellite};
use types::{Apsis, ApsisEvent, Illumination, IlluminationState, Observation, Transit};
use vectors::{PyVec3, StateVectorEcef, StateVectorEnu, StateVectorTeme};

define_stub_info_gatherer!(stub_info);

// Re-export all public symbols into the top-level sgp4_predict package stub.
// Without this, the generated sgp4_predict/__init__.pyi has __all__ = [] and
// type checkers (Pylance, mypy) cannot resolve `from sgp4_predict import X`.
pyo3_stub_gen::reexport_module_members!("sgp4_predict", "sgp4_predict._sgp4_predict");

#[pymodule]
fn _sgp4_predict(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Satellite>()?;
    m.add_class::<GroundStation>()?;
    m.add_class::<PyVec3>()?;
    m.add_class::<StateVectorTeme>()?;
    m.add_class::<StateVectorEcef>()?;
    m.add_class::<StateVectorEnu>()?;
    m.add_class::<Observation>()?;
    m.add_class::<Transit>()?;
    m.add_class::<ApsisEvent>()?;
    m.add_class::<Apsis>()?;
    m.add_class::<IlluminationState>()?;
    m.add_class::<Illumination>()?;
    m.add_class::<Predictor>()?;
    m.add_class::<PredictionIter>()?;
    m.add_class::<ApsisIter>()?;
    m.add_class::<IlluminationIter>()?;
    m.add_class::<TransitIter>()?;
    m.add_class::<ObservationIter>()?;
    m.add_class::<Refinement>()?;
    Ok(())
}
