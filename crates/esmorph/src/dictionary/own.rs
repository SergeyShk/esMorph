//! Own words: a word inflects as a word of the dictionary, its sample

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fst::{Map, MapBuilder};

use super::{Analysis, Dictionary, Entry, Lexeme, Source, analyses_at, normalized};
use crate::Error;
use crate::alphabet::{Key, MAX_KEY};
use crate::format::{LEMMA_ONLY, OLD, SPOKEN, UD_BITS, VARIANT};

/// Sample of an own word: a word of the dictionary in the same form as the own word (mostly the
/// lemma) and the UD part of speech if the sample has lexemes of several (`bajo` is ADJ, ADP, ADV
/// and NOUN), or several parts of speech separated by commas (`"ADJ,NOUN"`)
///
/// Made from a word (`"bloguear".into()`) or a pair of a word and a part of speech
/// (`("bajo", "NOUN").into()`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Sample<'a> {
    /// The word of the dictionary
    pub word: &'a str,
    /// The UD part of speech (`"VERB"`) or several separated by commas (`"ADJ,NOUN"`): needed if
    /// the sample has lexemes of several
    pub pos: Option<&'a str>,
}

impl<'a> From<&'a str> for Sample<'a> {
    fn from(word: &'a str) -> Self {
        Sample { word, pos: None }
    }
}

impl<'a> From<(&'a str, &'a str)> for Sample<'a> {
    fn from((word, pos): (&'a str, &'a str)) -> Self {
        Sample {
            word,
            pos: Some(pos),
        }
    }
}

/// Own words: lexemes of their own stems with the forms of their samples, found by the keys of
/// their forms. A form is the key of its stem with the key of the ending of its sample, so neither
/// spellings nor keys of forms are kept but in the automaton of keys
#[derive(Clone, Default)]
pub(super) struct OwnWords {
    /// Keys of the endings of samples with their accents, each once
    endings: Vec<(Box<[u8]>, u64)>,
    /// Forms of the lexemes of samples, each shared by the own words of its sample
    samples: Vec<SampleForms>,
    /// Keys of the stems of own words one after another
    stems: Vec<u8>,
    /// Lexemes of own words in the order they were added
    lexemes: Vec<OwnLexeme>,
    /// The key of every form of an own word: the number of its lexeme or, with GROUP, the place
    /// of a group
    keys: Map<Vec<u8>>,
    /// Lexemes that have forms of one key, in the order they were added: each group is its length
    /// and its lexemes
    groups: Vec<u32>,
}

/// The bit of a value of `OwnWords::keys` that gives the place of a group of lexemes
const GROUP: u64 = 1 << 32;

/// The forms of a lexeme of a sample that own words take
#[derive(Clone)]
struct SampleForms {
    table: u16,
    /// The stem of the sample, which with the table and the forms tells one sample from another
    stem: Box<str>,
    forms: Box<[SampleForm]>,
}

/// A form of a sample: its number in the table, the number of its ending in `OwnWords::endings`,
/// and the share and the place of the form among the analyses of the spelling of the sample
#[derive(Clone)]
struct SampleForm {
    form: u16,
    place: u16,
    ending: u32,
    share: f32,
}

/// A lexeme of an own word: the accents and the place and the length of the key of its stem in
/// `OwnWords::stems`, and the number of its sample
#[derive(Clone)]
struct OwnLexeme {
    marks: u64,
    stem: u32,
    sample: u32,
    len: u8,
}

/// A form of an own word of a key: the accents of the key, which with the key give its spelling,
/// the form of a table, and the share and the place of that form among the analyses of the
/// spelling of the sample
pub(super) struct OwnForm {
    marks: u64,
    entry: Entry,
    share: f32,
    place: u16,
}

/// A lexeme of an own word as `sample_forms` finds it: the stem of the word, the table and the stem
/// of the sample, and the forms of the table it takes with the numbers of their endings
struct OwnSpec {
    stem: String,
    table: u16,
    like: String,
    forms: Vec<(u16, u32)>,
}

impl OwnWords {
    pub(super) fn is_empty(&self) -> bool {
        self.lexemes.is_empty()
    }

    /// Whether the sample of a lexeme has a form of the ending `rest`
    fn has_ending(&self, lexeme: u32, rest: &[u8]) -> bool {
        let sample = (self.lexemes.get(lexeme as usize))
            .and_then(|lexeme| self.samples.get(lexeme.sample as usize));
        (sample.into_iter().flat_map(|sample| sample.forms.iter())).any(|form| {
            (self.endings.get(form.ending as usize)).is_some_and(|(ending, _)| **ending == *rest)
        })
    }

    /// The key of the stem of a lexeme
    fn stem(&self, lexeme: &OwnLexeme) -> &[u8] {
        let start = lexeme.stem as usize;
        self.stems
            .get(start..start + usize::from(lexeme.len))
            .unwrap_or_default()
    }

