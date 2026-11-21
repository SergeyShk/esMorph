//! Builder of the dictionary of esMorph: `esmorph-build <export directory> <counts directory>
//! <dictionary file> [<context model> [<paradigm model>]]`, `-` for no context model. The export is `lexemes.tsv` (lemma, part of speech, form,
//! features, `variant`, `old` for an old or nonstandard variant, `spoken:` with the form a spoken
//! spelling stands for, `extra`, `extra-variant` or `extra-old` for a form of the extra source or
//! `_`, the place in the table of the source or `_`), `diminutives.tsv` (lemma, part of speech
//! and the word it is a diminutive of) and `meta.json`, written by
//! `scripts/export_wiktionary.py`; the counts are `analyses.tsv`, `pos.tsv` and `sources.json`,
//! written by `scripts/export_ud.py`; the context model is the file of weights of
//! `scripts/train_context.py`, the paradigm model - of `scripts/train_paradigms.py`. A form of the extra source that the treebanks have only as a name
//! is left out, but an interjection: `ciu` is `CiU`; so is a lexeme of only the extra source with
//! such a lemma (`cius`)

mod frequencies;

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use esmorph::{Builder, Error};
use frequencies::{Counts, Row};

fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|err| Error::Build(format!("{}: {err}", path.display())))
}

/// An error of a file with its path: `counts: analyses.tsv:3: wrong count`
fn within(path: &Path, err: Error) -> Error {
    let message = match err {
        Error::Build(message) | Error::Format(message) => message,
        err => err.to_string(),
    };
    Error::Build(format!("{}: {message}", path.display()))
}

/// An error of a line of a file with its place: `lexemes.tsv:2: word "Casa" is not in lower case`
fn at_line(path: &Path, number: usize, err: Error) -> Error {
    within(
        &path.with_file_name(format!("{}:{}", file_name(path), number + 1)),
        err,
    )
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// Description of the dictionary: the description of its export with the sources of the counts,
/// both objects of JSON
fn joined(meta: &str, sources: &str) -> Result<String, Error> {
    for (name, text) in [("the export", meta), ("the counts", sources)] {
        if !json_object(text) {
            return Err(Error::Build(format!(
                "the description of {name} is not an object of JSON: {text:?}"
            )));
        }
    }
    let fields = meta
        .trim_end()
        .strip_suffix('}')
        .ok_or_else(|| Error::Build(format!("wrong description of the export {meta:?}")))?;
    let separator = if fields.trim_end().ends_with('{') {
        ""
    } else {
        ", "
    };
    Ok(format!(
        "{fields}{separator}\"counts\": {}}}",
        sources.trim_end()
    ))
}

/// The text is an object of JSON
fn json_object(text: &str) -> bool {
    let mut json = Json {
        bytes: text.as_bytes(),
        at: 0,
    };
    // Only the whitespace of JSON may go around it
    json.space();
    if json.bytes.get(json.at) != Some(&b'{') || !json.value(0) {
        return false;
    }
    json.space();
    json.at == json.bytes.len()
}

/// A reader of JSON that only checks it
struct Json<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Json<'_> {
    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn take(&mut self, byte: u8) -> bool {
        self.space();
        let found = self.bytes.get(self.at) == Some(&byte);
        self.at += usize::from(found);
        found
    }

    fn value(&mut self, depth: usize) -> bool {
        self.space();
        match self.bytes.get(self.at) {
            _ if depth > 64 => false,
            Some(b'{') => self.items(b'}', |json| {
                json.space();
                json.string() && json.take(b':') && json.value(depth + 1)
            }),
            Some(b'[') => self.items(b']', |json| json.value(depth + 1)),
            Some(b'"') => self.string(),
            Some(b't') => self.word(b"true"),
            Some(b'f') => self.word(b"false"),
            Some(b'n') => self.word(b"null"),
            Some(_) => self.number(),
            None => false,
        }
    }

    /// The items of an object or an array after its opening bracket
    fn items(&mut self, close: u8, mut item: impl FnMut(&mut Self) -> bool) -> bool {
        self.at += 1;
        if self.take(close) {
            return true;
        }
        loop {
            if !item(self) {
                return false;
            }
            if self.take(close) {
                return true;
            }
            if !self.take(b',') {
                return false;
            }
        }
    }

    fn string(&mut self) -> bool {
        if self.bytes.get(self.at) != Some(&b'"') {
            return false;
        }
        self.at += 1;
        while let Some(&byte) = self.bytes.get(self.at) {
            self.at += 1;
            match byte {
                b'"' => return true,
                b'\\' => match self.bytes.get(self.at) {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => self.at += 1,
                    Some(b'u') => match self.bytes.get(self.at + 1..self.at + 5) {
                        Some(hex) if hex.iter().all(u8::is_ascii_hexdigit) => self.at += 5,
                        _ => return false,
                    },
                    _ => return false,
                },
                0..=0x1F => return false,
                _ => {}
            }
        }
        false
    }

    fn word(&mut self, word: &[u8]) -> bool {
        let found = self.bytes[self.at..].starts_with(word);
        self.at += if found { word.len() } else { 0 };
        found
    }

    /// `-? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?`
    fn number(&mut self) -> bool {
        let start = self.at;
        let digits = |json: &mut Self| {
            let from = json.at;
            while json.bytes.get(json.at).is_some_and(u8::is_ascii_digit) {
                json.at += 1;
            }
            json.at > from
        };
        self.take(b'-');
        let first = self.at;
        if !digits(self) || (self.bytes[first] == b'0' && self.at - first > 1) {
            return false;
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            if !digits(self) {
                return false;
            }
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !digits(self) {
                return false;
            }
        }
        self.at > start
    }
}

