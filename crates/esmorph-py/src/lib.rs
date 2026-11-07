//! Python module `esmorph._esmorph`

use pyo3::prelude::*;

/// Version of the crate as PEP 440 writes it: `0.1.0-dev.12` is `0.1.0.dev12`
fn package_version() -> String {
    env!("CARGO_PKG_VERSION").replace("-dev.", ".dev")
}

#[pymodule]
fn _esmorph(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", package_version())?;
    Ok(())
}