    /// The forms of the own words of the bytes of a key; a form of two lexemes of one table and
    /// the same accents is given once, by the lexeme added first
    fn forms(&self, bytes: &[u8]) -> Vec<OwnForm> {
        let Some(value) = self.keys.get(bytes) else {
            return Vec::new();
        };
        let single = [value as u32];
        let lexemes: &[u32] = if value & GROUP == 0 {
            &single
        } else {
            let at = (value & !GROUP) as usize;
            let len = self.groups.get(at).map_or(0, |&len| len as usize);
            self.groups.get(at + 1..at + 1 + len).unwrap_or_default()
        };
        let mut found: Vec<OwnForm> = Vec::new();
        for &number in lexemes {
            let Some(lexeme) = self.lexemes.get(number as usize) else {
                continue;
            };
            let stem = self.stem(lexeme);
            let (Some(rest), Some(sample)) = (
                bytes.strip_prefix(stem),
                self.samples.get(lexeme.sample as usize),
            ) else {
                continue;
            };
            for form in sample.forms.iter() {
                let Some((ending, ending_marks)) = self.endings.get(form.ending as usize) else {
                    continue;
                };
                if **ending != *rest {
                    continue;
                }
                let marks = lexeme.marks | ending_marks << stem.len();
                let entry = Entry {
                    table: sample.table,
                    form: form.form,
                };
                if !(found.iter()).any(|own| own.entry == entry && own.marks == marks) {
                    found.push(OwnForm {
                        marks,
                        entry,
                        share: form.share,
                        place: form.place,
                    });
                }
            }
        }
        found
    }

    /// Builds the automaton of the keys of the forms of all the lexemes
    fn index(&mut self) -> Result<(), Error> {
        // The keys in one buffer, each its length and its bytes, as their places and lexemes: a key
        // is a stem and an ending. The buffers are sized first, as they take the most memory
        let forms = |lexeme: &OwnLexeme| {
            let sample = self.samples.get(lexeme.sample as usize);
            (sample.into_iter().flat_map(|sample| sample.forms.iter()))
                .filter_map(|form| self.endings.get(form.ending as usize))
        };
        let (count, size) = (self.lexemes.iter()).fold((0, 0), |(count, size), lexeme| {
            forms(lexeme).fold((count, size), |(count, size), (ending, _)| {
                (count + 1, size + 1 + usize::from(lexeme.len) + ending.len())
            })
        });
        let too_many = || Error::Word("the own words have too many forms".into());
        if u32::try_from(size).is_err() {
            return Err(too_many());
        }
        let mut bytes = Vec::with_capacity(size);
        let mut keys: Vec<(u32, u32)> = Vec::with_capacity(count);
        for (number, lexeme) in self.lexemes.iter().enumerate() {
            for (ending, _) in forms(lexeme) {
                keys.push((bytes.len() as u32, number as u32));
                bytes.push(lexeme.len + ending.len() as u8);
                bytes.extend_from_slice(self.stem(lexeme));
                bytes.extend_from_slice(ending);
            }
        }
        let key = |&(start, _): &(u32, u32)| {
            let start = start as usize;
            &bytes[start + 1..start + 1 + usize::from(bytes[start])]
        };
        keys.sort_unstable_by(|a, b| key(a).cmp(key(b)).then(a.1.cmp(&b.1)));
        keys.dedup_by(|later, first| later.1 == first.1 && key(later) == key(first));
        let mut builder = MapBuilder::memory();
        let mut groups: Vec<u32> = Vec::new();
        for run in keys.chunk_by(|a, b| key(a) == key(b)) {
            let value = match run {
                [(_, lexeme)] => u64::from(*lexeme),
                _ => {
                    let at = groups.len() as u64;
                    groups.push(run.len() as u32);
                    groups.extend(run.iter().map(|&(_, lexeme)| lexeme));
                    GROUP | at
                }
            };
            (builder.insert(key(&run[0]), value))
                .map_err(|error| Error::Word(format!("the keys of own words: {error}")))?;
        }
        self.keys = builder.into_map();
        self.groups = groups;
        // The vectors grew by doubling: an analyzer keeps them as long as it lives
        self.endings.shrink_to_fit();
        self.samples.shrink_to_fit();
        self.stems.shrink_to_fit();
        self.lexemes.shrink_to_fit();
        self.groups.shrink_to_fit();
        Ok(())
    }
}

