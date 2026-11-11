//! Python module `esmorph._esmorph`

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::ErrorKind;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use pyo3::create_exception;
use pyo3::exceptions::{PyLookupError, PyOSError, PyOverflowError, PyTypeError, PyValueError};
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyByteArray, PyBytes, PyDict, PyFloat, PyIterator, PyList, PyString, PyTuple, PyWeakrefMethods,
    PyWeakrefReference,
};

/// How many words `analyze_many` and `lemmatize` read and analyze at a time without the GIL: the
/// input is not copied whole, and Ctrl+C is checked between the chunks
const CHUNK: usize = 8192;
/// `errno` of an invalid argument, the same on POSIX and in the C runtime of Windows
const EINVAL: i32 = 22;

/// The analyzer of the dictionary of the package that pickle restores: one for the process
static PACKAGE: PyOnceLock<Py<Analyzer>> = PyOnceLock::new();
/// Analyzers of dictionary files that pickle saved or restored, by path; a weak reference lets an
/// analyzer go with its file
static FILES: Mutex<BTreeMap<PathBuf, Py<PyWeakrefReference>>> = Mutex::new(BTreeMap::new());
/// The functions that restore an analysis and an analyzer from pickle
static RESTORE_ANALYSIS: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
static RESTORE_ANALYZER: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

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

/// The analyzer of the analyses of one thread. Analyses hold their analyzer through the handle of
/// their thread: in Python without the GIL a reference count of a shared object that several
/// threads change slows the analysis several times
struct Handle {
    analyzer: Py<Analyzer>,
}

/// How many handles a thread keeps: a thread uses few analyzers by turns, and a long list would
/// make unpickling thousands of analyses quadratic
const HANDLE_SLOTS: usize = 4;

/// Handles of the analyzers of a thread: the address of the analyzer object and a weak reference,
/// so a handle lives as long as its analyses and does not keep the analyzer longer; a new handle
/// takes the place of a dead one or the next place in turn
struct Handles {
    slots: [(usize, Weak<Handle>); HANDLE_SLOTS],
    next: usize,
}

thread_local! {
    static HANDLES: RefCell<Handles> = const {
        RefCell::new(Handles {
            slots: [const { (0, Weak::new()) }; HANDLE_SLOTS],
            next: 0,
        })
    };
}

/// The tag of an analysis: the number of a tag of the dictionary of its analyzer, whose objects it
/// shares, or its own for an analysis pickled with another dictionary
enum AnalysisTag {
    Dictionary(u16),
    Own(Box<Tag>),
}

/// Analysis of a word form
///
/// Description:
///     Analyses are immutable and hashable; they are compared without the probability, so an
///     analysis equals its form from forms(). An analysis keeps its analyzer; pickle saves the
///     path of its dictionary, and unpickling takes the analyzer of that path the process has
///     loaded, or the dictionary of the package if the file is gone
///
/// Attributes:
///     word (str): The word form as the dictionary spells it, with its accents; a verb with
///         clitics as written, in lower case; a token of the rules as written
///     lemma (str): The lemma
///     pos (str): The part of speech of Universal Dependencies
///     feats (dict[str, str]): The Universal Dependencies features
///     prob (float): Share of the analysis among the analyses of the word, by the counts of
///         treebanks; equal for a word they did not see
///     words (tuple[str, ...]): Words of the token: a verb and its clitics, the words of a
///         contraction, or the word itself; the analysis describes the first of them
///     source (str): Where the analysis comes from: "dictionary", "clitics" for a verb of the
///         dictionary with clitics, "rule" for a token out of the dictionary (punctuation, a
///         symbol, a number, a Roman numeral, a name) and for the name or the Roman numeral of a
///         word with a capital letter that the dictionary has, "guess" for a guessed lowercase word
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
    tag: AnalysisTag,
    /// Words of a token of several; empty for one word
    words: Vec<String>,
    /// The analyzer that gave the analysis, through the handle of a thread: its dictionary has
    /// the forms of the lexeme
    handle: Arc<Handle>,
    /// The form of the inflection table of the analysis; an unpickled analysis finds it on first use
    entry: OnceLock<Option<esmorph::Entry>>,
}

impl Analysis {
    /// Part of speech and features of the analysis
    fn tag(&self) -> &Tag {
        match &self.tag {
            AnalysisTag::Dictionary(ud) => &self.handle.analyzer.get().tags[usize::from(*ud)],
            AnalysisTag::Own(tag) => tag,
        }
    }

    /// Number of the tag in the dictionary of the analyzer; `None` if it has no such tag
    fn ud(&self) -> Option<u16> {
        match self.tag {
            AnalysisTag::Dictionary(ud) => Some(ud),
            AnalysisTag::Own(_) => None,
        }
    }

    /// The analysis as the core has it: the forms of its lexeme come from it. An analysis of a
    /// lexeme that the dictionary does not have (pickled with another one) has none
    fn core(&self) -> Option<esmorph::Analysis> {
        let source = source(self.source)?;
        let ud = self.ud()?;
        let entry = *self.entry.get_or_init(|| self.find_entry(ud, source));
        if entry.is_none() && source != esmorph::Source::Rule {
            return None;
        }
        Some(esmorph::Analysis::new(
            self.word.clone(),
            self.lemma.clone(),
            ud,
            self.prob,
            source,
            entry,
            self.words.clone(),
        ))
    }

