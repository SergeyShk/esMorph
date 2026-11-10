//! Builder of the dictionary of esMorph: `esmorph-build <export directory> <counts directory>
//! <dictionary file>`. The export is `lexemes.tsv` (lemma, part of speech, form, features, `variant`
//! or `_`, the place in the table of the source or `_`) and `meta.json`, written by
//! `scripts/export_wiktionary.py`; the counts are `analyses.tsv`, `pos.tsv` and `sources.json`,
//! written by `scripts/export_ud.py`

mod frequencies;

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use esmorph::{Builder, Error};
use frequencies::{Counts, Row};

fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|err| Error::Build(format!("{}: {err}", path.display())))
}

/// An error of a line of a file with its place: `lexemes.tsv:2: word "Casa" is not in lower case`
fn at_line(path: &Path, number: usize, err: Error) -> Error {
    let message = match err {
        Error::Build(message) | Error::Format(message) => message,
        err => err.to_string(),
    };
    Error::Build(format!("{}:{}: {message}", path.display(), number + 1))
}

/// Description of the dictionary: the description of its export with the sources of the counts
fn joined(meta: &str, sources: &str) -> Result<String, Error> {
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

fn build(export: &Path, counts: &Path, output: &Path) -> Result<usize, Error> {
    let mut builder = Builder::new();
    builder.meta(joined(
        &read(&export.join("meta.json"))?,
        &read(&counts.join("sources.json"))?,
    )?);
    let lexemes = export.join("lexemes.tsv");
    let table = read(&lexemes)?;
    let mut forms: BTreeMap<&str, Vec<Row>> = BTreeMap::new();
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
            _ => {
                return Err(at(Error::Build(format!(
                    "the kind is `variant` or `_`, not {kind:?}"
                ))));
            }
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
        forms.entry(form).or_default().push(Row {
            lemma,
            pos,
            feats,
            spelling: false,
        });
    }
    if forms.is_empty() {
        return Err(Error::Build(format!("{}: no rows", lexemes.display())));
    }
    for row in forms.values_mut().flatten() {
        row.spelling = !main.contains(&(row.lemma, row.pos));
    }
    let counts = Counts::read(
        &read(&counts.join("analyses.tsv"))?,
        &read(&counts.join("pos.tsv"))?,
    )
    .map_err(|err| Error::Build(format!("{}: {err}", counts.display())))?;
    for (pos, feats, count) in counts.tags() {
        builder.set_tag_frequency(pos, plain(feats), count)?;
    }
    for (form, rows) in &forms {
        builder.set_frequency(form, counts.frequency(form));
        for &row in rows {
            let count = counts.count(form, row);
            if count > 0 {
                builder.set_count(row.lemma, row.pos, form, plain(row.feats), count)?;
            }
        }
        if rows.len() < 2 {
            continue;
        }
        let ranked = counts.rank(form, rows);
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
    let bytes = builder.build()?;
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
    let [export, counts, output] = args.as_slice() else {
        eprintln!("usage: esmorph-build <export directory> <counts directory> <dictionary file>");
        return ExitCode::FAILURE;
    };
    match build(Path::new(export), Path::new(counts), Path::new(output)) {
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
    }

    #[test]
    fn errors_name_files_and_lines() {
        let dir = std::env::temp_dir().join(format!("esmorph-build-{}", std::process::id()));
        let (export, counts) = (dir.join("export"), dir.join("counts"));
        fs::create_dir_all(&export).unwrap();
        fs::create_dir_all(&counts).unwrap();
        fs::write(export.join("meta.json"), "{}").unwrap();
        for name in ["analyses.tsv", "pos.tsv"] {
            fs::write(counts.join(name), "").unwrap();
        }
        fs::write(counts.join("sources.json"), "{}").unwrap();
        let output = dir.join("es.dict");
        let error = |lexemes: &str, output: &Path| {
            fs::write(export.join("lexemes.tsv"), lexemes).unwrap();
            build(&export, &counts, output).unwrap_err().to_string()
        };
        let line = "casa\tNOUN\tcasa\t_\t_\t_\n";
        let message = error(&format!("{line}Casa\tNOUN\tCasa\t_\t_\t_\n"), &output);
        assert!(message.contains("lexemes.tsv:2:"), "{message}");
        assert!(error("", &output).contains("no rows"));
        // A directory in place of the dictionary file: the error names it and leaves no file
        let taken = dir.join("taken.dict");
        fs::create_dir_all(&taken).unwrap();
        assert!(error(line, &taken).contains("taken.dict"));
        assert!(!dir.join("taken.tmp").exists());
        fs::write(export.join("lexemes.tsv"), line).unwrap();
        assert!(build(&export, &counts, &output).is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_file_names_its_path() {
        let error = read(Path::new("no/such/file.tsv")).unwrap_err();
        assert!(error.to_string().contains("no/such/file.tsv"), "{error}");
    }
}