impl Dictionary {
    /// The dictionary with own words: a word takes the forms of the lexemes of its sample with its
    /// own stem (`tiktokear` like `bloguear` gives `tiktokeo`, `tiktokeó`, `tiktokeando`) and
    /// analyses as a word of the dictionary. The word and the sample are taken as `analyze` takes a
    /// word. A sample that is a lemma gives its lexemes of that lemma; the lexemes of one part of
    /// speech are all taken. A sample may be an own word added before. A dictionary with and
    /// without own words: `dictionary.clone().with_words(...)`, the copy shares the mapped file.
    /// Every call builds the lookup of all own words anew: many words are added in one call
    ///
    /// ```
    /// # #[cfg(feature = "bundled")] {
    /// use esmorph::Dictionary;
    ///
    /// let dictionary = Dictionary::bundled()?.with_words([("tiktokear", "bloguear")])?;
    /// assert_eq!(dictionary.analyze("tiktokeamos")[0].lemma, "tiktokear");
    /// # }
    /// # Ok::<(), esmorph::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::Word`] if the sample is not in the dictionary, has lexemes of several parts of
    /// speech and none is given, has no lexemes of a given part of speech, the word does not
    /// inflect as the sample (the sample has another ending or the word has no stem), or the word
    /// is no token: it has a space or a control character
    pub fn with_words<'a, S: Into<Sample<'a>>>(
        mut self,
        words: impl IntoIterator<Item = (&'a str, S)>,
    ) -> Result<Self, Error> {
        // The endings, the samples and the lexemes known: words of one sample share them, and a
        // lexeme added again is not repeated
        let mut endings: HashMap<(Box<[u8]>, u64), u32> = (self.own_words.endings.iter())
            .enumerate()
            .map(|(number, ending)| (ending.clone(), number as u32))
            .collect();
        let mut samples: HashMap<(u16, Box<str>, Vec<u16>), u32> = (self.own_words.samples.iter())
            .enumerate()
            .map(|(number, sample)| {
                let forms = sample.forms.iter().map(|form| form.form).collect();
                ((sample.table, sample.stem.clone(), forms), number as u32)
            })
            .collect();
        let mut lexemes: HashSet<(Box<[u8]>, u64, u32)> = (self.own_words.lexemes.iter())
            .map(|lexeme| {
                (
                    self.own_words.stem(lexeme).into(),
                    lexeme.marks,
                    lexeme.sample,
                )
            })
            .collect();
        // Lexemes of this call that the lookup has not yet, by the keys of their stems: a sample
        // that may be a form of one of them is looked for after the lookup is built anew
        let mut fresh: HashMap<Box<[u8]>, Vec<u32>> = HashMap::new();
        for (word, sample) in words {
            let sample = sample.into();
            let own = &self.own_words;
            let key = (!fresh.is_empty())
                .then(|| {
                    self.alphabet
                        .input_key(&normalized(sample.word).to_lowercase())
                })
                .flatten();
            let of_fresh = key.is_some_and(|key| {
                let diaeresis = self.alphabet.diaeresis_keys(&key);
                (std::iter::once(&key).chain(&diaeresis)).any(|key| {
                    let bytes = key.bytes();
                    (1..=bytes.len()).any(|len| {
                        (fresh.get(&bytes[..len])).is_some_and(|lexemes| {
                            (lexemes.iter()).any(|&lexeme| own.has_ending(lexeme, &bytes[len..]))
                        })
                    })
                })
            });
            if of_fresh {
                Arc::make_mut(&mut self.own_words).index()?;
                fresh.clear();
            }
            let specs = self.sample_forms(word, sample)?;
            for spec in &specs {
                let forms: Vec<u16> = spec.forms.iter().map(|&(form, _)| form).collect();
                let id = (spec.table, Box::from(spec.like.as_str()), forms);
                let sample = match samples.get(&id) {
                    Some(&number) => number,
                    None => {
                        let shares = self.sample_form_shares(spec);
                        let own = Arc::make_mut(&mut self.own_words);
                        let forms = (shares.into_iter())
                            .map(|(form, key, share, place)| {
                                let ending = (Box::<[u8]>::from(key.bytes()), key.marks);
                                let number = *endings.entry(ending).or_insert_with_key(|ending| {
                                    own.endings.push(ending.clone());
                                    (own.endings.len() - 1) as u32
                                });
                                SampleForm {
                                    form,
                                    place,
                                    ending: number,
                                    share,
                                }
                            })
                            .collect();
                        own.samples.push(SampleForms {
                            table: spec.table,
                            stem: id.1.clone(),
                            forms,
                        });
                        let number = (own.samples.len() - 1) as u32;
                        samples.insert(id, number);
                        number
                    }
                };
                // Letters that the dictionary has not take free bytes of the alphabet of the copy
                if !self.alphabet.extend(&spec.stem) {
                    return Err(Error::Word(format!(
                        "the word {word:?} has too many letters that the dictionary has not"
                    )));
                }
                let longest = (spec.forms.iter())
                    .filter_map(|&(_, suffix)| self.suffixes.get(suffix as usize))
                    .max_by_key(|ending| ending.chars().count())
                    .unwrap_or_default();
                let fits = (self.alphabet.input_key(&spec.stem))
                    .filter(|key| key.bytes().len() + longest.chars().count() <= MAX_KEY);
                let Some(key) = fits else {
                    return Err(Error::Word(format!(
                        "the word {:?} is longer than {MAX_KEY} letters",
                        format!("{}{longest}", spec.stem)
                    )));
                };
                if lexemes.insert((key.bytes().into(), key.marks, sample)) {
                    let own = Arc::make_mut(&mut self.own_words);
                    let number = own.lexemes.len() as u32;
                    fresh.entry(key.bytes().into()).or_default().push(number);
                    own.lexemes.push(OwnLexeme {
                        marks: key.marks,
                        stem: own.stems.len() as u32,
                        sample,
                        len: key.bytes().len() as u8,
                    });
                    own.stems.extend_from_slice(key.bytes());
                }
            }
        }
        Arc::make_mut(&mut self.own_words).index()?;
        Ok(self)
    }

    /// The forms of a sample with the keys of their endings, and the shares and the places of the
    /// forms among the analyses of the spellings of the sample: in `blogueaba` the third person
    /// goes first
    fn sample_form_shares(&self, spec: &OwnSpec) -> Vec<(u16, Key, f32, u16)> {
        // The forms and the shares of the analyses of each spelling of the sample: forms of one
        // spelling share them
        let mut spelled: HashMap<String, Vec<(Option<Entry>, f64)>> = HashMap::new();
        let mut forms = Vec::with_capacity(spec.forms.len());
        for &(form, suffix) in &spec.forms {
            let Some((ending, key)) = (self.suffixes.get(suffix as usize))
                .and_then(|ending| Some((ending, self.alphabet.input_key(ending)?)))
            else {
                continue;
            };
            let like = format!("{}{ending}", spec.like);
            let analyses = (spelled.entry(like)).or_insert_with_key(|like| {
                (self.spelled(like).0.into_iter())
                    .map(|analysis| (analysis.entry, analysis.prob))
                    .collect()
            });
            let entry = Entry {
                table: spec.table,
                form,
            };
            let (share, place) = (analyses.iter().enumerate())
                .find(|(_, (found, _))| *found == Some(entry))
                .map_or(
                    (1.0 / analyses.len().max(1) as f64, analyses.len()),
                    |(at, &(_, prob))| (prob, at),
                );
            let place = u16::try_from(place).unwrap_or(u16::MAX);
            forms.push((form, key, share as f32, place));
        }
        forms
    }

    /// Lexemes of the word `word` like the sample: the stem of the word, the table and the stem of
    /// the sample, and the forms of the table with their endings, without the spoken spellings
    /// and, but of a verb (`hablás` of `hablas`), without the variants: those of a noun are other
    /// spellings (`yoghurt` of `yogur`)
    fn sample_forms(&self, word: &str, sample: Sample<'_>) -> Result<Vec<OwnSpec>, Error> {
        let normal = |text: &str| normalized(text).to_lowercase();
        let (word, like) = (normal(word), normal(sample.word));
        if word.is_empty() || word.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(Error::Word(format!(
                "the own word {word:?} is empty or has a space or a control character: no token \
                 of a text is such"
            )));
        }
        let pos = |analysis: &Analysis| self.ud.get(analysis.ud).map_or("", |tag| tag.pos());
        let mut analyses: Vec<Analysis> = (self.found(&like).into_iter())
            .filter(|analysis| analysis.entry.is_some())
            .collect();
        if analyses.is_empty() {
            // A reflexive infinitive is in the dictionary without `se`
            let verb = (like.strip_suffix("se"))
                .filter(|verb| verb.ends_with('r') && !self.found(verb).is_empty());
            return Err(Error::Word(match verb {
                Some(verb) => format!(
                    "the sample {like:?} is not in the dictionary: give the verb without se, {verb:?}"
                ),
                None => format!("the sample {like:?} is not in the dictionary"),
            }));
        }
        let mut found: Vec<&str> = analyses.iter().map(pos).collect();
        found.sort_unstable();
        found.dedup();
        if let Some(wanted) = sample.pos {
            let wanted: Vec<&str> = wanted.split(',').map(str::trim).collect();
            if let Some(missing) = wanted.iter().find(|wanted| !found.contains(wanted)) {
                return Err(Error::Word(format!(
                    "the sample {like:?} has no lexemes of {missing}, only of {}",
                    found.join(", ")
                )));
            }
            analyses.retain(|analysis| wanted.contains(&pos(analysis)));
        }
        // A sample that is a lemma gives its lexemes of that lemma: `bajo` of `bajar` is no sample
        // of a noun; of several parts of speech given, within each (`helado` as a noun and `helar`)
        if sample.pos.is_some() {
            let of_lemma: Vec<&str> = (analyses.iter())
                .filter(|analysis| analysis.lemma == like)
                .map(pos)
                .collect();
            analyses
                .retain(|analysis| analysis.lemma == like || !of_lemma.contains(&pos(analysis)));
        } else if analyses.iter().any(|analysis| analysis.lemma == like) {
            analyses.retain(|analysis| analysis.lemma == like);
        }
        if sample.pos.is_none() {
            let mut kept: Vec<&str> = analyses.iter().map(pos).collect();
            kept.sort_unstable();
            kept.dedup();
            if kept.len() > 1 {
                return Err(Error::Word(format!(
                    "the sample {like:?} has lexemes of several parts of speech ({}): give the \
                     part of speech",
                    kept.join(", ")
                )));
            }
        }
        let mut specs: Vec<OwnSpec> = Vec::new();
        // The endings of the sample that the word needs, for the error
        let mut endings: Vec<&str> = Vec::new();
        for analysis in &analyses {
            let Some(entry) = analysis.entry else {
                continue;
            };
            let Some(sample_lexeme) = self.lexeme(entry, &analysis.word) else {
                continue;
            };
            if let Some(ending) = analysis.word.strip_prefix(sample_lexeme.stem)
                && !endings.contains(&ending)
            {
                endings.push(ending);
            }
            let Some(lexeme) = self
                .lexeme(entry, &word)
                .filter(|lexeme| !lexeme.stem.is_empty())
            else {
                continue;
            };
            if (specs.iter()).any(|spec| spec.table == entry.table && spec.stem == lexeme.stem) {
                continue;
            }
            // An own verb takes the forms of `vos`, not the old ones (`truje` of `traer`)
            let skipped = match pos(analysis) {
                "VERB" | "AUX" => LEMMA_ONLY | SPOKEN | OLD,
                _ => LEMMA_ONLY | SPOKEN | VARIANT,
            };
            // A variant is kept if the sample is that form (`film`, `buen`) or no main form has its
            // number (`sexys` of `sexy`): not an abbreviation or a spelling of the main form
            let number = |cell: u32| {
                (self.ud.get((cell & UD_BITS) as u16)).and_then(|tag| tag.feat("Number"))
            };
            let main = |form: usize| {
                if form == usize::from(entry.form) {
                    return false;
                }
                let wanted = number(lexeme.cells[form]);
                (lexeme.cells.iter()).any(|&main| {
                    main & (LEMMA_ONLY | SPOKEN | VARIANT) == 0 && number(main) == wanted
                })
            };
            let forms: Vec<(u16, u32)> = (0..lexeme.cells.len())
                .filter(|&form| {
                    let cell = lexeme.cells[form];
                    !(cell & skipped != 0
                        && (cell & (LEMMA_ONLY | SPOKEN | OLD) != 0 || main(form)))
                })
                .filter_map(|form| Some((form as u16, *lexeme.paradigm.get(form)?)))
                .filter(|&(_, suffix)| self.suffixes.get(suffix as usize).is_some())
                .collect();
            if !forms.is_empty() {
                specs.push(OwnSpec {
                    stem: lexeme.stem.to_owned(),
                    table: entry.table,
                    like: sample_lexeme.stem.to_owned(),
                    forms,
                });
            }
        }
        if specs.is_empty() {
            let endings: Vec<String> = endings.iter().map(|ending| format!("{ending:?}")).collect();
            return Err(Error::Word(format!(
                "{word:?} does not inflect as {like:?}: the word needs the ending {} of the sample \
                 and a stem before it",
                endings.join(" or ")
            )));
        }
        Ok(specs)
    }