    /// The form of the table of an unpickled analysis: the one of the same analysis in the
    /// dictionary of its analyzer
    fn find_entry(&self, ud: u16, source: esmorph::Source) -> Option<esmorph::Entry> {
        let dictionary = &self.handle.analyzer.get().dictionary;
        // The lemma of a name may differ in case: `Madrid` of the token `madrid`
        let lemma = self.lemma.to_lowercase();
        (dictionary.analyze(&self.word).into_iter())
            .find(|analysis| {
                analysis.lemma.to_lowercase() == lemma
                    && analysis.ud == ud
                    && analysis.words == self.words
                    && analysis.source == source
            })
            .and_then(|analysis| analysis.entry)
            // A guessed form is a guess of its lexeme, not of its word: `blorpeás` of `blorpear`
            .or_else(|| {
                if source != esmorph::Source::Guess {
                    return None;
                }
                (dictionary.analyze(&self.lemma).into_iter())
                    .flat_map(|analysis| dictionary.forms(&analysis))
                    .find(|form| {
                        form.word == self.word
                            && form.ud == ud
                            && form.source == source
                            && form.lemma.to_lowercase() == lemma
                    })
                    .and_then(|form| form.entry)
            })
    }
}

/// The source of an analysis by its name
fn source(name: &str) -> Option<esmorph::Source> {
    [
        esmorph::Source::Dictionary,
        esmorph::Source::Clitics,
        esmorph::Source::Rule,
        esmorph::Source::Guess,
    ]
    .into_iter()
    .find(|source| source.as_str() == name)
}

#[pymethods]
impl Analysis {
    #[getter]
    fn pos(&self) -> &str {
        &self.tag().pos
    }

