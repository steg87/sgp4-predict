fn main() -> pyo3_stub_gen::Result<()> {
    let stub = sgp4_predict_py::stub_info()?;
    stub.generate()?;
    Ok(())
}