    /// The analyses of a word with its own words: an own word of the exact spelling, or of any
    /// spelling of the key if neither the dictionary nor the own words have the exact one, as the
    /// spellings of the dictionary; then with a lost diaeresis restored. The analyses of an own
    /// spelling take the order and the shares of the sample; together they take the mean share of
    /// the analyses of the dictionary each and go after them on equal shares; one that the
    /// dictionary has is not repeated
    pub(super) fn with_own(
        &self,
        word: &str,
        mut analyses: Vec<Analysis>,
        exact: bool,
    ) -> (Vec<Analysis>, bool) {
        // A lost diaeresis is not looked for in a word the dictionary has as written: `guiso` is
        // no own `güiso`
        let Some((key, forms, as_written)) =
            (self.alphabet.input_key(word)).and_then(|key| self.own_forms_of(&key, !exact))
        else {
            return (analyses, exact);
        };
        let own_exact = as_written && forms.iter().any(|own| own.marks == key.marks);
        // The analyses of the dictionary with other accents give way to the exact own word
        if own_exact && !exact {
            analyses.clear();
        }
        let exact = exact || own_exact;
        let mut chosen: Vec<&OwnForm> = (forms.iter())
            .filter(|own| !exact || own.marks == key.marks)
            .collect();
        chosen.sort_by_key(|own| own.place);
        let total: f64 = chosen.iter().map(|own| f64::from(own.share)).sum();
        let scale = chosen.len() as f64 / analyses.len().max(1) as f64;
        let known = analyses.len();
        for own in chosen {
            let share = if total > 0.0 {
                f64::from(own.share) / total * scale
            } else {
                scale
            };
            let spelling = self.alphabet.spelling(&key, own.marks);
            let Some(analysis) = self.analysis(spelling, own.entry, share) else {
                continue;
            };
            let repeated = analyses[..known].iter().any(|other| {
                other.word == analysis.word
                    && other.lemma == analysis.lemma
                    && other.ud == analysis.ud
            });
            if !repeated {
                analyses.push(analysis);
            }
        }
        let sum: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
        if sum > 0.0 {
            for analysis in &mut analyses {
                analysis.prob /= sum;
            }
        }
        analyses.sort_by(|a, b| b.prob.total_cmp(&a.prob));
        (analyses, exact)
    }

