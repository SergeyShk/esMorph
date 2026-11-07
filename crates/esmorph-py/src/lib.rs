//! Python module `esmorph._esmorph`

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pyo3::exceptions::{PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString, PyType};

/// How many words `analyze_many` and `lemmatize` analyze at a time without the GIL: results are
/// converted and Ctrl+C is checked between the chunks
const CHUNK: usize = 8192;

/// Part of speech and features of a UD tag as text, shared by all analyses with this tag
#[derive(PartialEq, Eq, Hash)]
struct Tag {
    pos: String,
    feats: String,
}

impl Tag {
    fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.feats
            .split('|')
            .filter_map(|feat| feat.split_once('='))
    }
}

/// Analysis of a word form
///
/// Attributes:
///     word (str): The word form as the dictionary spells it, with its accents
///     lemma (str): The lemma
///     pos (str): The part of speech of Universal Dependencies
///     feats (dict[str, str]): The Universal Dependencies features
///     prob (float): Share of the analysis among the analyses of the word, by the counts of
///         treebanks; equal for a word they did not see
///     source (str): Where the analysis comes from: "dictionary"
///
/// Example:
///     >>> from esmorph import Analyzer
///     >>> analysis = Analyzer().analyze("camion")[0]
///     >>> analysis.word, analysis.lemma, analysis.pos, analysis.feats
///     ('camión', 'camión', 'NOUN', {'Gender': 'Masc', 'Number': 'Sing'})
#[pyclass(frozen, module = "esmorph")]
struct Analysis {
    #[pyo3(get)]
    word: String,
    #[pyo3(get)]
    lemma: String,
    #[pyo3(get)]
    prob: f64,
    #[pyo3(get)]
    source: &'static str,
    tag: Arc<Tag>,
}

#[pymethods]
impl Analysis {
    #[getter]
    fn pos(&self) -> &str {
        &self.tag.pos
    }

    #[getter]
    fn feats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let feats = PyDict::new(py);
        for (name, value) in self.tag.pairs() {
            feats.set_item(name, value)?;
        }
        Ok(feats)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Analysis(word={}, lemma={}, pos={}, feats={}, prob={:.6}, source={})",
            py_repr(py, &self.word)?,
            py_repr(py, &self.lemma)?,
            py_repr(py, &self.tag.pos)?,
            self.feats(py)?.repr()?,
            self.prob,
            py_repr(py, self.source)?
        ))
    }

    fn __eq__(&self, other: &Bound<'_, Self>) -> bool {
        let other = other.get();
        self.word == other.word
            && self.lemma == other.lemma
            && self.tag == other.tag
            && self.prob == other.prob
            && self.source == other.source
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        (
            &self.word,
            &self.lemma,
            &self.tag,
            self.prob.to_bits(),
            self.source,
        )
            .hash(&mut hasher);
        hasher.finish()
    }

    fn __reduce__<'py>(&self, py: Python<'py>) -> PyResult<(Bound<'py, PyAny>, AnalysisState)> {
        let restore = py.import("esmorph._esmorph")?.getattr("_analysis")?;
        Ok((
            restore,
            (
                self.word.clone(),
                self.lemma.clone(),
                self.tag.pos.clone(),
                self.tag.feats.clone(),
                self.prob,
                self.source.to_owned(),
            ),
        ))
    }
}

/// Arguments of `_analysis`: the fields of an analysis for pickle
type AnalysisState = (String, String, String, String, f64, String);

/// Restores an analysis from pickle
#[pyfunction]
fn _analysis(
    word: String,
    lemma: String,
    pos: String,
    feats: String,
    prob: f64,
    source: &str,
) -> PyResult<Analysis> {
    let source = match source {
        "dictionary" => esmorph::Source::Dictionary.as_str(),
        _ => {
            return Err(PyValueError::new_err(format!(
                "Unknown source of an analysis: {source}"
            )));
        }
    };
    Ok(Analysis {
        word,
        lemma,
        prob,
        source,
        tag: Arc::new(Tag { pos, feats }),
    })
}

/// Morphological analyzer of Spanish
///
/// Description:
///     Finds the analyses of a word form in the dictionary, the most probable first. Case does not
///     matter. A word spelled as in the dictionary gives the analyses of that spelling; a word
///     absent as written is looked for without its accents and with a lost diaeresis. The dictionary
///     is loaded once when the analyzer is created; an analyzer can be used from several threads
///
/// Arguments:
///     path (str | os.PathLike | None): Path to a dictionary file; by default the dictionary of the
///         package
///
/// Raises:
///     FileNotFoundError: If there is no dictionary file
///     OSError: If the dictionary file cannot be read
///     ValueError: If the file is no dictionary of esMorph or is damaged
///
/// Example:
///     >>> from esmorph import Analyzer
///     >>> morph = Analyzer()
///     >>> sorted({analysis.lemma for analysis in morph.analyze("hablamos")})
///     ['hablar']
#[pyclass(frozen, module = "esmorph")]
struct Analyzer {
    dictionary: Arc<esmorph::Dictionary>,
    tags: Vec<Arc<Tag>>,
    path: PathBuf,
    /// The path was given: otherwise pickle does not keep it, and another environment takes the
    /// dictionary of its own package
    explicit: bool,
}