    #[getter]
    fn feats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let feats = PyDict::new(py);
        for (name, value) in self.tag().pairs() {
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
    ///     clitics gives the forms of its verb, without clitics; an analysis of a rule is its
    ///     only form
    ///
    /// Returns:
    ///     list[Analysis]: The forms
    ///
    /// Example:
    ///     >>> from esmorph import Analyzer
    ///     >>> [form.word for form in Analyzer().analyze("gatos")[0].forms()]
    ///     ['gato', 'gata', 'gatos', 'gatas']
    fn forms(&self, py: Python<'_>) -> PyResult<Vec<Py<Analysis>>> {
        let Some(found) = self.core() else {
            return Ok(Vec::new());
        };
        let analyzer = self.handle.analyzer.get();
        analyzer.convert(py, &self.handle, analyzer.dictionary.forms(&found))
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
    ///     FormNotFoundError: If the lexeme has no form with these features, or the dictionary has
    ///         no lexeme of the analysis
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
        let analyzer = self.handle.analyzer.get();
        let mut requested: Vec<(String, String)> = Vec::new();
        for (name, value) in feats.into_iter().flatten() {
            let name: String = name.extract()?;
            let Ok(value) = value.cast::<PyString>() else {
                return Err(type_error(
                    &format!("The value of the feature {name} must be a string"),
                    &value,
                ));
            };
            // The parts of a value with several are in the order of letters: `Acc,Nom`
            let given = value.to_str()?;
            let mut parts: Vec<&str> = given.split(',').collect();
            parts.sort_unstable();
            let value = parts.join(",");
            let Some(values) = analyzer.features.get(&name) else {
                return Err(PyValueError::new_err(format!(
                    "Unknown UD feature {name}; the dictionary has {}",
                    listed(analyzer.features.keys())
                )));
            };
            if !values.contains(&value) {
                return Err(PyValueError::new_err(format!(
                    "Unknown value of the feature {name}: {given}; the dictionary has {}",
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
        let Some(core) = self.core() else {
            return Err(FormNotFoundError::new_err(format!(
                "The dictionary of the analyzer has no lexeme of {}",
                py_repr(py, &self.word)?
            )));
        };
        if let Some(form) = analyzer.dictionary.inflect(&core, &pairs) {
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
            "Analysis(word={}, lemma={}, pos={}, feats={}, prob={}, words={}, source={})",
            py_repr(py, &self.word)?,
            py_repr(py, &self.lemma)?,
            py_repr(py, &self.tag().pos)?,
            self.feats(py)?.repr()?,
            // Six significant digits: a tiny probability is not 0.000000
            PyFloat::new(
                py,
                format!("{:.5e}", self.prob).parse().unwrap_or(self.prob)
            )
            .repr()?,
            self.words(py)?.repr()?,
            py_repr(py, self.source)?
        ))
    }

    fn __eq__(&self, other: &Bound<'_, Self>) -> bool {
        let other = other.get();
        self.word == other.word
            && self.lemma == other.lemma
            && self.tag() == other.tag()
            && self.words == other.words
            && self.source == other.source
    }

    fn __hash__(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        (
            &self.word,
            &self.lemma,
            self.tag(),
            &self.words,
            self.source,
        )
            .hash(&mut hasher);
        hasher.finish()
    }

    fn __copy__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf
    }

    fn __deepcopy__<'py>(slf: Bound<'py, Self>, _memo: &Bound<'py, PyAny>) -> Bound<'py, Self> {
        slf
    }

    fn __reduce__<'py>(&self, py: Python<'py>) -> PyResult<(Bound<'py, PyAny>, AnalysisState)> {
        let analyzer = &self.handle.analyzer;
        let tag = self.tag();
        Ok((
            restorer(py, &RESTORE_ANALYSIS, "_analysis")?,
            (
                self.word.clone(),
                self.lemma.clone(),
                tag.pos.clone(),
                tag.feats.clone(),
                self.prob,
                self.words.clone(),
                self.source.to_owned(),
                // The analyzer itself: pickle keeps it once for all the analyses of a list
                Some(analyzer.clone_ref(py).into_any()),
                (self.entry.get())
                    .map(|&entry| saved_entry(analyzer.get().dictionary.checksum(), entry)),
                analyzer.get().verified.load(Ordering::Relaxed),
            ),
        ))
    }
}

/// The form of the table of an analysis in pickle, if known: the checksum of the dictionary in
/// the high 32 bits, the table and the form in the low ones, all ones for no table
type SavedEntry = Option<u64>;

const NO_ENTRY: u64 = 0xFFFF_FFFF;

fn saved_entry(checksum: u32, entry: Option<esmorph::Entry>) -> u64 {
    let low = entry.map_or(NO_ENTRY, |entry| {
        u64::from(entry.table) << 16 | u64::from(entry.form)
    });
    u64::from(checksum) << 32 | low
}

/// The path of a dictionary that pickle saves (`None` for the dictionary of the package) or, for an
/// analysis, the analyzer itself
type SavedPath = Option<Py<PyAny>>;

/// Arguments of `_analysis`: the fields of an analysis and the path of its dictionary for pickle
type AnalysisState = (
    String,
    String,
    String,
    String,
    f64,
    Vec<String>,
    String,
    SavedPath,
    SavedEntry,
    bool,
);

/// Restores an analysis from pickle with the analyzer of `path` (`None` for the dictionary of the
/// package), verified with `verify`; if that file cannot be read, with the dictionary of the
/// package. The form of its table is taken from `entry` if that is for the same dictionary,
/// otherwise found on first use
#[pyfunction]
#[pyo3(signature = (word, lemma, pos, feats, prob, words, source, path=None, entry=None, verify=false))]
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
    path: Option<&Bound<'_, PyAny>>,
    entry: SavedEntry,
    verify: bool,
) -> PyResult<Py<Analysis>> {
    let Some(source) = self::source(source).map(esmorph::Source::as_str) else {
        return Err(PyValueError::new_err(format!(
            "Unknown source of an analysis: {source}"
        )));
    };
    // The analyzer itself, or the path of its dictionary in a pickle of an earlier version
    let analyzer = match path {
        Some(path) => match path.cast::<Analyzer>() {
            Ok(analyzer) => Ok(analyzer.clone()),
            Err(_) => {
                shared(py, Some(path.extract()?), verify).map(|analyzer| analyzer.into_bound(py))
            }
        },
        None => shared(py, None, verify).map(|analyzer| analyzer.into_bound(py)),
    };
    let analyzer = match analyzer {
        Ok(analyzer) => analyzer,
        Err(err) if !err.is_instance_of::<PyOSError>(py) => return Err(err),
        Err(_) => shared(py, None, verify)?.into_bound(py),
    };
    // The tag of the dictionary is shared by its analyses
    let this = analyzer.get();
    let tag = Tag { pos, feats };
    let ud = this.tag_numbers.get(&tag).copied();
    let handle = Analyzer::handle(&analyzer);
    let tag = match ud {
        Some(ud) => AnalysisTag::Dictionary(ud),
        None => AnalysisTag::Own(Box::new(tag)),
    };
    // The form of its table, unless pickle has it for this dictionary, is found on first use:
    // unpickling a list of analyses stays cheap
    let entry = match entry {
        _ if ud.is_none() => OnceLock::from(None),
        Some(saved) if saved >> 32 == u64::from(this.dictionary.checksum()) => {
            let low = saved & NO_ENTRY;
            OnceLock::from(
                (low != NO_ENTRY).then_some(esmorph::Entry::new((low >> 16) as u16, low as u16)),
            )
        }
        _ => OnceLock::new(),
    };
    untracked(
        py,
        Analysis {
            word,
            lemma,
            prob,
            source,
            tag,
            words,
            handle,
            entry,
        },
    )
}

/// Restores an analyzer from pickle: the analyzer of the dictionary of the package or of the file
/// at `path` that the process has, otherwise a new one; `verify` - the dictionary is verified
#[pyfunction]
#[pyo3(signature = (path=None, verify=false))]
fn _analyzer(py: Python<'_>, path: Option<PathBuf>, verify: bool) -> PyResult<Py<Analyzer>> {
    shared(py, path, verify)
}

/// The analyzer of the dictionary of the package, or of the file at `path`, shared in the process;
/// with `verify` its dictionary is verified, once
fn shared(py: Python<'_>, path: Option<PathBuf>, verify: bool) -> PyResult<Py<Analyzer>> {
    let analyzer = match path {
        None => (PACKAGE.get_or_try_init(py, || Py::new(py, Analyzer::new(py, None, false)?))?)
            .clone_ref(py),
        Some(path) => {
            let known = (FILES.lock().unwrap_or_else(PoisonError::into_inner))
                .get(&path)
                .and_then(|reference| reference.bind(py).upgrade());
            match known {
                Some(analyzer) => analyzer.cast_into::<Analyzer>()?.unbind(),
                None => {
                    // The dictionary is loaded without the lock: the loading lets other threads run
                    let analyzer = Bound::new(py, Analyzer::new(py, Some(path), verify)?)?;
                    Analyzer::saved_path(&analyzer)?;
                    analyzer.unbind()
                }
            }
        }
    };
    let this = analyzer.get();
    if verify && !this.verified.load(Ordering::Relaxed) {
        py.detach(|| this.dictionary.verify())
            .map_err(|err| load_error(&this.path, err))?;
        this.verified.store(true, Ordering::Relaxed);
    }
    Ok(analyzer)
}

/// Morphological analyzer of Spanish
///
/// Description:
///     Finds the analyses of a word form in the dictionary, the most probable first. Case does not
///     matter for finding a word, though a word with a capital letter may also be a name. A word
///     spelled as in the dictionary gives the analyses of that spelling; a word absent as written
///     is looked for without its accents and with a lost diaeresis. A word absent from the
///     dictionary may be a verb with clitics (`dámelo`); other tokens get the analyses of rules
///     (punctuation, symbols, numbers, Roman numerals and names), and other lowercase words are
///     guessed. The dictionary is loaded once when the analyzer is created; an analyzer can be
///     used from several threads
///
/// Arguments:
///     path (str | os.PathLike | None): Path to a dictionary file; by default the dictionary of the
///         package
///     verify (bool): Check the whole dictionary, for a file of a source that is not trusted: a
///         deliberately crafted file with a right checksum may otherwise make the analysis fail.
///         Takes tens of milliseconds; pickle keeps it
///
/// Raises:
///     FileNotFoundError: If there is no dictionary file
///     OSError: If the dictionary file cannot be read or is not a file
///     ValueError: If the file is no dictionary of esMorph or is damaged, or the path has a NUL
///         character
///
/// Example:
///     >>> from esmorph import Analyzer
///     >>> morph = Analyzer()
///     >>> sorted({analysis.lemma for analysis in morph.analyze("hablamos")})
///     ['hablar']
#[pyclass(frozen, weakref, module = "esmorph")]
struct Analyzer {
    dictionary: Arc<esmorph::Dictionary>,
    tags: Vec<Arc<Tag>>,
    /// Numbers of the tags, for unpickling
    tag_numbers: HashMap<Arc<Tag>, u16>,
    /// Values of every feature of the dictionary; a value that lists several (`Acc,Nom`) gives
    /// each of them too
    features: BTreeMap<String, BTreeSet<String>>,
    path: PathBuf,
    /// The path was given: otherwise pickle does not keep it, and another environment takes the
    /// dictionary of its own package
    explicit: bool,
    /// The path that pickle saves, made once: one object for all analyses of a list
    saved: PyOnceLock<Option<Py<PyAny>>>,
    /// The dictionary is verified: pickle keeps it
    verified: AtomicBool,
}

#[pymethods]
impl Analyzer {
    #[new]
    #[pyo3(signature = (path=None, *, verify=false))]
    fn new(py: Python<'_>, path: Option<PathBuf>, verify: bool) -> PyResult<Self> {
        let explicit = path.is_some();
        let path = match path {
            // An absolute path: pickle may restore the analyzer in another working directory
            Some(path) => std::path::absolute(&path).unwrap_or(path),
            None => default_path(py)?,
        };
        if path.as_os_str().as_encoded_bytes().contains(&0) {
            return Err(PyValueError::new_err(
                "The path to the dictionary has a NUL character",
            ));
        }
        // Opening a pipe or a device would wait for its writer
        if std::fs::metadata(&path).is_ok_and(|meta| !meta.is_file() && !meta.is_dir()) {
            return Err(PyOSError::new_err((
                EINVAL,
                "The path to the dictionary is not a file",
                path.display().to_string(),
            )));
        }
        let dictionary = py
            .detach(|| {
                let dictionary = esmorph::Dictionary::open(&path)?;
                if verify {
                    dictionary.verify()?;
                }
                Ok(dictionary)
            })
            .map_err(|err| load_error(&path, err))?;
        let tags: Vec<Arc<Tag>> = dictionary
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
        let tag_numbers = (tags.iter().enumerate())
            .filter_map(|(ud, tag)| Some((Arc::clone(tag), u16::try_from(ud).ok()?)))
            .collect();
        Ok(Analyzer {
            dictionary: Arc::new(dictionary),
            tags,
            tag_numbers,
            features,
            path,
            explicit,
            saved: PyOnceLock::new(),
            verified: AtomicBool::new(verify),
        })
    }

    fn __reduce__<'py>(slf: &Bound<'py, Self>) -> PyResult<(Bound<'py, PyAny>, (SavedPath, bool))> {
        let restore = restorer(slf.py(), &RESTORE_ANALYZER, "_analyzer")?;
        let verified = slf.get().verified.load(Ordering::Relaxed);
        Ok((restore, (Analyzer::saved_path(slf)?, verified)))
    }

    /// Path to the dictionary file
    #[getter]
    fn path(&self) -> PathBuf {
        self.path.clone()
    }

    /// Analyses of a word form
    ///
    /// Description:
    ///     A token that the dictionary does not have gets the analyses of the rules: punctuation,
    ///     a symbol, a number in digits, an ordinal number in digits, a Roman numeral, a name with
    ///     a capital letter, an address, an abbreviation. A word with a capital letter that the
    ///     dictionary knows may be a name too (`ETA`, `Angel`), and a name of the dictionary has a
    ///     lemma with a capital letter (`Madrid`). A lowercase word out of the dictionary is
    ///     guessed: by its last part after a hyphen, by the word without a known prefix, as a verb
    ///     with clitics of a guessed host or by the dictionary words with its ending
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
    ///     >>> [(analysis.lemma, analysis.pos) for analysis in morph.analyze("camiones")]
    ///     [('camión', 'NOUN')]
    ///     >>> [(analysis.lemma, analysis.pos, analysis.source) for analysis in morph.analyze("10.000")]
    ///     [('10000', 'NUM', 'rule')]
    ///     >>> [(analysis.lemma, analysis.source) for analysis in morph.analyze("supergallos")]
    ///     [('supergallo', 'guess')]
    ///     >>> morph.analyze("qzxw")
    ///     []
    fn analyze(slf: &Bound<'_, Self>, word: &Bound<'_, PyAny>) -> PyResult<Vec<Py<Analysis>>> {
        let word = text(word)?;
        let this = slf.get();
        this.convert(
            slf.py(),
            &Analyzer::handle(slf),
            this.dictionary.analyze(&word),
        )
    }

    /// Analyses of a list of word forms
    ///
    /// Description:
    ///     The same as analyze for every word form. The analyses are Python objects, so threads
    ///     speed it up little; lemmatize releases the GIL
    ///
    /// Arguments:
    ///     words (Iterable[str]): The word forms
    ///
    /// Returns:
    ///     list[list[Analysis]]: The analyses of every word form
    ///
    /// Raises:
    ///     TypeError: If a string or bytes are given instead of a collection of words, or a word is
    ///         not a string
    ///
    /// Example:
    ///     >>> from esmorph import Analyzer
    ///     >>> analyses = Analyzer().analyze_many(["camiones", "qzxw"])
    ///     >>> [analysis.lemma for analysis in analyses[0]], analyses[1]
    ///     (['camión'], [])
    fn analyze_many<'py>(
        slf: &Bound<'py, Self>,
        words: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let py = slf.py();
        let this = slf.get();
        let handle = Analyzer::handle(slf);
        let mut words = Words::new(words)?;
        let result = PyList::empty(py);
        while words.next_chunk()? {
            let analyses = py.detach(|| {
                (words.texts.iter())
                    .map(|word| this.dictionary.analyze(word))
                    .collect::<Vec<_>>()
            });
            for analyses in analyses {
                result.append(this.convert(py, &handle, analyses)?)?;
            }
            py.check_signals()?;
        }
        Ok(result)
    }