    /// The key of a word, the own words of it and whether the key is as written: the key as written,
    /// then with a lost diaeresis restored
    fn own_forms(&self, word: &str) -> Option<(Key, Vec<OwnForm>, bool)> {
        self.own_forms_of(&self.alphabet.input_key(word)?, true)
    }

    /// The own words of a key as written and, with `diaeresis`, of the key with a lost diaeresis
    /// restored: the key found, its own words and whether it is as written
    fn own_forms_of(&self, key: &Key, diaeresis: bool) -> Option<(Key, Vec<OwnForm>, bool)> {
        let forms = self.own_words.forms(key.bytes());
        if !forms.is_empty() {
            return Some((key.clone(), forms, true));
        }
        if !diaeresis {
            return None;
        }
        (self.alphabet.diaeresis_keys(key).into_iter()).find_map(|key| {
            let forms = self.own_words.forms(key.bytes());
            (!forms.is_empty()).then_some((key, forms, false))
        })
    }

    /// Whether a form of a table is a form of its lexeme for `forms` and `inflect`: no lemma only and
    /// no spoken spelling; of the lexeme of an own word (`own`), a variant only if the dictionary
    /// or the own words have it, as an own word that is no verb takes no variants of its sample
    /// (`yoghurt` of `yogur`)
    pub(super) fn takes_form(&self, lexeme: &Lexeme<'_>, form: usize, own: bool) -> bool {
        let cell = lexeme.cells[form];
        if cell & (LEMMA_ONLY | SPOKEN) != 0 {
            return false;
        }
        if !own || cell & VARIANT == 0 {
            return true;
        }
        let entry = Entry {
            table: lexeme.table,
            form: form as u16,
        };
        lexeme.spell(&self.suffixes, form).is_some_and(|spelling| {
            self.in_automaton(&spelling, entry) || self.in_own_words(&spelling, entry)
        })
    }

