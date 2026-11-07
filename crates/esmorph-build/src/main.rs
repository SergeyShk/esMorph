//! Builder of the dictionary of esMorph: `esmorph-build <export directory> <dictionary file>`.
//! The export is `lexemes.tsv` (lemma, part of speech, form, features) and `meta.json`, written by
//! `scripts/export_wiktionary.py`

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::ExitCode;

use esmorph::{Builder, Error};

fn build(export: &Path, output: &Path) -> Result<usize, Error> {
    let mut builder = Builder::new();
    builder.meta(fs::read_to_string(export.join("meta.json"))?);
    let lines = BufReader::new(fs::File::open(export.join("lexemes.tsv"))?);
    for (number, line) in lines.lines().enumerate() {
        let line = line?;
        let fields: Vec<&str> = line.split('\t').collect();
        let &[lemma, pos, form, feats] = fields.as_slice() else {
            return Err(Error::Build(format!(
                "line {}: expected 4 fields, got {}",
                number + 1,
                fields.len()
            )));
        };
        let feats = if feats == "_" { "" } else { feats };
        builder.add_form(lemma, pos, form, feats)?;
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [export, output] = args.as_slice() else {
        eprintln!("usage: esmorph-build <export directory> <dictionary file>");
        return ExitCode::FAILURE;
    };
    match build(Path::new(export), Path::new(output)) {
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