    /// Lemmas of a list of word forms
    ///
    /// Description:
    ///     The lemma of the first analysis; a word without analyses comes back in lower case. The
    ///     analysis goes without the GIL
    ///
    /// Arguments:
    ///     words (Iterable[str]): The word forms
    ///
    /// Returns:
    ///     list[str]: The lemmas
    ///
    /// Raises:
    ///     TypeError: If a string or bytes are given instead of a collection of words, or a word is
    ///         not a string
    ///
    /// Example:
    ///     >>> from esmorph import Analyzer
    ///     >>> Analyzer().lemmatize(["CAMIONES", "Bush", "qzxw"])
    ///     ['camión', 'Bush', 'qzxw']
    fn lemmatize<'py>(
        &self,
        py: Python<'py>,
        words: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let mut words = Words::new(words)?;
        // Equal lemmas are one string object: a list of the lemmas of a corpus takes much less memory
        let mut strings: HashMap<String, Py<PyString>> = HashMap::new();
        let result = PyList::empty(py);
        while words.next_chunk()? {
            let lemmas = py.detach(|| {
                (words.texts.iter())
                    .map(|word| {
                        let analysis = self.dictionary.analyze(word).into_iter().next();
                        analysis.map(|analysis| analysis.lemma)
                    })
                    .collect::<Vec<_>>()
            });
            for (number, lemma) in lemmas.into_iter().enumerate() {
                let lemma = match lemma {
                    Some(lemma) => lemma,
                    // A word with lone surrogates comes back with them
                    None => match words.original(number) {
                        Some(word) => {
                            result.append(word.call_method0("lower")?)?;
                            continue;
                        }
                        None => words.texts[number].to_lowercase(),
                    },
                };
                let object = match strings.get(&lemma) {
                    Some(object) => object.clone_ref(py),
                    None => {
                        let object = PyString::new(py, &lemma).unbind();
                        strings.insert(lemma, object.clone_ref(py));
                        object
                    }
                };
                result.append(object)?;
            }
            py.check_signals()?;
        }
        Ok(result)
    }

    /// Analysis of a document of the anySeg segmenter
    ///
    /// Description:
    ///     Fills the columns lemma, upos and feats of the words of the document with the most
    ///     probable analysis of every word, as analyze orders them. The words of a multiword token
    ///     are analyzed by the token: "dime" split into "di" and "me" is the imperative of "decir"
    ///     with the pronoun "me", and clitics after a verb are pronouns. A token that the segmenter
    ///     leaves as one word gets the analysis of the whole token: "devuélvenos" is the imperative
    ///     of "devolver". A word without analyses gets its lower case as the lemma and the part of
    ///     speech X; the earlier values of the columns are replaced. The document is an
    ///     anyseg.Document or an object with the same column("form"), multiword_tokens (a token has
    ///     text and words, a word has index) and set_columns. The analysis goes without the GIL
    ///
    /// Arguments:
    ///     document (anyseg.Document): The document
    ///
    /// Raises:
    ///     ValueError: If the words of the multiword tokens are not consecutive words of the
    ///         document in order
    ///     TypeError: If a form or the text of a token is not a string or the index of a word is
    ///         not an integer
    ///
    /// Example:
    ///     >>> from anyseg import Segmenter
    ///     >>> from esmorph import Analyzer
    ///     >>> document = Segmenter("es").segment("Dime la verdad.")
    ///     >>> Analyzer().annotate(document)
    ///     >>> [(word.form, word.lemma, word.upos) for word in document.words]
    ///     [('Di', 'decir', 'VERB'), ('me', 'yo', 'PRON'), ('la', 'el', 'DET'), ('verdad', 'verdad', 'NOUN'), ('.', '.', 'PUNCT')]
    fn annotate(&self, py: Python<'_>, document: &Bound<'_, PyAny>) -> PyResult<()> {
        let mut words = Words::new(&document.call_method1(intern!(py, "column"), ("form",))?)?;
        let mut forms = Vec::new();
        while words.next_chunk()? {
            forms.append(&mut words.texts);
        }
        let tokens = multiword_tokens(document, forms.len())?;
        let mut fields = Vec::with_capacity(forms.len());
        let (mut word, mut multiword) = (0, tokens.iter().peekable());
        while word < forms.len() {
            // Chunks of about CHUNK words: Ctrl+C is checked between them
            let end = (word + CHUNK).min(forms.len());
            py.detach(|| {
                while word < end {
                    match multiword.next_if(|(_, range)| range.start == word) {
                        Some((text, range)) => {
                            self.dictionary
                                .token_fields(text, &forms[range.clone()], &mut fields);
                            word = range.end;
                        }
                        None => {
                            let form = forms[word].as_str();
                            self.dictionary.token_fields(form, &[form], &mut fields);
                            word += 1;
                        }
                    }
                }
            });
            py.check_signals()?;
        }
        // A part of speech and features of the dictionary are one string object for all words
        let mut strings: HashMap<&str, Bound<'_, PyString>> = HashMap::new();
        let (mut lemmas, mut parts, mut feats) = (
            Vec::with_capacity(fields.len()),
            Vec::with_capacity(fields.len()),
            Vec::with_capacity(fields.len()),
        );
        for word in fields {
            let mut string = |text| {
                (strings.entry(text))
                    .or_insert_with(|| PyString::new(py, text))
                    .clone()
            };
            lemmas.push(word.lemma);
            parts.push(string(word.pos));
            feats.push(word.feats.map(string));
        }
        let columns = PyDict::new(py);
        columns.set_item(intern!(py, "lemma"), lemmas)?;
        columns.set_item(intern!(py, "upos"), parts)?;
        columns.set_item(intern!(py, "feats"), feats)?;
        document.call_method(intern!(py, "set_columns"), (), Some(&columns))?;
        Ok(())
    }

    /// Whether the dictionary has the word form
    ///
    /// Description:
    ///     The analyses of the rules and the guesses do not count: a name, a number or a guessed
    ///     word has analyses but is not known
    ///
    /// Arguments:
    ///     word (str): The word form
    ///
    /// Returns:
    ///     bool: Whether the dictionary has the word form, as written or as a verb of it with
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

    /// Description of the dictionary
    ///
    /// Description:
    ///     The source of the dictionary and its snapshot with its SHA-256, the license, the
    ///     release of Universal Dependencies and the treebanks of the counts of analyses with their
    ///     licenses, as far as the dictionary describes them, and always the number of word forms
    ///     and the checksum of the file
    ///
    /// Returns:
    ///     dict: The keys source, snapshot, sha256, license, ud_release, treebanks, word_forms and
    ///     checksum; a new dictionary every time
    ///
    /// Example:
    ///     >>> from esmorph import Analyzer
    ///     >>> info = Analyzer().dictionary_info
    ///     >>> info["source"], info["license"], sorted(info["treebanks"])
    ///     ('Wiktionary', 'CC BY-SA 4.0', ['UD_Spanish-AnCora', 'UD_Spanish-GSD'])
    #[getter]
    fn dictionary_info<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let info = PyDict::new(py);
        // The description is JSON of the builder; another one has none of these keys
        let meta = py
            .import(intern!(py, "json"))?
            .call_method1(intern!(py, "loads"), (self.dictionary.meta(),));
        if let Ok(meta) = meta
            && let Ok(meta) = meta.cast::<PyDict>()
        {
            for key in ["source", "snapshot", "sha256", "license"] {
                if let Some(value) = meta.get_item(key)? {
                    info.set_item(key, value)?;
                }
            }
            if let Some(counts) = meta.get_item("counts")?
                && let Ok(counts) = counts.cast::<PyDict>()
            {
                for (key, name) in [("release", "ud_release"), ("treebanks", "treebanks")] {
                    if let Some(value) = counts.get_item(key)? {
                        info.set_item(name, value)?;
                    }
                }
            }
        }
        info.set_item("word_forms", self.dictionary.len())?;
        info.set_item("checksum", self.dictionary.checksum())?;
        Ok(info)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Analyzer(path={})",
            py_repr(py, &self.path.display().to_string())?
        ))
    }
}