    /// Whether an analysis is of an own word: of the dictionary or of a verb with clitics, its form
    /// is no record of the automaton but one of the own words
    pub(super) fn is_own_analysis(&self, analysis: &Analysis) -> bool {
        if self.own_words.is_empty() {
            return false;
        }
        let word = match analysis.source {
            Source::Dictionary => Some(&analysis.word),
            Source::Clitics => analysis.words.first(),
            _ => None,
        };
        let (Some(word), Some(entry)) = (word, analysis.entry) else {
            return false;
        };
        !self.in_automaton(word, entry) && self.in_own_words(word, entry)
    }

    /// Whether the automaton has the form `entry` spelled exactly as `spelling`
    fn in_automaton(&self, spelling: &str, entry: Entry) -> bool {
        let packed = (u32::from(entry.table) << 16) | u32::from(entry.form);
        self.lookup(spelling)
            .is_some_and(|(key, spellings, as_written)| {
                as_written
                    && spellings.iter().any(|(marks, records, _)| {
                        marks == key.marks
                            && analyses_at(self.records(), records)
                                .is_some_and(|(records, _)| records.contains(&packed))
                    })
            })
    }

    /// Whether the own words have the form `entry` spelled exactly as `spelling`
    fn in_own_words(&self, spelling: &str, entry: Entry) -> bool {
        self.own_forms(spelling)
            .is_some_and(|(key, forms, as_written)| {
                as_written && (forms.iter()).any(|own| own.marks == key.marks && own.entry == entry)
            })
    }

