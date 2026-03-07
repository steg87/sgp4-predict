fn main() -> pyo3_stub_gen::Result<()> {
    let mut stub = sgp4_predict_py::stub_info()?;
    // sgp4_predict/__init__.pyi is hand-maintained (it exposes IntervalRange and typed Predictor).
    // Only generate the internal _sgp4_predict module stub.
    stub.modules.retain(|name, _| name != "sgp4_predict");
    stub.generate()?;
    Ok(())
}