#[pymethods]
impl Analyzer {
    #[new]
    #[pyo3(signature = (path=None))]
    fn new(py: Python<'_>, path: Option<PathBuf>) -> PyResult<Self> {
        let explicit = path.is_some();
        let path = match path {
            Some(path) => path,
            None => default_path(py)?,
        };
        let dictionary = py
            .detach(|| esmorph::Dictionary::open(&path))
            .map_err(|err| load_error(&path, err))?;
        let tags = dictionary
            .ud_tags()
            .map(|tag| {
                Arc::new(Tag {
                    pos: tag.pos().to_owned(),
                    feats: tag.feats_string(),
                })
            })
            .collect();
        Ok(Analyzer {
            dictionary: Arc::new(dictionary),
            tags,
            path,
            explicit,
        })
    }

    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> (Bound<'py, PyType>, (Option<PathBuf>,)) {
        let analyzer = slf.get();
        let path = analyzer.explicit.then(|| analyzer.path.clone());
        (slf.get_type(), (path,))
    }

    /// Path to the dictionary file
    #[getter]
    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    /// Analyses of a word form
    ///
    /// Description:
    ///     A word that is not in the dictionary has no analyses
    ///
    /// Arguments:
    ///     word (str): The word form
    ///
    /// Returns:
    ///     list[Analysis]: The analyses
    ///
    /// Raises:
    ///     TypeError: If the word is not a string
    ///
    /// Example:
    ///     >>> from esmorph import Analyzer
    ///     >>> morph = Analyzer()
    ///     >>> [(analysis.lemma, analysis.pos) for analysis in morph.analyze("CAMIONES")]
    ///     [('camión', 'NOUN')]
    ///     >>> morph.analyze("qzxw")
    ///     []
    fn analyze(&self, word: &Bound<'_, PyAny>) -> PyResult<Vec<Analysis>> {
        let word = text(word)?;
        Ok(self.convert(self.dictionary.analyze(&word)))
    }

    /// Analyses of a list of word forms
    ///
    /// Description:
    ///     The same as analyze for every word form; the analysis goes without the GIL, so several
    ///     threads analyze their lists in parallel
    ///
    /// Arguments:
    ///     words (Iterable[str]): The word forms
    ///
    /// Returns:
    ///     list[list[Analysis]]: The analyses of every word form
    ///
    /// Raises:
    ///     TypeError: If a string is given instead of a collection of words, or a word is not a string
    ///
    /// Example:
    ///     >>> from esmorph import Analyzer
    ///     >>> analyses = Analyzer().analyze_many(["camiones", "qzxw"])
    ///     >>> [analysis.lemma for analysis in analyses[0]], analyses[1]
    ///     (['camión'], [])
    fn analyze_many(
        &self,
        py: Python<'_>,
        words: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<Vec<Analysis>>> {
        let words = collect_words(words)?;
        let mut result = Vec::with_capacity(words.len());
        for chunk in words.chunks(CHUNK) {
            let analyses = py.detach(|| {
                chunk
                    .iter()
                    .map(|word| self.dictionary.analyze(word))
                    .collect::<Vec<_>>()
            });
            result.extend(analyses.into_iter().map(|analyses| self.convert(analyses)));
            py.check_signals()?;
        }
        Ok(result)
    }

    /// Lemmas of a list of word forms
    ///
    /// Description:
    ///     The lemma of the first analysis; a word that is not in the dictionary comes back in
    ///     lower case. The analysis goes without the GIL
    ///
    /// Arguments:
    ///     words (Iterable[str]): The word forms
    ///
    /// Returns:
    ///     list[str]: The lemmas
    ///
    /// Raises:
    ///     TypeError: If a string is given instead of a collection of words, or a word is not a string
    ///
    /// Example:
    ///     >>> from esmorph import Analyzer
    ///     >>> Analyzer().lemmatize(["CAMIONES", "Qzxw"])
    ///     ['camión', 'qzxw']
    fn lemmatize(&self, py: Python<'_>, words: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyString>>> {
        let words = collect_words(words)?;
        // Equal lemmas are one string object: a list of the lemmas of a corpus takes much less memory
        let mut strings: HashMap<String, Py<PyString>> = HashMap::new();
        let mut result = Vec::with_capacity(words.len());
        for chunk in words.chunks(CHUNK) {
            let lemmas = py.detach(|| {
                chunk
                    .iter()
                    .map(
                        |word| match self.dictionary.analyze(word).into_iter().next() {
                            Some(analysis) => analysis.lemma,
                            None => word.to_lowercase(),
                        },
                    )
                    .collect::<Vec<_>>()
            });
            for lemma in lemmas {
                let object = match strings.get(&lemma) {
                    Some(object) => object.clone_ref(py),
                    None => {
                        let object = PyString::new(py, &lemma).unbind();
                        strings.insert(lemma, object.clone_ref(py));
                        object
                    }
                };
                result.push(object);
            }
            py.check_signals()?;
        }
        Ok(result)
    }

    /// Whether the word form is in the dictionary
    ///
    /// Arguments:
    ///     word (str): The word form
    ///
    /// Returns:
    ///     bool: Whether analyze finds the word form in the dictionary
    ///
    /// Raises:
    ///     TypeError: If the word is not a string
    ///
    /// Example:
    ///     >>> from esmorph import Analyzer
    ///     >>> morph = Analyzer()
    ///     >>> morph.is_known("Camión"), morph.is_known("camion"), morph.is_known("qzxw")
    ///     (True, True, False)
    fn is_known(&self, word: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.dictionary.is_known(&text(word)?))
    }

