//! Builder of the dictionary of esMorph: `esmorph-build <export directory> <counts directory>
//! <dictionary file>`. The export is `lexemes.tsv` (lemma, part of speech, form, features) and
//! `meta.json`, written by `scripts/export_wiktionary.py`; the counts are `analyses.tsv` and `pos.tsv`,
//! written by `scripts/export_ud.py`

mod frequencies;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use esmorph::{Builder, Error};
use frequencies::{Counts, Row};

fn build(export: &Path, counts: &Path, output: &Path) -> Result<usize, Error> {
    let mut builder = Builder::new();
    builder.meta(fs::read_to_string(export.join("meta.json"))?);
    let table = fs::read_to_string(export.join("lexemes.tsv"))?;
    let mut forms: BTreeMap<&str, Vec<Row>> = BTreeMap::new();
    for (number, line) in table.lines().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        let &[lemma, pos, form, feats] = fields.as_slice() else {
            return Err(Error::Build(format!(
                "line {}: expected 4 fields, got {}",
                number + 1,
                fields.len()
            )));
        };
        builder.add_form(lemma, pos, form, plain(feats))?;
        forms
            .entry(form)
            .or_default()
            .push(Row { lemma, pos, feats });
    }
    let counts = Counts::read(
        &fs::read_to_string(counts.join("analyses.tsv"))?,
        &fs::read_to_string(counts.join("pos.tsv"))?,
    )?;
    for (form, rows) in forms.iter().filter(|(_, rows)| rows.len() > 1) {
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