impl Analyzer {
    /// The handle of the analyzer for the analyses of this thread: one per analyzer and thread
    /// while its analyses live
    fn handle(slf: &Bound<'_, Self>) -> Arc<Handle> {
        let address = slf.as_ptr() as usize;
        let new = || {
            Arc::new(Handle {
                analyzer: slf.clone().unbind(),
            })
        };
        // A thread that is ending has no handles: the handle is made without keeping it
        HANDLES
            .try_with(|handles| {
                let mut handles = handles.borrow_mut();
                let found = (handles.slots.iter())
                    .filter(|(known, _)| *known == address)
                    .find_map(|(_, handle)| handle.upgrade());
                if let Some(handle) = found {
                    return handle;
                }
                let slot = match (handles.slots.iter())
                    .position(|(_, handle)| handle.strong_count() == 0)
                {
                    Some(slot) => slot,
                    None => {
                        let slot = handles.next;
                        handles.next = (slot + 1) % HANDLE_SLOTS;
                        slot
                    }
                };
                let handle = new();
                handles.slots[slot] = (address, Arc::downgrade(&handle));
                handle
            })
            .unwrap_or_else(|_| new())
    }

    /// The path that pickle saves for an analyzer: `None` for the dictionary of the package. An
    /// analyzer of a file becomes the one that unpickling in this process takes for its path
    fn saved_path(analyzer: &Bound<'_, Analyzer>) -> PyResult<SavedPath> {
        let py = analyzer.py();
        let this = analyzer.get();
        let saved = this.saved.get_or_try_init(py, || -> PyResult<_> {
            if !this.explicit {
                return Ok(None);
            }
            let reference = PyWeakrefReference::new(analyzer)?.unbind();
            let mut files = FILES.lock().unwrap_or_else(PoisonError::into_inner);
            files.retain(|_, known| known.bind(py).upgrade().is_some());
            files.insert(this.path.clone(), reference);
            Ok(Some(
                this.path.clone().into_pyobject(py)?.into_any().unbind(),
            ))
        })?;
        Ok(saved.as_ref().map(|path| path.clone_ref(py)))
    }

