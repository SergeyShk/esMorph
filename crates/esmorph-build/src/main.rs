//! Builder of the dictionary of esMorph: `esmorph-build <export directory> <counts directory>
//! <dictionary file>`. The export is `lexemes.tsv` (lemma, part of speech, form, features, `variant`
//! or `_`) and `meta.json`, written by `scripts/export_wiktionary.py`; the counts are `analyses.tsv`,
//! `pos.tsv` and `sources.json`, written by `scripts/export_ud.py`

mod frequencies;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use esmorph::{Builder, Error};
use frequencies::{Counts, Row};

fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|err| Error::Build(format!("{}: {err}", path.display())))
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
    let table = read(&export.join("lexemes.tsv"))?;
    let mut forms: BTreeMap<&str, Vec<Row>> = BTreeMap::new();
    for (number, line) in table.lines().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        let &[lemma, pos, form, feats, kind] = fields.as_slice() else {
            return Err(Error::Build(format!(
                "line {}: expected 5 fields, got {}",
                number + 1,
                fields.len()
            )));
        };
        builder.add_form(lemma, pos, form, plain(feats))?;
        match kind {
            "_" => {}
            "variant" => builder.mark_variant(lemma, pos, form, plain(feats))?,
            _ => {
                return Err(Error::Build(format!(
                    "line {}: the last field is `variant` or `_`, not {kind:?}",
                    number + 1
                )));
            }
        }
        forms
            .entry(form)
            .or_default()
            .push(Row { lemma, pos, feats });
    }
    let counts = Counts::read(
        &read(&counts.join("analyses.tsv"))?,
        &read(&counts.join("pos.tsv"))?,
    )?;
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
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    // The file of an open dictionary must not change in place: a mapping keeps its old file
    let temporary = output.with_extension("tmp");
    fs::write(&temporary, &bytes)?;
    fs::rename(&temporary, output)?;
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
    fn a_missing_file_names_its_path() {
        let error = read(Path::new("no/such/file.tsv")).unwrap_err();
        assert!(error.to_string().contains("no/such/file.tsv"), "{error}");
    }
}
