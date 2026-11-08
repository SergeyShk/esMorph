//! Python module `esmorph._esmorph`

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::{PyLookupError, PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString, PyTuple, PyType};

/// How many words `analyze_many` and `lemmatize` analyze at a time without the GIL: results are
/// converted and Ctrl+C is checked between the chunks
const CHUNK: usize = 8192;

create_exception!(
    esmorph,
    FormNotFoundError,
    PyLookupError,
    "The lexeme has no form with the requested features"
);

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
/// Description:
///     Analyses are immutable and hashable; they are compared without the probability, so an
///     analysis equals its form from forms(). An analysis keeps its analyzer: pickle saves them
///     together
///
/// Attributes:
///     word (str): The word form as the dictionary spells it, with its accents; a verb with
///         clitics as written, in lower case
///     lemma (str): The lemma
///     pos (str): The part of speech of Universal Dependencies
///     feats (dict[str, str]): The Universal Dependencies features
///     prob (float): Share of the analysis among the analyses of the word, by the counts of
///         treebanks; equal for a word they did not see
///     words (tuple[str, ...]): Words of the token: a verb and its clitics, the words of a
///         contraction, or the word itself; the analysis describes the first of them
///     source (str): Where the analysis comes from: "dictionary", "clitics" for a verb of the
///         dictionary with clitics
///
/// Example:
///     >>> from esmorph import Analyzer
///     >>> analysis = Analyzer().analyze("camion")[0]
///     >>> analysis.word, analysis.lemma, analysis.pos, analysis.feats
///     ('camión', 'camión', 'NOUN', {'Gender': 'Masc', 'Number': 'Sing'})
///     >>> analysis = Analyzer().analyze("dámelo")[0]
///     >>> analysis.lemma, analysis.pos, analysis.words
///     ('dar', 'VERB', ('da', 'me', 'lo'))
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
    /// Words of a token of several; empty for one word
    words: Vec<String>,
    /// The analyzer that gave the analysis: its dictionary has the forms of the lexeme
    analyzer: Py<Analyzer>,
}

impl Analysis {
    /// The analysis of the dictionary that this one is: the forms of its lexeme come from it
    fn found(&self) -> Option<esmorph::Analysis> {
        let analyzer = self.analyzer.get();
        analyzer
            .dictionary
            .analyze(&self.word)
            .into_iter()
            .find(|analysis| {
                analysis.lemma == self.lemma
                    && analysis.words == self.words
                    && analysis.source.as_str() == self.source
                    && analyzer
                        .tags
                        .get(usize::from(analysis.ud))
                        .is_some_and(|tag| **tag == *self.tag)
            })
    }
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