    /// Analyses for Python; `handle` is the handle of this analyzer
    fn convert(
        &self,
        py: Python<'_>,
        handle: &Arc<Handle>,
        analyses: Vec<esmorph::Analysis>,
    ) -> PyResult<Vec<Py<Analysis>>> {
        analyses
            .into_iter()
            .filter(|analysis| usize::from(analysis.ud) < self.tags.len())
            .map(|analysis| Analysis {
                tag: AnalysisTag::Dictionary(analysis.ud),
                source: analysis.source.as_str(),
                prob: analysis.prob,
                word: analysis.word,
                lemma: analysis.lemma,
                words: analysis.words,
                handle: Arc::clone(handle),
                entry: OnceLock::from(analysis.entry),
            })
            .map(|analysis| untracked(py, analysis))
            .collect()
    }
}

/// An analysis as a Python object that the garbage collector does not track. An analysis refers
/// only to its analyzer, which refers to no Python object, so it can close no cycle; tracking
/// hundreds of thousands of them took half of the time of `analyze_many`
fn untracked(py: Python<'_>, analysis: Analysis) -> PyResult<Py<Analysis>> {
    let object = Py::new(py, analysis)?;
    let pointer = object.as_ptr();
    // SAFETY: the pointer is a live object; untracking an object of a type with
    // `Py_TPFLAGS_HAVE_GC` is allowed at any time, and its deallocation untracks it again safely
    unsafe {
        let flags = pyo3::ffi::PyType_GetFlags(pyo3::ffi::Py_TYPE(pointer));
        if flags & pyo3::ffi::Py_TPFLAGS_HAVE_GC != 0 {
            pyo3::ffi::PyObject_GC_UnTrack(pointer.cast());
        }
    }
    Ok(object)
}