    /// Description of the source of the dictionary in JSON (for checking the dictionary in tests)
    fn _meta(&self) -> &str {
        self.dictionary.meta()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Analyzer(path={})",
            py_repr(py, &self.path.display().to_string())?
        ))
    }
}

impl Analyzer {
    fn convert(&self, analyses: Vec<esmorph::Analysis>) -> Vec<Analysis> {
        analyses
            .into_iter()
            .filter_map(|analysis| {
                Some(Analysis {
                    tag: Arc::clone(self.tags.get(usize::from(analysis.ud))?),
                    source: analysis.source.as_str(),
                    prob: analysis.prob,
                    word: analysis.word,
                    lemma: analysis.lemma,
                })
            })
            .collect()
    }
}

/// Version of the crate as PEP 440 writes it: `0.1.0-dev.12` is `0.1.0.dev12`
fn package_version() -> String {
    env!("CARGO_PKG_VERSION").replace("-dev.", ".dev")
}

fn py_repr(py: Python<'_>, value: &str) -> PyResult<String> {
    Ok(PyString::new(py, value).repr()?.to_string())
}

/// An error of reading a file is an OSError with errno and the file name (Python picks the subclass
/// by errno), a wrong format is a ValueError
fn load_error(path: &Path, err: esmorph::Error) -> PyErr {
    let esmorph::Error::Io(err) = err else {
        return PyValueError::new_err(format!("{}: {err}", path.display()));
    };
    let message = match err.kind() {
        ErrorKind::NotFound => "No dictionary file".to_owned(),
        // Windows reports a directory as a lack of access
        ErrorKind::IsADirectory => "The path to the dictionary is a directory".to_owned(),
        ErrorKind::PermissionDenied if path.is_dir() => {
            "The path to the dictionary is a directory".to_owned()
        }
        ErrorKind::PermissionDenied => "No access to the dictionary file".to_owned(),
        _ => err.to_string(),
    };
    let filename = path.display().to_string();
    match err.raw_os_error() {
        #[cfg(windows)]
        Some(code) => PyOSError::new_err((0, message, filename, code)),
        #[cfg(not(windows))]
        Some(errno) => PyOSError::new_err((errno, message, filename)),
        None => PyOSError::new_err(format!("{message}: {filename}")),
    }
}

fn default_path(py: Python<'_>) -> PyResult<PathBuf> {
    let init: PathBuf = py.import("esmorph")?.getattr("__file__")?.extract()?;
    Ok(init.with_file_name("data").join("es.dict"))
}

fn collect_words(words: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    if words.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "Expected a collection of words, not a string: pass a list of words",
        ));
    }
    let iterator = words.try_iter().map_err(|err| {
        if err.is_instance_of::<PyTypeError>(words.py()) {
            type_error("Expected a collection of words", words)
        } else {
            err
        }
    })?;
    iterator
        .enumerate()
        .map(|(number, word)| {
            let word = word?;
            let Ok(word) = word.cast::<PyString>() else {
                return Err(PyTypeError::new_err(format!(
                    "Every word must be a string: item {number} is {}",
                    word.get_type().name()?
                )));
            };
            lossy(word)
        })
        .collect()
}

/// A Python string as a Rust string: a lone surrogate (`surrogateescape`, `os.fsdecode`) is U+FFFD
fn lossy(word: &Bound<'_, PyString>) -> PyResult<String> {
    if let Ok(text) = word.to_str() {
        return Ok(text.to_owned());
    }
    let encoded = word.call_method1("encode", ("utf-16-le", "surrogatepass"))?;
    let bytes = encoded.extract::<Vec<u8>>()?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    Ok(String::from_utf16_lossy(&units))
}

/// A word as a Rust string; lone surrogates are replaced with U+FFFD
fn text(word: &Bound<'_, PyAny>) -> PyResult<String> {
    let Ok(word) = word.cast::<PyString>() else {
        return Err(type_error("A word must be a string", word));
    };
    lossy(word)
}

fn type_error(message: &str, value: &Bound<'_, PyAny>) -> PyErr {
    let name = value
        .get_type()
        .name()
        .map_or_else(|_| "?".into(), |name| name.to_string());
    PyTypeError::new_err(format!("{message}, not {name}"))
}

#[pymodule]
fn _esmorph(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Analyzer>()?;
    m.add_class::<Analysis>()?;
    m.add_function(wrap_pyfunction!(_analysis, m)?)?;
    m.add("__version__", package_version())?;
    Ok(())
}
