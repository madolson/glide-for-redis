use glide_core::start_socket_listener;
use pyo3::exceptions::PyUnicodeDecodeError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyList, PySet};
use pyo3::Python;

use redis::Value;

pub const DEFAULT_TIMEOUT_IN_MILLISECONDS: u32 =
    glide_core::client::DEFAULT_RESPONSE_TIMEOUT.as_millis() as u32;

#[pyclass]
#[derive(PartialEq, Eq, PartialOrd, Clone)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

#[allow(dead_code)]
#[pymethods]
impl Level {
    fn is_lower(&self, level: &Level) -> bool {
        self <= level
    }
}

/// A Python module implemented in Rust.
#[pymodule]
fn glide(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Level>()?;
    m.add(
        "DEFAULT_TIMEOUT_IN_MILLISECONDS",
        DEFAULT_TIMEOUT_IN_MILLISECONDS,
    )?;

    #[pyfn(m)]
    fn py_log(log_level: Level, log_identifier: String, message: String) {
        log(log_level, log_identifier, message);
    }

    #[pyfn(m)]
    fn py_init(level: Option<Level>, file_name: Option<&str>) -> Level {
        init(level, file_name)
    }

    #[pyfn(m)]
    fn start_socket_listener_external(init_callback: PyObject) -> PyResult<PyObject> {
        start_socket_listener(move |socket_path| {
            Python::with_gil(|py| {
                match socket_path {
                    Ok(path) => {
                        let _ = init_callback.call(py, (path, py.None()), None);
                    }
                    Err(error_message) => {
                        let _ = init_callback.call(py, (py.None(), error_message), None);
                    }
                };
            });
        });
        Ok(Python::with_gil(|py| "OK".into_py(py)))
    }

    fn iter_to_value<TIterator>(
        py: Python,
        iter: impl IntoIterator<Item = Value, IntoIter = TIterator>,
    ) -> PyResult<Vec<PyObject>>
    where
        TIterator: ExactSizeIterator<Item = Value>,
    {
        let mut iterator = iter.into_iter();
        let len = iterator.len();

        iterator.try_fold(Vec::with_capacity(len), |mut acc, val| {
            acc.push(redis_value_to_py(py, val)?);
            Ok(acc)
        })
    }

    fn redis_value_to_py(py: Python, val: Value) -> PyResult<PyObject> {
        match val {
            Value::Nil => Ok(py.None()),
            Value::SimpleString(str) => Ok(str.into_py(py)),
            Value::Okay => Ok("OK".into_py(py)),
            Value::Int(num) => Ok(num.into_py(py)),
            Value::BulkString(data) => match std::str::from_utf8(data.as_ref()) {
                Ok(val) => Ok(val.into_py(py)),
                Err(_err) => Err(PyUnicodeDecodeError::new_err(data)),
            },
            Value::Array(bulk) => {
                let elements: &PyList = PyList::new(py, iter_to_value(py, bulk)?);
                Ok(elements.into_py(py))
            }
            Value::Map(map) => {
                let dict = PyDict::new(py);
                for (key, value) in map {
                    dict.set_item(redis_value_to_py(py, key)?, redis_value_to_py(py, value)?)?;
                }
                Ok(dict.into_py(py))
            }
            Value::Attribute { data, attributes } => {
                let dict = PyDict::new(py);
                let value = redis_value_to_py(py, *data)?;
                let attributes = redis_value_to_py(py, Value::Map(attributes))?;
                dict.set_item("value", value)?;
                dict.set_item("attributes", attributes)?;
                Ok(dict.into_py(py))
            }
            Value::Set(set) => {
                let set = iter_to_value(py, set)?;
                let set = PySet::new(py, set.iter())?;
                Ok(set.into_py(py))
            }
            Value::Double(double) => Ok(PyFloat::new(py, double.into()).into_py(py)),
            Value::Boolean(boolean) => Ok(PyBool::new(py, boolean).into_py(py)),
            Value::VerbatimString { format: _, text } => Ok(text.into_py(py)),
            Value::BigNumber(bigint) => Ok(bigint.into_py(py)),
            Value::Push { kind: _, data: _ } => todo!(),
        }
    }

    #[pyfn(m)]
    pub fn value_from_pointer(py: Python, pointer: u64) -> PyResult<PyObject> {
        let value = unsafe { Box::from_raw(pointer as *mut Value) };
        redis_value_to_py(py, *value)
    }

    #[pyfn(m)]
    /// This function is for tests that require a value allocated on the heap.
    /// Should NOT be used in production.
    pub fn create_leaked_value(message: String) -> usize {
        let value = Value::SimpleString(message);
        Box::leak(Box::new(value)) as *mut Value as usize
    }
    Ok(())
}

impl From<logger_core::Level> for Level {
    fn from(level: logger_core::Level) -> Self {
        match level {
            logger_core::Level::Error => Level::Error,
            logger_core::Level::Warn => Level::Warn,
            logger_core::Level::Info => Level::Info,
            logger_core::Level::Debug => Level::Debug,
            logger_core::Level::Trace => Level::Trace,
        }
    }
}

impl From<Level> for logger_core::Level {
    fn from(level: Level) -> logger_core::Level {
        match level {
            Level::Error => logger_core::Level::Error,
            Level::Warn => logger_core::Level::Warn,
            Level::Info => logger_core::Level::Info,
            Level::Debug => logger_core::Level::Debug,
            Level::Trace => logger_core::Level::Trace,
        }
    }
}

#[pyfunction]
pub fn log(log_level: Level, log_identifier: String, message: String) {
    logger_core::log(log_level.into(), log_identifier, message);
}

#[pyfunction]
pub fn init(level: Option<Level>, file_name: Option<&str>) -> Level {
    let logger_level = logger_core::init(level.map(|level| level.into()), file_name);
    logger_level.into()
}