/// A function of the module that pickle calls to restore an object, looked up once
fn restorer<'py>(
    py: Python<'py>,
    cell: &'static PyOnceLock<Py<PyAny>>,
    name: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let function = cell.get_or_try_init(py, || {
        PyResult::Ok(py.import("esmorph._esmorph")?.getattr(name)?.unbind())
    })?;
    Ok(function.bind(py).clone())
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

/// The command `esmorph` with `args`: its exit code. The core parses the arguments, reads the files
/// and writes the result; the default dictionary is the one of the package
#[pyfunction]
fn _main(py: Python<'_>, args: Vec<std::ffi::OsString>) -> PyResult<u8> {
    let path = default_path(py)?;
    let version = package_version();
    Ok(py.detach(|| esmorph::cli::run(args, &version, Some(&path))))
}

fn default_path(py: Python<'_>) -> PyResult<PathBuf> {
    let init: PathBuf = py.import("esmorph")?.getattr("__file__")?.extract()?;
    Ok(init.with_file_name("data").join("es.dict"))
}

/// Words of an iterable, read in chunks: the input is not copied whole
struct Words<'py> {
    /// Fused: an iterator that has ended is not asked again, as `sys.stdin` would wait for more
    iterator: std::iter::Fuse<Bound<'py, PyIterator>>,
    /// Number of the words read
    read: usize,
    /// The words of the chunk
    texts: Vec<String>,
    /// Words of the chunk with lone surrogates, by their numbers in it
    originals: Vec<(usize, Bound<'py, PyString>)>,
}

