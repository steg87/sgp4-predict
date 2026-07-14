use pyo3::prelude::*;
use pyo3_stub_gen::define_stub_info_gatherer;

mod elements;
mod errors;
mod observer;
mod predictor;
mod tle;
mod types;
mod vectors;

use elements::{Classification, Elements};
use observer::GroundObserver;
use predictor::{
    ApsisIter, IlluminationIter, ObservationIter, PoleApproachIter, PredictionIter, Predictor,
    Refinement, TransitIter,
};
use tle::Tle;
use types::{
    Apsis, ApsisEvent, Illumination, IlluminationState, Observation, PoleApproach, PoleEvent,
    Transit,
};
use vectors::{PyVec3, StateVectorEcef, StateVectorEnu, StateVectorTeme};

define_stub_info_gatherer!(stub_info);

// Populate the top-level sgp4_predict module entry so pyo3-stub-gen knows the package structure.
// stub_gen.rs filters this module out before writing stubs; sgp4_predict/__init__.pyi is hand-maintained.
pyo3_stub_gen::reexport_module_members!("sgp4_predict", "sgp4_predict._sgp4_predict");

#[pymodule]
fn _sgp4_predict(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Classification>()?;
    m.add_class::<Elements>()?;
    m.add_class::<Tle>()?;
    m.add_class::<GroundObserver>()?;
    m.add_class::<PyVec3>()?;
    m.add_class::<StateVectorTeme>()?;
    m.add_class::<StateVectorEcef>()?;
    m.add_class::<StateVectorEnu>()?;
    m.add_class::<Observation>()?;
    m.add_class::<Transit>()?;
    m.add_class::<ApsisEvent>()?;
    m.add_class::<Apsis>()?;
    m.add_class::<PoleEvent>()?;
    m.add_class::<PoleApproach>()?;
    m.add_class::<IlluminationState>()?;
    m.add_class::<Illumination>()?;
    m.add_class::<Predictor>()?;
    m.add_class::<PredictionIter>()?;
    m.add_class::<ApsisIter>()?;
    m.add_class::<PoleApproachIter>()?;
    m.add_class::<IlluminationIter>()?;
    m.add_class::<TransitIter>()?;
    m.add_class::<ObservationIter>()?;
    m.add_class::<Refinement>()?;
    Ok(())
}