fn build(
    export: &Path,
    counts: &Path,
    output: &Path,
    context: Option<&Path>,
    paradigms: Option<&Path>,
) -> Result<usize, Error> {
    let mut builder = Builder::new();
    let mut meta = joined(
        &read(&export.join("meta.json"))?,
        &read(&counts.join("sources.json"))?,
    )?;
    // The weights of a model and the data it was trained on, which the description of the
    // dictionary gets under `key`
    let mut model = |path: &Path,
                     key: &str,
                     sources: fn(&[u8]) -> Result<String, Error>|
     -> Result<Vec<u8>, Error> {
        let weights =
            fs::read(path).map_err(|err| Error::Build(format!("{}: {err}", path.display())))?;
        let sources = sources(&weights).map_err(|err| within(path, err))?;
        if !json_object(&sources) {
            return Err(within(
                path,
                Error::Build("the sources of the model are not an object of JSON".into()),
            ));
        }
        // The description ends with the object of the counts that `joined` closed
        meta.truncate(meta.len() - 1);
        meta.push_str(&format!(", \"{key}\": {sources}}}"));
        Ok(weights)
    };
    if let Some(context) = context {
        let weights = model(context, "context", Builder::context_sources)?;
        (builder.context(weights)).map_err(|err| within(context, err))?;
    }
    if let Some(paradigms) = paradigms {
        let weights = model(paradigms, "paradigms", Builder::paradigm_sources)?;
        (builder.paradigm_model(weights)).map_err(|err| within(paradigms, err))?;
    }
    builder.meta(meta);
    let analyses = counts.join("analyses.tsv");
    let counts = Counts::read(&read(&analyses)?, &read(&counts.join("pos.tsv"))?)
        .map_err(|err| within(counts, err))?;
    let lexemes = export.join("lexemes.tsv");
    let table = read(&lexemes)?;
    // Lexemes of the main source: a name for a lemma leaves out only their forms of names
    let main_source: HashSet<(&str, &str)> = (table.lines())
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let (lemma, pos) = (fields.next()?, fields.next()?);
            let kind = fields.nth(2)?;
            (!kind.starts_with("extra")).then_some((lemma, pos))
        })
        .collect();
    // The rows of every form for its counts, a pair a row: a map of vectors takes much more
    let mut forms: Vec<(&str, Row)> = Vec::new();
    // Lexemes with a main form: the others only spell other words
    let mut main: HashSet<(&str, &str)> = HashSet::new();
    for (number, line) in table.lines().enumerate() {
        let at = |err| at_line(&lexemes, number, err);
        let fields: Vec<&str> = line.split('\t').collect();
        let &[lemma, pos, form, feats, kind, rank] = fields.as_slice() else {
            return Err(at(Error::Build(format!(
                "expected 6 fields, got {}",
                fields.len()
            ))));
        };
        // A lexeme of the extra source whose lemma the treebanks have only as a name is left out
        // whole
        if kind.starts_with("extra")
            && pos != "INTJ"
            && (counts.only_name(form)
                || counts.only_name(lemma) && !main_source.contains(&(lemma, pos)))
        {
            continue;
        }
        builder
            .add_form(lemma, pos, form, plain(feats))
            .map_err(at)?;
        match kind {
            "_" => {
                main.insert((lemma, pos));
            }
            "variant" => builder
                .mark_variant(lemma, pos, form, plain(feats))
                .map_err(at)?,
            "old" => builder
                .mark_old(lemma, pos, form, plain(feats))
                .map_err(at)?,
            "extra" => {
                main.insert((lemma, pos));
                builder
                    .mark_extra(lemma, pos, form, plain(feats))
                    .map_err(at)?;
            }
            "extra-variant" => {
                builder
                    .mark_variant(lemma, pos, form, plain(feats))
                    .map_err(at)?;
                builder
                    .mark_extra(lemma, pos, form, plain(feats))
                    .map_err(at)?;
            }
            "extra-old" => {
                builder
                    .mark_old(lemma, pos, form, plain(feats))
                    .map_err(at)?;
                builder
                    .mark_extra(lemma, pos, form, plain(feats))
                    .map_err(at)?;
            }
            _ => match kind.strip_prefix("spoken:") {
                Some(standard) => builder
                    .mark_spoken(lemma, pos, form, plain(feats), standard)
                    .map_err(at)?,
                None => {
                    return Err(at(Error::Build(format!(
                        "the kind is `variant`, `old`, `spoken:<form>`, `extra`, `extra-variant`, \
                         `extra-old` or `_`, not {kind:?}"
                    ))));
                }
            },
        }
        if rank != "_" {
            let rank = rank.parse().map_err(|_| {
                at(Error::Build(format!(
                    "the place is a number or `_`, not {rank:?}"
                )))
            })?;
            builder
                .set_rank(lemma, pos, form, plain(feats), rank)
                .map_err(at)?;
        }
        let row = Row {
            lemma,
            pos,
            feats,
            spelling: false,
        };
        forms.push((form, row));
    }
    if forms.is_empty() {
        return Err(Error::Build(format!("{}: no rows", lexemes.display())));
    }
    // The rows of a form go together in the order of the table
    forms.sort_by_key(|&(form, _)| form);
    for (_, row) in &mut forms {
        row.spelling = !main.contains(&(row.lemma, row.pos));
    }
    for (pos, feats, count) in counts.tags() {
        builder
            .add_tag_frequency(pos, plain(feats), count)
            .map_err(|err| within(&analyses, err))?;
    }
    for group in forms.chunk_by(|a, b| a.0 == b.0) {
        let form = group[0].0;
        let rows: Vec<Row> = group.iter().map(|&(_, row)| row).collect();
        builder.set_frequency(form, counts.frequency(form));
        for &row in &rows {
            let count = counts.count(form, row);
            if count > 0 {
                builder.set_count(row.lemma, row.pos, form, plain(row.feats), count)?;
            }
        }
        if rows.len() < 2 {
            continue;
        }
        let ranked = counts.rank(form, &rows);
        for (place, row) in ranked.rows.iter().enumerate() {
            let probability = ranked.probabilities.as_ref().map(|values| values[place]);
            builder.place(
                row.lemma,
                row.pos,
                form,
                plain(row.feats),
                place as u32,
                probability,
            )?;
        }
    }
    // The builder keeps its own copies: the table and the counts go before it builds
    for (lemma, count) in counts.verbs() {
        builder
            .set_verb_frequency(lemma, count)
            .map_err(|err| within(&analyses, err))?;
    }
    drop((forms, main, counts));
    drop(table);
    let diminutives = export.join("diminutives.tsv");
    for (number, line) in read(&diminutives)?.lines().enumerate() {
        let at = |err| at_line(&diminutives, number, err);
        let fields: Vec<&str> = line.split('\t').collect();
        let &[lemma, pos, base] = fields.as_slice() else {
            return Err(at(Error::Build(format!(
                "expected 3 fields, got {}",
                fields.len()
            ))));
        };
        builder.add_diminutive(lemma, pos, base).map_err(at)?;
    }
    let bytes = builder.build()?;
    if context.is_some() {
        let gaps = esmorph::Dictionary::from_bytes(&bytes)?.context_gaps()?;
        if !gaps.is_empty() {
            return Err(Error::Build(format!(
                "the context model was trained with a dictionary without the tags {}: build the \
                 dictionary without the context model (`-`, with the paradigm model as the fifth \
                 argument), train the model with it (scripts/train_context.py --dictionary) and \
                 build the dictionary again",
                gaps.join(", ")
            )));
        }
    }
    let failed = |err: std::io::Error| Error::Build(format!("{}: {err}", output.display()));
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(failed)?;
    }
    // The file of an open dictionary must not change in place: a mapping keeps its old file
    let temporary = output.with_extension("tmp");
    if let Err(err) = fs::write(&temporary, &bytes).and_then(|()| fs::rename(&temporary, output)) {
        // Nothing is left of a failed writing
        let _ = fs::remove_file(&temporary);
        return Err(failed(err));
    }
    Ok(bytes.len())
}