impl<'py> Words<'py> {
    fn new(words: &Bound<'py, PyAny>) -> PyResult<Self> {
        if words.is_instance_of::<PyString>() {
            return Err(PyTypeError::new_err(
                "Expected a collection of words, not a string: pass a list of words",
            ));
        }
        if words.is_instance_of::<PyBytes>() || words.is_instance_of::<PyByteArray>() {
            return Err(type_error(
                "Expected a collection of words: pass a list of strings",
                words,
            ));
        }
        let iterator = words.try_iter().map_err(|err| {
            if err.is_instance_of::<PyTypeError>(words.py()) {
                type_error("Expected a collection of words", words)
            } else {
                err
            }
        })?;
        Ok(Words {
            iterator: iterator.fuse(),
            read: 0,
            texts: Vec::with_capacity(CHUNK),
            originals: Vec::new(),
        })
    }

    /// Reads the next chunk of words; false at the end
    fn next_chunk(&mut self) -> PyResult<bool> {
        self.texts.clear();
        self.originals.clear();
        for word in self.iterator.by_ref().take(CHUNK) {
            let word = word?;
            if !word.is_instance_of::<PyString>() {
                return Err(PyTypeError::new_err(format!(
                    "Every word must be a string: item {} is {}",
                    self.read,
                    word.get_type().name()?
                )));
            }
            let word = word.cast_into::<PyString>()?;
            let (text, replaced) = lossy(&word)?;
            if replaced {
                self.originals.push((self.texts.len(), word));
            }
            self.texts.push(text);
            self.read += 1;
        }
        Ok(!self.texts.is_empty())
    }

    /// The word of the chunk with lone surrogates as Python has it
    fn original(&self, number: usize) -> Option<&Bound<'py, PyString>> {
        let at = (self.originals)
            .binary_search_by_key(&number, |&(known, _)| known)
            .ok()?;
        Some(&self.originals[at].1)
    }
}

/// The multiword tokens of a document: the text of every token and its words, consecutive words
/// of the `count` words of the document in order
fn multiword_tokens(
    document: &Bound<'_, PyAny>,
    count: usize,
) -> PyResult<Vec<(String, Range<usize>)>> {
    let py = document.py();
    let unordered = || {
        PyValueError::new_err(
            "The words of the multiword tokens must be consecutive words of the document in order",
        )
    };
    let (mut tokens, mut next) = (Vec::new(), 0);
    for token in document
        .getattr(intern!(py, "multiword_tokens"))?
        .try_iter()?
    {
        let token = token?;
        let text = token.getattr(intern!(py, "text"))?;
        let Ok(text) = text.cast::<PyString>() else {
            return Err(type_error("The text of a token must be a string", &text));
        };
        let text = lossy(text)?.0;
        let mut range: Option<Range<usize>> = None;
        for word in token.getattr(intern!(py, "words"))?.try_iter()? {
            let index = word?.getattr(intern!(py, "index"))?;
            let index: usize = match index.extract::<isize>() {
                // An index past isize is out of the document
                Err(err) if err.is_instance_of::<PyOverflowError>(py) => return Err(unordered()),
                Err(_) => return Err(type_error("The index of a word must be an integer", &index)),
                Ok(index) => index.try_into().map_err(|_| unordered())?,
            };
            range = match range {
                None if index >= next => Some(index..index + 1),
                Some(range) if index == range.end => Some(range.start..index + 1),
                _ => return Err(unordered()),
            };
        }
        // A token without words gives no words
        let Some(range) = range else { continue };
        if range.end > count {
            return Err(unordered());
        }
        next = range.end;
        tokens.push((text, range));
    }
    Ok(tokens)
}

/// A Python string as a Rust string, and whether it changed: a lone surrogate (`surrogateescape`,
/// `os.fsdecode`) is U+FFFD
fn lossy(word: &Bound<'_, PyString>) -> PyResult<(String, bool)> {
    if let Ok(text) = word.to_str() {
        return Ok((text.to_owned(), false));
    }
    let encoded = word.call_method1("encode", ("utf-16-le", "surrogatepass"))?;
    let bytes = encoded.extract::<Vec<u8>>()?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    Ok((String::from_utf16_lossy(&units), true))
}

/// A word as a Rust string; lone surrogates are replaced with U+FFFD
fn text(word: &Bound<'_, PyAny>) -> PyResult<String> {
    let Ok(word) = word.cast::<PyString>() else {
        return Err(type_error("A word must be a string", word));
    };
    Ok(lossy(word)?.0)
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
    m.add_function(wrap_pyfunction!(_analyzer, m)?)?;
    m.add_function(wrap_pyfunction!(_main, m)?)?;
    m.add("__version__", package_version())?;
    m.add(
        "__all__",
        ["Analysis", "Analyzer", "FormNotFoundError", "__version__"],
    )?;
    Ok(())
}