    #[getter]
    fn words<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        if self.words.is_empty() {
            PyTuple::new(py, [&self.word])
        } else {
            PyTuple::new(py, &self.words)
        }
    }

    /// Forms of the lexeme
    ///
    /// Description:
    ///     The lemma, then the forms in the order of the paradigm: the infinitive, the gerund,
    ///     the participles, the finite forms by mood, tense, number and person; for a noun or an
    ///     adjective by number and gender. The probability of every form is 1. A verb with
    ///     clitics gives the forms of its verb, without clitics
    ///
    /// Returns:
    ///     list[Analysis]: The forms
    ///
    /// Example:
    ///     >>> from esmorph import Analyzer
    ///     >>> [form.word for form in Analyzer().analyze("gatos")[0].forms()]
    ///     ['gato', 'gata', 'gatos', 'gatas']
    fn forms(&self, py: Python<'_>) -> Vec<Analysis> {
        let Some(found) = self.found() else {
            return Vec::new();
        };
        let analyzer = self.analyzer.get();
        analyzer.convert(py, &self.analyzer, analyzer.dictionary.forms(&found))
    }

    /// The form with the given features
    ///
    /// Description:
    ///     Of the forms of the lexeme with the given UD features, the one that keeps most of the
    ///     other features of the analysis: "gata" in the plural is "gatas". An infinitive or a
    ///     gerund asked for a tense, a mood, a person or a number gives a finite form. A variant
    ///     (a form of vos, an old spelling) comes only from a variant
    ///
    /// Arguments:
    ///     **feats (str): UD features and their values: Number="Plur", Tense="Past"
    ///
    /// Returns:
    ///     str: The word form as the dictionary spells it
    ///
    /// Raises:
    ///     FormNotFoundError: If the lexeme has no form with these features
    ///     ValueError: If the dictionary has no such feature or value
    ///     TypeError: If a value is not a string
    ///
    /// Example:
    ///     >>> from esmorph import Analyzer
    ///     >>> morph = Analyzer()
    ///     >>> morph.analyze("gata")[0].inflect(Number="Plur")
    ///     'gatas'
    ///     >>> morph.analyze("hablar")[0].inflect(Tense="Past", Person="3", Number="Plur")
    ///     'hablaron'
    #[pyo3(signature = (**feats))]
    fn inflect(&self, py: Python<'_>, feats: Option<&Bound<'_, PyDict>>) -> PyResult<String> {
        let analyzer = self.analyzer.get();
        let mut requested: Vec<(String, String)> = Vec::new();
        for (name, value) in feats.into_iter().flatten() {
            let name: String = name.extract()?;
            let Ok(value) = value.cast::<PyString>() else {
                return Err(type_error(
                    &format!("The value of the feature {name} must be a string"),
                    &value,
                ));
            };
            let value = value.to_str()?.to_owned();
            let Some(values) = analyzer.features.get(&name) else {
                return Err(PyValueError::new_err(format!(
                    "Unknown UD feature {name}; the dictionary has {}",
                    listed(analyzer.features.keys())
                )));
            };
            if !values.contains(&value) {
                return Err(PyValueError::new_err(format!(
                    "Unknown value of the feature {name}: {value}; the dictionary has {}",
                    listed(values)
                )));
            }
            requested.push((name, value));
        }
        requested.sort();
        let pairs: Vec<(&str, &str)> = requested
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        if let Some(form) = self
            .found()
            .and_then(|found| analyzer.dictionary.inflect(&found, &pairs))
        {
            return Ok(form.word);
        }
        let feats: Vec<String> = requested
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        Err(FormNotFoundError::new_err(format!(
            "The lexeme of {} has no form {}",
            py_repr(py, &self.word)?,
            feats.join("|")
        )))
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Analysis(word={}, lemma={}, pos={}, feats={}, prob={:.6}, words={}, source={})",
            py_repr(py, &self.word)?,
            py_repr(py, &self.lemma)?,
            py_repr(py, &self.tag.pos)?,
            self.feats(py)?.repr()?,
            self.prob,
            self.words(py)?.repr()?,
            py_repr(py, self.source)?
        ))
    }

    fn __eq__(&self, other: &Bound<'_, Self>) -> bool {
        let other = other.get();
        self.word == other.word
            && self.lemma == other.lemma
            && self.tag == other.tag
            && self.words == other.words
            && self.source == other.source
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        (&self.word, &self.lemma, &self.tag, &self.words, self.source).hash(&mut hasher);
        hasher.finish()
    }

    fn __copy__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf
    }

    fn __deepcopy__<'py>(slf: Bound<'py, Self>, _memo: &Bound<'py, PyAny>) -> Bound<'py, Self> {
        slf
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
                self.words.clone(),
                self.source.to_owned(),
                Some(self.analyzer.clone_ref(py)),
            ),
        ))
    }
}

/// Arguments of `_analysis`: the fields of an analysis and its analyzer for pickle
type AnalysisState = (
    String,
    String,
    String,
    String,
    f64,
    Vec<String>,
    String,
    Option<Py<Analyzer>>,
);