    /// Whether an own word is spelled as `word` with any accents or with a lost diaeresis
    pub(super) fn is_own(&self, word: &str) -> bool {
        !self.own_words.is_empty() && self.own_forms(word).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;
    use crate::builder::Builder;

    fn dictionary() -> Dictionary {
        let mut builder = Builder::new();
        builder.meta("{}");
        let forms = [
            ("bloguear", "VERB", "bloguear", "VerbForm=Inf"),
            (
                "bloguear",
                "VERB",
                "blogueo",
                "Mood=Ind|Number=Sing|Person=1|Tense=Pres|VerbForm=Fin",
            ),
            (
                "bloguear",
                "VERB",
                "blogueó",
                "Mood=Ind|Number=Sing|Person=3|Tense=Past|VerbForm=Fin",
            ),
            ("bajo", "ADJ", "bajo", "Gender=Masc|Number=Sing"),
            ("bajo", "ADJ", "bajos", "Gender=Masc|Number=Plur"),
            ("bajo", "NOUN", "bajo", "Gender=Masc|Number=Sing"),
            ("bajo", "NOUN", "bajos", "Gender=Masc|Number=Plur"),
            ("casa", "NOUN", "casa", "Gender=Fem|Number=Sing"),
            ("casa", "NOUN", "casas", "Gender=Fem|Number=Plur"),
        ];
        for (lemma, pos, form, feats) in forms {
            builder.add_form(lemma, pos, form, feats).unwrap();
        }
        Dictionary::from_bytes(&builder.build().unwrap()).unwrap()
    }

    fn first(dictionary: &Dictionary, word: &str) -> (String, String) {
        let analysis = dictionary.analyze(word).remove(0);
        (analysis.word, analysis.lemma)
    }

    #[test]
    fn an_own_word_inflects_as_its_sample() {
        let base = dictionary();
        let dictionary = base.clone().with_words([("tuitear", "bloguear")]).unwrap();
        assert_eq!(
            first(&dictionary, "tuiteó"),
            ("tuiteó".into(), "tuitear".into())
        );
        assert_eq!(
            first(&dictionary, "TUITEO"),
            ("tuiteo".into(), "tuitear".into())
        );
        let analysis = dictionary.analyze("tuiteo").remove(0);
        assert_eq!(analysis.source, Source::Dictionary);
        let forms: Vec<String> = (dictionary.forms(&analysis).into_iter())
            .map(|form| form.word)
            .collect();
        assert_eq!(forms, ["tuitear", "tuiteo", "tuiteó"]);
        let past = dictionary.inflect(&analysis, &[("Tense", "Past")]).unwrap();
        assert_eq!(past.word, "tuiteó");
        // A verb with clitics and a word without its accent, as words of the dictionary
        assert_eq!(dictionary.analyze("tuitearlo")[0].words, ["tuitear", "lo"]);
        assert_eq!(
            first(&dictionary, "tuiteo"),
            ("tuiteo".into(), "tuitear".into())
        );
        assert!(dictionary.is_known("tuitearlo") && dictionary.is_known("tuiteó"));
        // The copy without own words does not know them
        assert!(!base.is_known("tuiteó"));
        assert!(
            (base.analyze("tuiteó").iter()).all(|analysis| analysis.source != Source::Dictionary)
        );
    }

    #[test]
    fn an_own_word_with_letters_out_of_the_dictionary() {
        let dictionary = dictionary().with_words([("wikear", "bloguear")]).unwrap();
        assert_eq!(
            first(&dictionary, "wikeó"),
            ("wikeó".into(), "wikear".into())
        );
        // A sample may be an own word, also of the same call
        let dictionary = dictionary.with_words([("zapear", "wikear")]).unwrap();
        assert_eq!(
            first(&dictionary, "zapeo"),
            ("zapeo".into(), "zapear".into())
        );
        let together = (super::tests::dictionary())
            .with_words([("wikear", "bloguear"), ("zapear", "wikear")])
            .unwrap();
        assert_eq!(first(&together, "zapeó"), ("zapeó".into(), "zapear".into()));
    }

    #[test]
    fn an_own_word_next_to_a_word_of_the_dictionary() {
        let dictionary = dictionary()
            .with_words([("casa", ("bajo", "ADJ"))])
            .unwrap();
        let analyses = dictionary.analyze("casa");
        assert_eq!(analyses.len(), 2);
        assert_eq!(analyses[0].lemma, "casa");
        assert_eq!(analyses[0].prob, analyses[1].prob);
        // The same analysis is not repeated
        let again = dictionary.with_words([("casa", ("bajo", "ADJ"))]).unwrap();
        assert_eq!(again.analyze("casa").len(), 2);
    }

    #[test]
    fn an_own_word_follows_the_order_and_variants_of_its_sample() {
        let mut builder = Builder::new();
        builder.meta("{}");
        let past =
            |person: &str| format!("Mood=Ind|Number=Sing|Person={person}|Tense=Imp|VerbForm=Fin");
        let (first_person, third_person) = (past("1"), past("3"));
        let (sing, plur) = ("Gender=Masc|Number=Sing", "Gender=Masc|Number=Plur");
        let forms = [
            ("bloguear", "VERB", "bloguear", "VerbForm=Inf"),
            ("bloguear", "VERB", "blogueaba", first_person.as_str()),
            ("bloguear", "VERB", "blogueaba", third_person.as_str()),
            ("yogur", "NOUN", "yogur", sing),
            ("yogur", "NOUN", "yogures", plur),
            ("yogur", "NOUN", "yoghurt", sing),
            ("pingüino", "NOUN", "pingüino", sing),
            ("guiso", "NOUN", "guiso", sing),
        ];
        for (lemma, pos, form, feats) in forms {
            builder.add_form(lemma, pos, form, feats).unwrap();
        }
        builder
            .mark_variant("yogur", "NOUN", "yoghurt", sing)
            .unwrap();
        // The sample has the third person first
        builder
            .place("bloguear", "VERB", "blogueaba", &third_person, 0, Some(0.8))
            .unwrap();
        builder
            .place("bloguear", "VERB", "blogueaba", &first_person, 1, Some(0.2))
            .unwrap();
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        let dictionary = dictionary
            .with_words([
                ("tiktokear", "bloguear"),
                ("zogur", "yogur"),
                ("zingüino", "pingüino"),
            ])
            .unwrap();
        let analyses = dictionary.analyze("tiktokeaba");
        let tag = |analysis: &Analysis| dictionary.ud_tag(analysis.ud).unwrap();
        assert_eq!(tag(&analyses[0]).feat("Person"), Some("3"));
        assert!(
            (analyses[0].prob - 0.8).abs() < 1e-6,
            "{}",
            analyses[0].prob
        );
        // A noun takes no other spellings of its sample: as words, forms and in inflection
        assert!((dictionary.analyze("zoghurt").iter()).all(|a| a.source != Source::Dictionary));
        let forms: Vec<String> = (dictionary
            .forms(&dictionary.analyze("zogur")[0])
            .into_iter())
        .map(|form| form.word)
        .collect();
        assert_eq!(forms, ["zogur", "zogures"]);
        // The variants of the sample itself stay
        let yogur = dictionary.analyze("yogur").remove(0);
        assert_eq!(dictionary.forms(&yogur).len(), 3);
        let zogures = dictionary.analyze("zogures").remove(0);
        let singular = dictionary.inflect(&zogures, &[("Number", "Sing")]).unwrap();
        assert_eq!(singular.word, "zogur");
        // A lost diaeresis
        assert_eq!(
            first(&dictionary, "zinguino"),
            ("zingüino".into(), "zingüino".into())
        );
        assert!(dictionary.is_known("zinguino"));
        // but not in a word of the dictionary
        let dictionary = dictionary.with_words([("güiso", "pingüino")]).unwrap();
        assert_eq!(dictionary.analyze("guiso").len(), 1);
        assert_eq!(
            first(&dictionary, "güiso"),
            ("güiso".into(), "güiso".into())
        );
        // The error names the ending the word needs
        let error = (dictionary.with_words([("zogurt", "yogur")])).unwrap_err();
        assert!(
            error.to_string().contains("needs the ending \"ur\""),
            "{error}"
        );
    }

    #[test]
    fn a_variant_that_is_the_sample_or_alone_in_its_number() {
        let mut builder = Builder::new();
        builder.meta("{}");
        let (sing, plur) = ("Gender=Masc|Number=Sing", "Gender=Masc|Number=Plur");
        let forms = [
            ("film", "NOUN", "film", sing),
            ("film", "NOUN", "films", plur),
            ("sexy", "ADJ", "sexy", "Number=Sing"),
            ("sexy", "ADJ", "sexys", "Number=Plur"),
            ("altura", "NOUN", "altura", "Gender=Fem|Number=Sing"),
            ("altura", "NOUN", "altor", sing),
            ("gran", "ADJ", "gran", "Number=Sing"),
            ("bueno", "ADJ", "bueno", sing),
            ("bueno", "ADJ", "buen", sing),
            ("bueno", "ADJ", "buenos", plur),
        ];
        for (lemma, pos, form, feats) in forms {
            builder.add_form(lemma, pos, form, feats).unwrap();
        }
        builder.mark_variant("film", "NOUN", "film", sing).unwrap();
        builder
            .mark_variant("gran", "ADJ", "gran", "Number=Sing")
            .unwrap();
        builder.mark_variant("bueno", "ADJ", "buen", sing).unwrap();
        for (form, feats) in [("sexy", "Number=Sing"), ("sexys", "Number=Plur")] {
            builder.mark_variant("sexy", "ADJ", form, feats).unwrap();
        }
        builder
            .mark_variant("altura", "NOUN", "altor", sing)
            .unwrap();
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        let own = (dictionary.clone())
            .with_words([
                ("podcast", "film"),
                ("zqgran", "gran"),
                ("zqbuen", "buen"),
                ("cringy", "sexy"),
                ("megaaltura", "altura"),
            ])
            .unwrap();
        for word in [
            "podcast", "podcasts", "zqgran", "zqbuen", "zqbueno", "cringys",
        ] {
            assert!(
                (own.analyze(word).iter()).any(|a| a.source == Source::Dictionary),
                "{word}"
            );
        }
        // A variant beside a main form of its number stays out: `altor` is no form of `altura`
        assert!((own.analyze("megaaltor").iter()).all(|a| a.source != Source::Dictionary));
        let dictionary = dictionary.with_words([("zqbueno", "bueno")]).unwrap();
        let forms: Vec<String> = (dictionary
            .forms(&dictionary.analyze("zqbuenos")[0])
            .into_iter())
        .map(|form| form.word)
        .collect();
        assert_eq!(forms, ["zqbueno", "zqbuenos"]);
    }

    #[test]
    fn samples_of_several_parts_of_speech() {
        let dictionary = dictionary();
        // Both parts of speech of a sample
        let both = (dictionary.clone())
            .with_words([("cajo", ("bajo", "ADJ,NOUN"))])
            .unwrap();
        let mut found: Vec<String> = (both.analyze("cajos").into_iter())
            .map(|analysis| both.ud_tag(analysis.ud).unwrap().pos().to_owned())
            .collect();
        found.sort();
        assert_eq!(found, ["ADJ", "NOUN"]);
        // A part of speech chooses among the analyses of the spelling before the lemma does: the
        // sample need not be the lemma of its part of speech
        let mut builder = Builder::new();
        builder.meta("{}");
        let (sing, plur) = ("Gender=Masc|Number=Sing", "Gender=Masc|Number=Plur");
        for (lemma, pos, form, feats) in [
            ("buen", "ADJ", "buen", sing),
            ("buen", "ADJ", "bueno", sing),
            ("buen", "ADJ", "buenos", plur),
            ("bueno", "INTJ", "bueno", ""),
        ] {
            builder.add_form(lemma, pos, form, feats).unwrap();
        }
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        let own = (dictionary.with_words([("zqbueno", ("bueno", "ADJ"))])).unwrap();
        assert_eq!(
            first(&own, "zqbuenos"),
            ("zqbuenos".into(), "zqbuen".into())
        );
    }

    #[test]
    fn wrong_own_words() {
        let dictionary = dictionary();
        for (word, sample, wanted) in [
            ("tuitear", Sample::from("qzx"), "not in the dictionary"),
            ("bajo", Sample::from("bajo"), "several parts of speech"),
            ("cajo", Sample::from(("bajo", "VERB")), "no lexemes of VERB"),
            (
                "cajo",
                Sample::from(("bajo", "NOUN,VERB")),
                "no lexemes of VERB",
            ),
            (
                "tuitearse",
                Sample::from("bloguearse"),
                "without se, \"bloguear\"",
            ),
            ("tuiteo", Sample::from("bloguear"), "does not inflect"),
            ("ar", Sample::from("bloguear"), "does not inflect"),
            ("tui tear", Sample::from("bloguear"), "no token"),
            ("", Sample::from("bloguear"), "no token"),
        ] {
            let Err(error) = dictionary.clone().with_words([(word, sample)]) else {
                panic!("{word:?} like {sample:?} is no error");
            };
            assert!(matches!(error, Error::Word(_)), "{error}");
            assert!(error.to_string().contains(wanted), "{error}");
        }
        let cajo = dictionary.with_words([("cajo", ("bajo", "NOUN"))]).unwrap();
        assert_eq!(first(&cajo, "cajos"), ("cajos".into(), "cajo".into()));
    }
}