/// Features for the builder: `_` is none
fn plain(feats: &str) -> &str {
    if feats == "_" { "" } else { feats }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    fn model(path: &str) -> Option<&Path> {
        (path != "-").then(|| Path::new(path))
    }
    let (export, counts, output, context, paradigms) = match args.as_slice() {
        [export, counts, output] => (export, counts, output, None, None),
        [export, counts, output, context] => (export, counts, output, model(context), None),
        [export, counts, output, context, paradigms] => {
            (export, counts, output, model(context), model(paradigms))
        }
        _ => {
            eprintln!(
                "usage: esmorph-build <export directory> <counts directory> <dictionary file> \
                 [<context model> [<paradigm model>]], - for no context model"
            );
            return ExitCode::FAILURE;
        }
    };
    match build(
        Path::new(export),
        Path::new(counts),
        Path::new(output),
        context,
        paradigms,
    ) {
        Ok(size) => {
            println!("{output}: {size} bytes");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn description_names_the_sources_of_the_counts() {
        assert_eq!(
            joined("{\"source\": \"a\"}\n", "{\"release\": \"b\"}\n").unwrap(),
            "{\"source\": \"a\", \"counts\": {\"release\": \"b\"}}"
        );
        assert_eq!(joined("{}", "{}").unwrap(), "{\"counts\": {}}");
        assert!(joined("no", "{}").is_err());
        assert!(joined("this is not json}", "{}").is_err());
        assert!(joined("{}", "[]").is_err());
    }

    #[test]
    fn descriptions_are_objects_of_json() {
        for text in [
            "{}",
            " {\"a\": [1, -2.5e+3, 0, true, false, null, {\"b\": \"\\u00e9\\n\"}]}\n",
        ] {
            assert!(json_object(text), "{text}");
        }
        for text in [
            "",
            "[]",
            "\"a\"",
            "{",
            "{}}",
            "{\"a\"}",
            "{\"a\": 01}",
            "{\"a\": 1.}",
            "{\"a\": .5}",
            "{\"a\": tru}",
            "{\"a\": [1,]}",
            "{a: 1}",
            "{\"a\": \"\\x\"}",
            "{\"a\": \"\n\"}",
            "{\"a\": 1} {}",
            "\x0c{}",
            "{}\u{a0}",
            "\u{feff}{}",
        ] {
            assert!(!json_object(text), "{text}");
        }
        let deep = "[".repeat(100) + &"]".repeat(100);
        assert!(!json_object(&format!("{{\"a\": {deep}}}")));
    }

    #[test]
    fn errors_name_files_and_lines() {
        let dir = std::env::temp_dir().join(format!("esmorph-build-{}", std::process::id()));
        let (export, counts) = (dir.join("export"), dir.join("counts"));
        fs::create_dir_all(&export).unwrap();
        fs::create_dir_all(&counts).unwrap();
        fs::write(export.join("meta.json"), "{}").unwrap();
        fs::write(export.join("diminutives.tsv"), "").unwrap();
        for name in ["analyses.tsv", "pos.tsv"] {
            fs::write(counts.join(name), "").unwrap();
        }
        fs::write(counts.join("sources.json"), "{}").unwrap();
        let output = dir.join("es.dict");
        let error = |lexemes: &str, output: &Path| {
            fs::write(export.join("lexemes.tsv"), lexemes).unwrap();
            build(&export, &counts, output, None, None)
                .unwrap_err()
                .to_string()
        };
        let line = "casa\tNOUN\tcasa\t_\t_\t_\n";
        let message = error(&format!("{line}Casa\tNOUN\tCasa\t_\t_\t_\n"), &output);
        assert!(message.contains("lexemes.tsv:2:"), "{message}");
        assert!(error("", &output).contains("no rows"));
        fs::write(export.join("diminutives.tsv"), "casa\tNOUN\n").unwrap();
        assert!(error(line, &output).contains("diminutives.tsv:1: expected 3 fields"));
        fs::write(export.join("diminutives.tsv"), "").unwrap();
        // A directory in place of the dictionary file: the error names it and leaves no file
        let taken = dir.join("taken.dict");
        fs::create_dir_all(&taken).unwrap();
        assert!(error(line, &taken).contains("taken.dict"));
        assert!(!dir.join("taken.tmp").exists());
        fs::write(export.join("lexemes.tsv"), line).unwrap();
        // An error of the counts names its file once, a wrong tag too
        for (analyses, wanted) in [
            ("a\tNOUN\ta\t_\tmany\n", "analyses.tsv:1: wrong count"),
            ("a\tNOUN\ta\tGender\t1\n", "analyses.tsv: wrong UD features"),
        ] {
            fs::write(counts.join("analyses.tsv"), analyses).unwrap();
            let message = build(&export, &counts, &output, None, None)
                .unwrap_err()
                .to_string();
            assert!(message.contains(wanted), "{message}");
            assert_eq!(message.matches("build error").count(), 1, "{message}");
        }
        fs::write(counts.join("analyses.tsv"), "").unwrap();
        assert!(build(&export, &counts, &output, None, None).is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extra_forms_of_names_are_left_out() {
        let dir = std::env::temp_dir().join(format!("esmorph-build-names-{}", std::process::id()));
        let (export, counts) = (dir.join("export"), dir.join("counts"));
        fs::create_dir_all(&export).unwrap();
        fs::create_dir_all(&counts).unwrap();
        fs::write(export.join("meta.json"), "{}").unwrap();
        fs::write(export.join("diminutives.tsv"), "").unwrap();
        fs::write(counts.join("analyses.tsv"), "").unwrap();
        fs::write(
            counts.join("pos.tsv"),
            "ciu\tPROPN\t3\nchist\tPROPN\t1\nnobel\tPROPN\t2\n",
        )
        .unwrap();
        fs::write(counts.join("sources.json"), "{}").unwrap();
        let lexemes = "casa\tNOUN\tcasa\t_\t_\t_\nciu\tNOUN\tciu\t_\textra\t_\n\
                       ciu\tNOUN\tcius\tNumber=Plur\textra\t_\n\
                       chist\tINTJ\tchist\t_\textra\t_\n\
                       nobel\tNOUN\tnobel\t_\t_\t_\nnobel\tNOUN\tnobeles\tNumber=Plur\textra\t_\n";
        fs::write(export.join("lexemes.tsv"), lexemes).unwrap();
        let output = dir.join("es.dict");
        build(&export, &counts, &output, None, None).unwrap();
        let dictionary = esmorph::Dictionary::open(&output).unwrap();
        // The treebanks have `ciu` only as the name `CiU`, so its lexeme is left out; an
        // interjection stays
        assert!(dictionary.analyze("ciu").iter().all(|a| a.lemma != "ciu"));
        assert!(dictionary.analyze("cius").iter().all(|a| a.lemma != "ciu"));
        // A lexeme of the main source keeps its forms of the extra source
        assert!(
            dictionary
                .analyze("nobeles")
                .iter()
                .any(|a| a.lemma == "nobel")
        );
        assert!(
            dictionary
                .analyze("chist")
                .iter()
                .any(|a| a.lemma == "chist")
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_context_model() {
        // The weights of the repository: the description names their data, a dictionary with a tag
        // they have not seen is refused, and damaged weights name their file
        let weights = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/es-context.bin");
        let dir = std::env::temp_dir().join(format!("esmorph-build-model-{}", std::process::id()));
        let (export, counts) = (dir.join("export"), dir.join("counts"));
        fs::create_dir_all(&export).unwrap();
        fs::create_dir_all(&counts).unwrap();
        fs::write(export.join("meta.json"), "{}").unwrap();
        fs::write(export.join("diminutives.tsv"), "").unwrap();
        for name in ["analyses.tsv", "pos.tsv"] {
            fs::write(counts.join(name), "").unwrap();
        }
        fs::write(counts.join("sources.json"), "{}").unwrap();
        let lexemes = export.join("lexemes.tsv");
        fs::write(&lexemes, "casa\tNOUN\tcasa\t_\t_\t_\n").unwrap();
        let output = dir.join("es.dict");
        build(&export, &counts, &output, Some(&weights), None).unwrap();
        let dictionary = esmorph::Dictionary::open(&output).unwrap();
        assert!(dictionary.has_context());
        assert!(json_object(dictionary.meta()), "{}", dictionary.meta());
        assert!(dictionary.meta().contains(", \"context\": {\""));
        assert_eq!(
            dictionary
                .analyze_sentence(&["casa", "casa"])
                .unwrap()
                .len(),
            2
        );
        let other = dir.join("other.dict");
        fs::write(&lexemes, "casa\tNOUN\tcasa\tTypo=Yes\t_\t_\n").unwrap();
        let message = build(&export, &counts, &other, Some(&weights), None)
            .unwrap_err()
            .to_string();
        assert!(message.contains("without the tags F=Typo=Yes"), "{message}");
        assert!(!other.exists());
        let truncated = dir.join("truncated.bin");
        fs::write(&truncated, &fs::read(&weights).unwrap()[..1000]).unwrap();
        let message = build(&export, &counts, &other, Some(&truncated), None)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("truncated.bin: context model"),
            "{message}"
        );
        // The paradigm model with no context model: the description names its data too
        let paradigms = weights.with_file_name("es-paradigms.bin");
        fs::write(&lexemes, "casa\tNOUN\tcasa\t_\t_\t_\n").unwrap();
        build(&export, &counts, &output, None, Some(&paradigms)).unwrap();
        let dictionary = esmorph::Dictionary::open(&output).unwrap();
        assert!(dictionary.has_paradigm_model() && !dictionary.has_context());
        assert!(json_object(dictionary.meta()), "{}", dictionary.meta());
        assert!(dictionary.meta().contains(", \"paradigms\": {\""));
        fs::write(&truncated, &fs::read(&paradigms).unwrap()[..1000]).unwrap();
        let message = build(&export, &counts, &other, None, Some(&truncated))
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("truncated.bin: paradigm model"),
            "{message}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_file_names_its_path() {
        let error = read(Path::new("no/such/file.tsv")).unwrap_err();
        assert!(error.to_string().contains("no/such/file.tsv"), "{error}");
    }
}