/// Restores an analysis from pickle; without an analyzer it takes the dictionary of the package
#[pyfunction]
#[pyo3(signature = (word, lemma, pos, feats, prob, words, source, analyzer=None))]
#[allow(clippy::too_many_arguments)]
fn _analysis(
    py: Python<'_>,
    word: String,
    lemma: String,
    pos: String,
    feats: String,
    prob: f64,
    words: Vec<String>,
    source: &str,
    analyzer: Option<Py<Analyzer>>,
) -> PyResult<Analysis> {
    let source = match source {
        "dictionary" => esmorph::Source::Dictionary.as_str(),
        "clitics" => esmorph::Source::Clitics.as_str(),
        _ => {
            return Err(PyValueError::new_err(format!(
                "Unknown source of an analysis: {source}"
            )));
        }
    };
    let analyzer = match analyzer {
        Some(analyzer) => analyzer,
        None => Py::new(py, Analyzer::new(py, None)?)?,
    };
    // The tag of the dictionary is shared by its analyses
    let tag = Tag { pos, feats };
    let tag = (analyzer.get().tags.iter())
        .find(|known| ***known == tag)
        .map_or_else(|| Arc::new(tag), Arc::clone);
    Ok(Analysis {
        word,
        lemma,
        prob,
        source,
        tag,
        words,
        analyzer,
    })
}

/// Morphological analyzer of Spanish
///
/// Description:
///     Finds the analyses of a word form in the dictionary, the most probable first. Case does not
///     matter. A word spelled as in the dictionary gives the analyses of that spelling; a word
///     absent as written is looked for without its accents and with a lost diaeresis. A word absent
///     from the dictionary may be a verb with clitics (`dámelo`). The dictionary is loaded once
///     when the analyzer is created; an analyzer can be used from several threads
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
    /// Values of every feature of the dictionary; a value that lists several (`Acc,Nom`) gives
    /// each of them too
    features: BTreeMap<String, BTreeSet<String>>,
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
        let mut features: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for tag in dictionary.ud_tags() {
            for (name, value) in tag.feats() {
                let values = features.entry(name.to_owned()).or_default();
                values.insert(value.to_owned());
                values.extend(value.split(',').map(str::to_owned));
            }
        }
        Ok(Analyzer {
            dictionary: Arc::new(dictionary),
            tags,
            features,
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
    fn analyze(slf: &Bound<'_, Self>, word: &Bound<'_, PyAny>) -> PyResult<Vec<Analysis>> {
        let word = text(word)?;
        let this = slf.get();
        Ok(this.convert(slf.py(), slf.as_unbound(), this.dictionary.analyze(&word)))
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
        slf: &Bound<'_, Self>,
        words: &Bound<'_, PyAny>,
    ) -> PyResult<Vec<Vec<Analysis>>> {
        let py = slf.py();
        let this = slf.get();
        let words = collect_words(words)?;
        let mut result = Vec::with_capacity(words.len());
        for chunk in words.chunks(CHUNK) {
            let analyses = py.detach(|| {
                chunk
                    .iter()
                    .map(|word| this.dictionary.analyze(word))
                    .collect::<Vec<_>>()
            });
            result.extend(
                analyses
                    .into_iter()
                    .map(|analyses| this.convert(py, slf.as_unbound(), analyses)),
            );
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

    /// Whether the word form has analyses
    ///
    /// Arguments:
    ///     word (str): The word form
    ///
    /// Returns:
    ///     bool: Whether analyze finds the word form in the dictionary or as a verb of it with
    ///     clitics
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
    /// Analyses for Python; `analyzer` is this analyzer
    fn convert(
        &self,
        py: Python<'_>,
        analyzer: &Py<Analyzer>,
        analyses: Vec<esmorph::Analysis>,
    ) -> Vec<Analysis> {
        analyses
            .into_iter()
            .filter_map(|analysis| {
                Some(Analysis {
                    tag: Arc::clone(self.tags.get(usize::from(analysis.ud))?),
                    source: analysis.source.as_str(),
                    prob: analysis.prob,
                    word: analysis.word,
                    lemma: analysis.lemma,
                    words: analysis.words,
                    analyzer: analyzer.clone_ref(py),
                })
            })
            .collect()
    }
}

/// Names or values for a message: in order, separated by commas
fn listed<'a>(items: impl IntoIterator<Item = &'a String>) -> String {
    items
        .into_iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
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
    m.add("FormNotFoundError", m.py().get_type::<FormNotFoundError>())?;
    m.add_function(wrap_pyfunction!(_analysis, m)?)?;
    m.add("__version__", package_version())?;
    Ok(())
}
