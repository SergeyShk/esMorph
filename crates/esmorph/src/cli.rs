//! The `esmorph` command: morphological analysis of text from files and the standard input

use std::borrow::Cow;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use anyseg::{ConlluWriter, Document, Segmenter, Span, read_conllu};

use crate::dictionary::{one_line, set_fields};
use crate::{Analysis, Dictionary};

const HELP: &str = "\
esmorph - morphological analysis of Spanish text: lemmas, parts of speech and Universal
Dependencies features

Usage: esmorph [options] [file ...]

Files are read one after another; without files and in place of \"-\" the standard input is
read, in UTF-8. The result goes to the standard output or to the file of -o. Every word gets
its most probable analysis; the words of a multiword token are analyzed by the token.

Options:
  -f, --format FORMAT      output format:
                             lemmas - the lemmas of the words of a sentence on a line, a blank
                             line between paragraphs (default)
                             conllu - CoNLL-U: the lemma, the part of speech and the features
                             of the most probable analysis of every word
                             tsv - a word, its lemma, part of speech, features and the
                             probability of the analysis, separated by tabs; a blank line after
                             every sentence
                             jsonl - JSON Lines: a sentence per line, all analyses of every
                             token
  -i, --input INPUT        input:
                             text - text: a blank line ends a paragraph, the anySeg segmenter
                             finds the tokens, their words and the sentences (default)
                             tokens - a token per line, a blank line between sentences
                             conllu - CoNLL-U: LEMMA, UPOS and FEATS are filled
  -d, --dictionary FILE    esMorph dictionary file (default: the dictionary of the package)
  -o, --output FILE        write the result to a file
  -h, --help               show this help and exit
  -V, --version            show the version and exit
";

/// Runs the command with `args` (without the name of the program) and returns the exit code: 0 on
/// success, 1 on an error of reading the dictionary or a file or of writing, 2 on wrong arguments.
/// `version` is printed by `--version`; `dictionary` is the default dictionary, without it the
/// dictionary of the crate `esmorph-dict-es`
pub fn run(args: Vec<OsString>, version: &str, dictionary: Option<&Path>) -> u8 {
    let stdout = io::stdout();
    let interactive = stdout.is_terminal();
    let closed = closed_stdout();
    let mut output = BufWriter::new(stdout.lock());
    execute(
        args,
        version,
        dictionary,
        interactive,
        closed,
        &mut io::stdin().lock(),
        &mut output,
        &mut io::stderr(),
    )
}

/// The standard output is closed (`>&-`): writing to it would fail without an error. A Rust
/// program finds it open on /dev/null, as the runtime opens the closed standard descriptors there;
/// the module of Python finds it closed
#[cfg(unix)]
fn closed_stdout() -> bool {
    use std::os::fd::AsFd;
    // EBADF, the same number on Linux, macOS and the BSDs: a closed descriptor
    io::stdout()
        .as_fd()
        .try_clone_to_owned()
        .is_err_and(|err| err.raw_os_error() == Some(9))
}

#[cfg(not(unix))]
fn closed_stdout() -> bool {
    false
}

/// The buffer of the input: the output is flushed when it is empty and the next read may wait for
/// input; tokens and CoNLL-U are analyzed in pieces of about this size
#[cfg(not(test))]
const INPUT_BUFFER: usize = 1 << 16;
#[cfg(test)]
const INPUT_BUFFER: usize = 1 << 9;

/// The longest text without blank lines that is analyzed at once: a longer one is written without
/// its last sentence, and a longer line is cut at a space, so that the memory does not grow with
/// the input
#[cfg(not(test))]
const PARAGRAPH: usize = 1 << 18;
#[cfg(test)]
const PARAGRAPH: usize = 1 << 12;

/// Output format
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Lemmas,
    Conllu,
    Tsv,
    Jsonl,
}

/// Input kind
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Input {
    Text,
    Tokens,
    Conllu,
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Version,
    Run {
        format: Format,
        input: Input,
        dictionary: Option<PathBuf>,
        output: Option<PathBuf>,
        files: Vec<OsString>,
    },
}

fn parse(args: Vec<OsString>) -> Result<Command, String> {
    let (mut format, mut input, mut dictionary, mut output) =
        (Format::Lemmas, Input::Text, None, None);
    let mut files = Vec::new();
    let mut options = true;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let bytes = arg.as_encoded_bytes();
        if !options || !bytes.starts_with(b"-") || bytes == b"-" {
            files.push(arg);
            continue;
        }
        // An option with an attached value not in UTF-8 would be taken for a file
        let Some(option) = arg.to_str().map(str::to_owned) else {
            return Err(format!(
                "cannot read the option {}: give its value as a separate argument",
                arg.to_string_lossy()
            ));
        };
        let (name, inline) = match option.split_once('=') {
            Some((name, value)) if option.starts_with("--") => (name, Some(value)),
            _ if !option.starts_with("--") && option.len() > 2 => option
                .split_at_checked(2)
                .map_or((option.as_str(), None), |(name, value)| (name, Some(value))),
            _ => (option.as_str(), None),
        };
        let mut value = || -> Result<OsString, String> {
            match inline {
                Some(value) => Ok(value.into()),
                None => args
                    .next()
                    .ok_or_else(|| format!("option {name} needs a value")),
            }
        };
        match name {
            "--" => options = false,
            "-h" | "--help" if inline.is_none() => return Ok(Command::Help),
            "-V" | "--version" if inline.is_none() => return Ok(Command::Version),
            "-f" | "--format" => {
                format = match value()?.to_str() {
                    Some("lemmas") => Format::Lemmas,
                    Some("conllu") => Format::Conllu,
                    Some("tsv") => Format::Tsv,
                    Some("jsonl") => Format::Jsonl,
                    other => return Err(unknown("format", other, "lemmas, conllu, tsv, jsonl")),
                }
            }
            "-i" | "--input" => {
                input = match value()?.to_str() {
                    Some("text") => Input::Text,
                    Some("tokens") => Input::Tokens,
                    Some("conllu") => Input::Conllu,
                    other => return Err(unknown("input", other, "text, tokens, conllu")),
                }
            }
            "-d" | "--dictionary" => dictionary = Some(PathBuf::from(value()?)),
            "-o" | "--output" => {
                output = Some(value()?)
                    .filter(|value| value != "-")
                    .map(PathBuf::from);
            }
            _ => return Err(format!("unknown option {option}")),
        }
    }
    Ok(Command::Run {
        format,
        input,
        dictionary,
        output,
        files,
    })
}

fn unknown(what: &str, value: Option<&str>, known: &str) -> String {
    format!(
        "unknown {what} {}, expected one of: {known}",
        value.unwrap_or("?")
    )
}

#[allow(clippy::too_many_arguments)]
fn execute(
    args: Vec<OsString>,
    version: &str,
    default: Option<&Path>,
    interactive: bool,
    closed: bool,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    // A message to stderr: if it is closed too, there is no one to tell
    let mut report = |message: String| drop(writeln!(stderr, "esmorph: {message}"));
    let (format, input, dictionary, output_path, files) = match parse(args) {
        Ok(Command::Run {
            format,
            input,
            dictionary,
            output,
            files,
        }) => (format, input, dictionary, output, files),
        Ok(Command::Help | Command::Version) if closed => {
            report("the standard output is closed".to_owned());
            return 1;
        }
        Ok(Command::Help) => {
            let written = stdout
                .write_all(HELP.as_bytes())
                .and_then(|()| stdout.flush());
            return written.map_or_else(|err| write_error(&err, &mut report), |()| 0);
        }
        Ok(Command::Version) => {
            let written = writeln!(stdout, "esmorph {version}").and_then(|()| stdout.flush());
            return written.map_or_else(|err| write_error(&err, &mut report), |()| 0);
        }
        Err(message) => {
            report(format!("{message}\nHelp: esmorph --help"));
            return 2;
        }
    };
    let dictionary_path = dictionary.as_deref().or(default);
    // A dictionary given by the user may come from anywhere: its automata are checked
    let dictionary = match load(dictionary_path, dictionary.is_some()) {
        Ok(dictionary) => dictionary,
        Err(message) => {
            report(message);
            return 1;
        }
    };
    let files = if files.is_empty() {
        vec![OsString::from("-")]
    } else {
        files
    };
    if closed && output_path.is_none() {
        report("the standard output is closed".to_owned());
        return 1;
    }
    // The output file is created anew and the standard output may be appended to: an input or the
    // dictionary it would overwrite or grow is not read. A device (/dev/null, /dev/stdout in a
    // terminal) is neither
    let sources: Vec<Stream> = (files.iter())
        .map(|file| match file == "-" {
            true => Stream::Stdin,
            false => Stream::Path(Path::new(file)),
        })
        .chain(dictionary_path.map(Stream::Path))
        .collect();
    let target = match &output_path {
        Some(path) => Stream::Path(path),
        None => Stream::Stdout,
    };
    if metadata(target).is_some_and(|metadata| metadata.is_file())
        && sources.iter().any(|&source| same_file(target, source))
    {
        report(match &output_path {
            Some(path) => format!(
                "{}: the output file is an input file or the dictionary",
                path.display()
            ),
            None => "the standard output is an input file or the dictionary".into(),
        });
        return 1;
    }
    let mut file_output;
    let output: &mut dyn Write = match &output_path {
        Some(path) => match File::create(path) {
            Ok(file) => {
                file_output = BufWriter::new(file);
                &mut file_output
            }
            Err(err) => {
                report(format!("{}: {}", path.display(), open_error(path, &err)));
                return 1;
            }
        },
        None => stdout,
    };
    let mut writer = Writer {
        dictionary: &dictionary,
        segmenter: Segmenter::new("es").expect("anyseg is built with the feature es"),
        format,
        input,
        output,
        interactive: interactive && output_path.is_none(),
        pending: false,
        conllu: ConlluWriter::new(),
        paragraphs: 0,
        text: String::new(),
        ready: 0,
        continued: false,
        carry: Vec::new(),
        tokens: Vec::new(),
        sentence_starts: Vec::new(),
        lines: String::new(),
        first_line: 0,
        next_line: 0,
        out: String::new(),
        wrong: Vec::new(),
    };
    let mut code = 0;
    for file in files {
        let path = Path::new(&file);
        let name = path.display().to_string();
        let result = if file == "-" {
            writer.process(stdin)
        } else {
            match File::open(path) {
                Ok(mut opened) => writer.process(&mut opened),
                Err(err) => Err(Failure::Open(open_error(path, &err))),
            }
        };
        // A wrong sentence of CoNLL-U goes out as it is, as it cannot be analyzed
        let kept = match format {
            Format::Conllu => "written as it is",
            _ => "skipped",
        };
        for (line, message) in writer.wrong.drain(..) {
            report(format!(
                "{name}: line {line}: {message}; the sentence is {kept}"
            ));
            code = 1;
        }
        match result {
            Ok(None) => {}
            Ok(Some(line)) => {
                report(format!(
                    "{name}: line {line}: not UTF-8, the wrong bytes are replaced with U+FFFD"
                ));
                code = 1;
            }
            Err(Failure::Open(message)) => {
                report(format!("{name}: {message}"));
                code = 1;
            }
            Err(Failure::Read(err)) => {
                report(format!("{name}: {}", describe(&err)));
                code = 1;
            }
            Err(Failure::Encoding(line)) => {
                report(format!("{name}: the file is not in UTF-8 (line {line})"));
                code = 1;
            }
            Err(Failure::Write(err)) => return write_error(&err, &mut report).max(code),
        }
    }
    match writer.output.flush() {
        Ok(()) => code,
        Err(err) => write_error(&err, &mut report).max(code),
    }
}

/// The exit code after an error of writing: a closed pipe (`esmorph ... | head`) is not an error
fn write_error(err: &io::Error, report: &mut impl FnMut(String)) -> u8 {
    if err.kind() == io::ErrorKind::BrokenPipe {
        return 0;
    }
    report(format!("write error: {err}"));
    1
}

/// The dictionary of the file `path`, verified with `verify`; without a path, the dictionary of the
/// crate `esmorph-dict-es`
fn load(path: Option<&Path>, verify: bool) -> Result<Dictionary, String> {
    match path {
        Some(path) => Dictionary::open(path)
            .and_then(|dictionary| {
                if verify {
                    // The panic of a damaged automaton is an error of the dictionary: its message
                    // is not printed
                    let hook = std::panic::take_hook();
                    std::panic::set_hook(Box::new(|_| {}));
                    let verified = dictionary.verify();
                    std::panic::set_hook(hook);
                    verified?;
                }
                Ok(dictionary)
            })
            .map_err(|err| match err {
                crate::Error::Io(err) => format!("{}: {}", path.display(), open_error(path, &err)),
                err => format!("{}: {err}", path.display()),
            }),
        #[cfg(feature = "bundled")]
        None => Dictionary::bundled().map_err(|err| err.to_string()),
        #[cfg(not(feature = "bundled"))]
        None => Err("no dictionary: give the dictionary file in --dictionary".into()),
    }
}

/// An error of input and output in plain words for the frequent cases
fn describe(err: &io::Error) -> String {
    match err.kind() {
        io::ErrorKind::NotFound => "no such file".into(),
        io::ErrorKind::PermissionDenied => "permission denied".into(),
        io::ErrorKind::IsADirectory => "is a directory".into(),
        _ => err.to_string(),
    }
}

/// The error of opening a file: on Windows a directory fails to open with «permission denied»
fn open_error(path: &Path, err: &io::Error) -> String {
    if path.is_dir() {
        "is a directory".into()
    } else {
        describe(err)
    }
}

/// A file that the command reads or writes
#[derive(Clone, Copy)]
enum Stream<'a> {
    Path(&'a Path),
    Stdin,
    Stdout,
}

/// The metadata of the file of a stream, `None` if it cannot be read
fn metadata(stream: Stream) -> Option<std::fs::Metadata> {
    match stream {
        Stream::Path(path) => opened_metadata(path).ok(),
        Stream::Stdin => descriptor_metadata(&io::stdin()),
        Stream::Stdout => descriptor_metadata(&io::stdout()),
    }
}

/// The metadata of the file that a standard stream refers to: by the descriptor, as /dev/stdin on
/// macOS is a file of devfs, not the redirected file
#[cfg(unix)]
fn descriptor_metadata(stream: &impl std::os::fd::AsFd) -> Option<std::fs::Metadata> {
    let descriptor = stream.as_fd().try_clone_to_owned().ok()?;
    File::from(descriptor).metadata().ok()
}

#[cfg(windows)]
fn descriptor_metadata(stream: &impl std::os::windows::io::AsHandle) -> Option<std::fs::Metadata> {
    let handle = stream.as_handle().try_clone_to_owned().ok()?;
    File::from(handle).metadata().ok()
}

#[cfg(not(any(unix, windows)))]
fn descriptor_metadata<T>(_stream: &T) -> Option<std::fs::Metadata> {
    None
}

/// The metadata of the file a path opens: on macOS /dev/stdin and /dev/fd/N are files of devfs with
/// a device of their own, which stand for the files open in the process. A pipe or a socket is not
/// opened, which could wait
#[cfg(unix)]
fn opened_metadata(path: &Path) -> io::Result<std::fs::Metadata> {
    use std::os::unix::fs::FileTypeExt;
    let metadata = std::fs::metadata(path)?;
    let kind = metadata.file_type();
    if kind.is_fifo() || kind.is_socket() {
        return Ok(metadata);
    }
    File::open(path)
        .and_then(|file| file.metadata())
        .or(Ok(metadata))
}

#[cfg(not(unix))]
fn opened_metadata(path: &Path) -> io::Result<std::fs::Metadata> {
    std::fs::metadata(path)
}

/// Two streams are one file. On Unix the device and the number of the file decide, so that a hard
/// link and a symbolic link are the same file
#[cfg(unix)]
fn same_file(a: Stream, b: Stream) -> bool {
    use std::os::unix::fs::MetadataExt;
    let id = |stream| metadata(stream).map(|metadata| (metadata.dev(), metadata.ino()));
    id(a).is_some_and(|a| Some(a) == id(b))
}

/// Two streams are one file. On Windows a file is its canonical path or, for a hard link and a
/// standard stream, which have no other, its times and size: the index of a file needs an unstable
/// API
#[cfg(windows)]
fn same_file(a: Stream, b: Stream) -> bool {
    use std::os::windows::fs::MetadataExt;
    if let (Stream::Path(a), Stream::Path(b)) = (a, b)
        && let (Ok(a), Ok(b)) = (std::fs::canonicalize(a), std::fs::canonicalize(b))
        && a == b
    {
        return true;
    }
    let stamp = |stream| {
        metadata(stream).map(|metadata| {
            (
                metadata.creation_time(),
                metadata.last_write_time(),
                metadata.file_size(),
            )
        })
    };
    stamp(a).is_some_and(|a| Some(a) == stamp(b))
}

/// Two streams are one file elsewhere: by the canonical path; a standard stream is not known
#[cfg(not(any(unix, windows)))]
fn same_file(a: Stream, b: Stream) -> bool {
    match (a, b) {
        (Stream::Path(a), Stream::Path(b)) => {
            match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
                (Ok(a), Ok(b)) => a == b,
                _ => false,
            }
        }
        _ => false,
    }
}

enum Failure {
    /// The file does not open: the message
    Open(String),
    Read(io::Error),
    /// The file is in another encoding: UTF-16, or wrong bytes before the first line of UTF-8; the
    /// line where it shows
    Encoding(usize),
    Write(io::Error),
}

impl From<io::Error> for Failure {
    fn from(err: io::Error) -> Self {
        Failure::Write(err)
    }
}

struct Writer<'a> {
    dictionary: &'a Dictionary,
    segmenter: Segmenter,
    format: Format,
    input: Input,
    output: &'a mut dyn Write,
    /// Flush after every piece: in a terminal the result shows at once
    interactive: bool,
    /// There is output after the last flush
    pending: bool,
    conllu: ConlluWriter,
    /// The number of paragraphs written: a blank line goes before the next one in the format
    /// `lemmas`
    paragraphs: usize,
    /// The text read and not yet written, and its length up to the last blank line: the paragraphs
    /// before it are complete
    text: String,
    ready: usize,
    /// The text goes on with a paragraph cut by its length or with the tokens of the file written
    /// before: its first paragraph is not a new one
    continued: bool,
    /// The end of a line longer than PARAGRAPH, which the next read gives
    carry: Vec<u8>,
    /// The tokens of the complete sentences not yet written and the first token of every sentence
    tokens: Vec<String>,
    sentence_starts: Vec<usize>,
    /// The lines of CoNLL-U up to the last blank line not yet written, and the number of the first
    lines: String,
    first_line: usize,
    /// The number of the line after the last blank one of CoNLL-U
    next_line: usize,
    /// The output of a sentence, kept between the pieces: a block of its own for each would not be
    /// given back
    out: String,
    /// The wrong sentences of CoNLL-U, which are written as they are: the number of the wrong line
    /// and what is wrong
    wrong: Vec<(usize, String)>,
}

impl Writer<'_> {
    /// Reads the input by lines and writes the analysis of every complete piece: the paragraphs of
    /// a text as soon as a blank line or the end of the input closes them, tokens and CoNLL-U by
    /// sentences. Returns the number of the first line that is not UTF-8, whose wrong bytes become
    /// U+FFFD
    fn process(&mut self, reader: &mut dyn Read) -> Result<Option<usize>, Failure> {
        let mut reader = BufReader::with_capacity(INPUT_BUFFER, reader);
        self.flush_pending()?;
        // The encoding shows at the start of the input: ASCII at the start of a file in another
        // encoding is not written
        let head = loop {
            match reader.fill_buf() {
                Ok(head) => break head,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Err(Failure::Read(err)),
            }
        };
        if let Some(line) = foreign_encoding(head) {
            return Err(Failure::Encoding(line));
        }
        // The end of the input in a terminal (Ctrl+D) is not read twice: a second read would wait
        if head.is_empty() {
            return Ok(None);
        }
        self.conllu.start_document();
        self.text.clear();
        self.carry.clear();
        self.tokens.clear();
        self.sentence_starts.clear();
        self.lines.clear();
        (self.ready, self.continued, self.first_line) = (0, false, 1);
        let mut sentence = false;
        let mut bytes = Vec::new();
        let mut end = false;
        let mut number = 0;
        let mut invalid = None;
        // A line longer than PARAGRAPH is read in pieces: the piece starts a line, and the line so
        // far is blank
        let mut starts = true;
        let mut line_blank = false;
        // A line of UTF-8 with non-ASCII bytes was read: before it, wrong bytes and NUL mean another
        // encoding of the file (cp1252, UTF-16 without BOM), after it a damaged line
        let mut utf8 = false;
        loop {
            bytes.clear();
            let Some(ended) = self.read_line(&mut reader, &mut bytes, &mut end)? else {
                break;
            };
            let first = starts;
            starts = ended;
            number += usize::from(first);
            if !utf8 && bytes.contains(&0) {
                return Err(Failure::Encoding(number));
            }
            let decoded = String::from_utf8_lossy(&bytes);
            match decoded {
                Cow::Owned(_) if !utf8 => return Err(Failure::Encoding(number)),
                Cow::Owned(_) => {
                    invalid.get_or_insert(number);
                }
                Cow::Borrowed(_) => utf8 |= !bytes.is_ascii(),
            }
            let mut line: &str = &decoded;
            if number == 1 && first {
                line = line.strip_prefix('\u{feff}').unwrap_or(line);
            }
            match self.input {
                Input::Text => {
                    line_blank = (first || line_blank) && blank(line);
                    self.push_text(line, line_blank, ended)?;
                }
                // Tokens and CoNLL-U are read by whole lines
                Input::Tokens => {
                    let line = line.trim_end_matches(['\n', '\r']);
                    if blank(line) {
                        // The tokens up to a blank line are complete sentences
                        sentence = false;
                        self.ready = self.tokens.len();
                        if self.ready >= INPUT_BUFFER / 8 {
                            self.write_ready()?;
                        }
                        continue;
                    }
                    // The token is up to a tab: a table with the token in its first column is read
                    // too
                    let token = line.split('\t').next().unwrap_or_default();
                    let token = token.trim_matches(is_separator);
                    if token.is_empty() {
                        continue;
                    }
                    if !std::mem::replace(&mut sentence, true) {
                        self.sentence_starts.push(self.tokens.len());
                    }
                    self.tokens.push(token.to_owned());
                    // A list of words without blank lines is not kept whole as one sentence
                    let start = self.sentence_starts.last().copied().unwrap_or_default();
                    if self.tokens.len() - start >= INPUT_BUFFER / 8 {
                        sentence = false;
                        self.ready = self.tokens.len();
                        self.write_ready()?;
                    }
                }
                Input::Conllu => {
                    if self.lines.is_empty() {
                        self.first_line = number;
                    }
                    let text = line.trim_end_matches(['\n', '\r']);
                    self.lines.push_str(text);
                    self.lines.push('\n');
                    if text.trim().is_empty() {
                        // The lines up to a blank one are complete sentences
                        self.ready = self.lines.len();
                        self.next_line = number + 1;
                        if self.ready >= INPUT_BUFFER {
                            self.write_ready()?;
                        }
                    }
                }
            }
        }
        match self.input {
            Input::Text => self.ready = self.text.len(),
            Input::Tokens => self.ready = self.tokens.len(),
            Input::Conllu => {
                if !self.lines.ends_with("\n\n") && !self.lines.is_empty() {
                    self.lines.push('\n');
                }
                self.ready = self.lines.len();
            }
        }
        self.write_ready()?;
        Ok(invalid)
    }

    /// A line of a text or a piece of a long line: a blank line ends the paragraphs before it,
    /// which are analyzed in pieces of about INPUT_BUFFER, or as soon as the next read may wait
    fn push_text(&mut self, line: &str, line_blank: bool, ended: bool) -> Result<(), Failure> {
        if !line_blank {
            self.text.push_str(line);
            if self.text.len() >= PARAGRAPH || !ended {
                self.write_long()?;
            }
        } else if ended || closes_paragraph(&format!("\n{line}")) {
            // It also ends a paragraph written whole, and so does a piece of a long line of
            // separators that holds the end of a paragraph
            if blank(&self.text) {
                self.continued = false;
            }
            if !self.text.is_empty() {
                self.text.push_str(line);
                self.ready = self.text.len();
            }
            if self.ready >= INPUT_BUFFER {
                self.write_ready()?;
            }
        }
        Ok(())
    }

    /// A line of the input with its line break (LF, CR or CRLF), or a piece of a line of text
    /// longer than PARAGRAPH, which ends after a space or else at a character; `true` - the line
    /// ended, `None` - the input did. Before a read that may wait for input (an empty buffer), the
    /// complete pieces are written and the output is flushed, so that the command can be kept open
    /// and fed text. After the end of the input (`end`) nothing is read: in a terminal a line
    /// without a line break ends with one Ctrl+D
    fn read_line(
        &mut self,
        reader: &mut BufReader<&mut dyn Read>,
        bytes: &mut Vec<u8>,
        end: &mut bool,
    ) -> Result<Option<bool>, Failure> {
        bytes.append(&mut self.carry);
        // A CR ended the buffer: the line feed of CRLF may begin the next one
        let mut after_cr = false;
        while !*end {
            if self.input == Input::Text && bytes.len() >= PARAGRAPH && !after_cr {
                let cut = (bytes.iter())
                    .rposition(|&byte| byte == b' ' || byte == b'\t')
                    .map(|at| at + 1)
                    .or_else(|| (1..bytes.len()).rev().find(|&at| bytes[at] & 0xC0 != 0x80))
                    .unwrap_or(bytes.len());
                self.carry = bytes.split_off(cut);
                return Ok(Some(false));
            }
            // The next read may wait for input: the complete pieces go out first
            if reader.buffer().is_empty() {
                self.write_ready()?;
                self.flush_pending()?;
            }
            let available = match reader.fill_buf() {
                Ok(available) => available,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(Failure::Read(err)),
            };
            if available.is_empty() {
                *end = true;
                break;
            }
            if after_cr {
                if available[0] == b'\n' {
                    bytes.push(b'\n');
                    reader.consume(1);
                }
                break;
            }
            let (length, done) =
                match (available.iter()).position(|&byte| byte == b'\n' || byte == b'\r') {
                    Some(at) if available[at] == b'\r' => match available.get(at + 1) {
                        Some(b'\n') => (at + 2, true),
                        Some(_) => (at + 1, true),
                        None => {
                            after_cr = true;
                            (at + 1, false)
                        }
                    },
                    Some(at) => (at + 1, true),
                    None => (available.len(), false),
                };
            bytes.extend_from_slice(&available[..length]);
            reader.consume(length);
            if done {
                break;
            }
        }
        Ok((!bytes.is_empty()).then_some(true))
    }

    /// The complete pieces of the input read so far: the text up to its last blank line, the
    /// tokens of the complete sentences, the lines of CoNLL-U up to the last blank one
    fn write_ready(&mut self) -> Result<(), Failure> {
        match self.input {
            Input::Text => {
                if self.ready == 0 {
                    return Ok(());
                }
                let rest = self.text.split_off(self.ready);
                let ready = std::mem::replace(&mut self.text, rest);
                self.ready = 0;
                let document = self.segmenter.segment_string(ready);
                let count = document.sentence_count();
                self.write_sentences(document, count)?;
                self.continued = false;
            }
            Input::Tokens => {
                if self.ready == 0 {
                    return Ok(());
                }
                let document = self.tokens_document()?;
                let count = document.sentence_count();
                self.write_sentences(document, count)?;
                // The next tokens of the file go on with its paragraph
                self.continued = true;
            }
            Input::Conllu => {
                if self.ready == 0 {
                    return Ok(());
                }
                let rest = self.lines.split_off(self.ready);
                let ready = std::mem::replace(&mut self.lines, rest);
                let mut first = std::mem::replace(&mut self.first_line, self.next_line);
                self.ready = 0;
                let mut lines = ready.as_str();
                while !lines.is_empty() {
                    let err = match read_conllu(lines) {
                        Ok(documents) => {
                            self.write_documents(documents)?;
                            break;
                        }
                        Err(err) => err,
                    };
                    // The wrong sentence is the lines between the blank ones around the wrong line
                    let line = err.line.max(1);
                    let (mut start, mut end) = (0, lines.len());
                    let mut at = 0;
                    for (number, text) in (1..).zip(lines.split_inclusive('\n')) {
                        at += text.len();
                        if text.trim().is_empty() {
                            if number >= line {
                                end = at;
                                break;
                            }
                            start = at;
                        }
                    }
                    let before = read_conllu(&lines[..start]).unwrap_or_default();
                    self.write_documents(before)?;
                    // It goes out as it is, and the file goes on
                    if self.format == Format::Conllu {
                        self.output.write_all(&lines.as_bytes()[start..end])?;
                        self.end_piece()?;
                    }
                    self.wrong.push((first + line - 1, err.message));
                    first += lines[..end].matches('\n').count();
                    lines = &lines[end..];
                }
            }
        }
        Ok(())
    }

    /// The document of the tokens of the complete sentences, taken from the writer: one paragraph,
    /// the tokens joined by spaces
    fn tokens_document(&mut self) -> Result<Document, Failure> {
        let rest = self.tokens.split_off(self.ready);
        let tokens = std::mem::replace(&mut self.tokens, rest);
        let split = (self.sentence_starts).partition_point(|&start| start < self.ready);
        let rest = (self.sentence_starts.split_off(split).into_iter())
            .map(|start| start - self.ready)
            .collect();
        let starts = std::mem::replace(&mut self.sentence_starts, rest);
        self.ready = 0;
        let mut text = String::new();
        let mut spans = Vec::with_capacity(tokens.len());
        for token in &tokens {
            if !text.is_empty() {
                text.push(' ');
            }
            // A line break inside a token would end a paragraph
            let token = one_line(token);
            spans.push(Span {
                start: text.len(),
                end: text.len() + token.len(),
            });
            text.push_str(&token);
        }
        // The tokens are not empty and have neither separators at their ends nor line breaks
        Document::from_tokens(text, spans, Some(&starts))
            .map_err(|err| Failure::Write(io::Error::other(err)))
    }

    /// A text without a blank line that reached PARAGRAPH, without its last two sentences, which
    /// the next line may go on with: they stay at the start of the text, and the next piece
    /// continues their paragraph. The segmenter decides the boundary before the last sentence by
    /// the text after it, which the next line may change. One sentence longer than PARAGRAPH is
    /// written whole, and the separators after it stay: with the next line they may end the
    /// paragraph
    fn write_long(&mut self) -> Result<(), Failure> {
        let text = std::mem::take(&mut self.text);
        let document = self.segmenter.segment_string(text);
        let count = document.sentence_count();
        let written = match count {
            0 | 1 => count,
            2 => 1,
            _ => count - 2,
        };
        let keep = match count {
            0 => document.text().len(),
            1 => document.sentence(0).span.end,
            _ => document.sentence(written).span.start,
        };
        let rest = document.text()[keep..].to_owned();
        // The sentences kept may themselves start a paragraph (after U+2029), and the separators
        // dropped may end one
        let continued = match count {
            0 => self.continued && !closes_paragraph(document.text()),
            1 => true,
            _ => (0..document.paragraph_count())
                .map(|paragraph| document.paragraph(paragraph).sentences)
                .find(|sentences| sentences.contains(&written))
                .is_some_and(|sentences| sentences.start < written),
        };
        self.write_sentences(document, written)?;
        self.continued = continued;
        self.text = rest;
        self.ready = 0;
        Ok(())
    }

    /// The first `count` sentences of a document in the format of the output, written sentence by
    /// sentence: in the format `lemmas` a blank line goes between the paragraphs of a text
    fn write_sentences(&mut self, mut document: Document, count: usize) -> Result<(), Failure> {
        if count == 0 {
            return Ok(());
        }
        // JSON Lines gives all analyses of the tokens, not the fields of the words
        let fields = match self.format {
            Format::Jsonl => Vec::new(),
            _ => self.dictionary.document_fields(&document),
        };
        if self.format == Format::Conllu {
            set_fields(&mut document, &fields).map_err(io::Error::other)?;
            // CoNLL-U goes back with its comments, multiword token lines and empty nodes
            if self.input == Input::Conllu {
                self.output.write_all(document.to_conllu().as_bytes())?;
                return Ok(self.end_piece()?);
            }
        }
        self.out.clear();
        // What the last piece kept and what was read since may end the paragraph before the first
        // token
        let lead = (document.tokens().first()).map_or("", |token| &document.text()[..token.start]);
        for paragraph in 0..document.paragraph_count() {
            let sentences = document.paragraph(paragraph).sentences;
            if sentences.start >= count {
                break;
            }
            if std::mem::take(&mut self.continued) && !closes_paragraph(lead) {
                self.conllu.continue_paragraph();
            } else {
                if self.format == Format::Lemmas && self.input == Input::Text && self.paragraphs > 0
                {
                    self.out.push('\n');
                }
                self.paragraphs += 1;
            }
            for sentence in sentences.start..sentences.end.min(count) {
                match self.format {
                    Format::Lemmas => {
                        for (number, word) in document.sentence(sentence).words.enumerate() {
                            if number > 0 {
                                self.out.push(' ');
                            }
                            self.out.push_str(&fields[word].lemma);
                        }
                        self.out.push('\n');
                    }
                    Format::Tsv => {
                        for word in document.sentence(sentence).words {
                            let analysis = &fields[word];
                            let _ = writeln!(
                                self.out,
                                "{}\t{}\t{}\t{}\t{}",
                                one_line(document.form(word)),
                                analysis.lemma,
                                analysis.pos,
                                analysis.feats.unwrap_or("_"),
                                analysis.prob
                            );
                        }
                        self.out.push('\n');
                    }
                    Format::Jsonl => self.json_sentence(&document, sentence),
                    Format::Conllu => {
                        (self.conllu).write(&document, sentence..sentence + 1, &mut self.out);
                    }
                }
                self.output.write_all(self.out.as_bytes())?;
                self.out.clear();
            }
        }
        Ok(self.end_piece()?)
    }

    /// The documents of CoNLL-U read
    fn write_documents(&mut self, documents: Vec<Document>) -> Result<(), Failure> {
        for document in documents {
            let count = document.sentence_count();
            self.write_sentences(document, count)?;
        }
        Ok(())
    }

    /// A sentence in JSON Lines: its text (not for tokens, which have none) and every token with
    /// all its analyses
    fn json_sentence(&mut self, document: &Document, sentence: usize) {
        let sentence = document.sentence(sentence);
        self.out.push('{');
        if self.input != Input::Tokens {
            self.out.push_str("\"text\": ");
            json_string(&mut self.out, sentence.span.slice(document.text()));
            self.out.push_str(", ");
        }
        self.out.push_str("\"tokens\": [");
        for (number, token) in sentence.tokens.enumerate() {
            if number > 0 {
                self.out.push_str(", ");
            }
            let text = document.tokens()[token].slice(document.text());
            self.out.push_str("{\"word\": ");
            json_string(&mut self.out, text);
            self.out.push_str(", \"analyses\": [");
            for (index, analysis) in self.dictionary.analyze(text).iter().enumerate() {
                if index > 0 {
                    self.out.push_str(", ");
                }
                json_analysis(&mut self.out, self.dictionary, analysis);
            }
            self.out.push_str("]}");
        }
        self.out.push_str("]}\n");
    }

    /// The end of a piece written: in a terminal the output is flushed at once
    fn end_piece(&mut self) -> io::Result<()> {
        self.pending = true;
        if self.interactive {
            self.flush_pending()?;
        }
        Ok(())
    }

    fn flush_pending(&mut self) -> io::Result<()> {
        if self.pending {
            self.output.flush()?;
            self.pending = false;
        }
        Ok(())
    }
}

/// An analysis in JSON: the fields of `Analysis` of the Python module
fn json_analysis(out: &mut String, dictionary: &Dictionary, analysis: &Analysis) {
    let tag = dictionary.ud_tag(analysis.ud);
    out.push_str("{\"word\": ");
    json_string(out, &analysis.word);
    out.push_str(", \"lemma\": ");
    json_string(out, &analysis.lemma);
    out.push_str(", \"pos\": ");
    json_string(out, tag.map_or("X", |tag| tag.pos()));
    out.push_str(", \"feats\": {");
    for (number, (name, value)) in tag.into_iter().flat_map(|tag| tag.feats()).enumerate() {
        if number > 0 {
            out.push_str(", ");
        }
        json_string(out, name);
        out.push_str(": ");
        json_string(out, value);
    }
    let _ = write!(out, "}}, \"prob\": {}, \"words\": [", analysis.prob);
    // A token of one word has it as its only word
    let words: &[String] = match analysis.words.as_slice() {
        [] => std::slice::from_ref(&analysis.word),
        words => words,
    };
    for (number, word) in words.iter().enumerate() {
        if number > 0 {
            out.push_str(", ");
        }
        json_string(out, word);
    }
    let _ = write!(out, "], \"source\": \"{}\"}}", analysis.source.as_str());
}

/// A text of nothing but separators
fn blank(text: &str) -> bool {
    text.chars().all(is_separator)
}

/// A separator between tokens, as anySeg has it: whitespace, control characters and the invisible
/// separators
fn is_separator(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(
            c,
            '\u{200b}' | '\u{2060}' | '\u{feff}' | '\u{ad}' | '\u{200c}' | '\u{200d}'
        )
}

/// The separators after a sentence hold the end of its paragraph: U+2029 or a blank line
fn closes_paragraph(separators: &str) -> bool {
    if separators.contains('\u{2029}') {
        return true;
    }
    let mut lines = 0;
    let mut chars = separators.chars().peekable();
    while let Some(c) = chars.next() {
        let line_break = match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                true
            }
            '\n' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' => true,
            _ => false,
        };
        if line_break {
            lines += 1;
            if lines > 1 {
                return true;
            }
        } else if !is_separator(c) {
            lines = 0;
        }
    }
    false
}

/// The line at which the start of the input shows another encoding than UTF-8: a byte order mark of
/// UTF-16, NUL or wrong bytes on a line before the first line of UTF-8 with non-ASCII characters
fn foreign_encoding(head: &[u8]) -> Option<usize> {
    if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) {
        return Some(1);
    }
    for (index, line) in lines(head).enumerate() {
        if line.contains(&0) {
            return Some(index + 1);
        }
        if line.is_ascii() {
            continue;
        }
        return match std::str::from_utf8(line) {
            // The line is cut by the end of the buffer in the middle of a character: the reading
            // of lines decides
            Err(err) if err.error_len().is_some() => Some(index + 1),
            _ => None,
        };
    }
    None
}

/// The lines of bytes as the command reads them: each ends after LF, CR or CRLF
fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = bytes;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = match rest.iter().position(|&byte| byte == b'\n' || byte == b'\r') {
            Some(at) if rest[at] == b'\r' && rest.get(at + 1) == Some(&b'\n') => at + 2,
            Some(at) => at + 1,
            None => rest.len(),
        };
        let (line, after) = rest.split_at(end);
        rest = after;
        Some(line)
    })
}

/// A string of JSON
fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Line breaks of Unicode too: a reader of lines would cut a line of JSON Lines there
            c if u32::from(c) < 0x20 || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}') => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(all(test, feature = "bundled"))]
mod tests {
    use super::*;

    /// The exit code, the output and the messages of the command on an input
    fn command(args: &[&str], input: &[u8]) -> (u8, String, String) {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let code = execute(
            args.iter().map(OsString::from).collect(),
            "1.2.3",
            None,
            false,
            false,
            &mut &input[..],
            &mut stdout,
            &mut stderr,
        );
        (
            code,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    /// The output of a command that succeeds
    fn output(args: &[&str], input: &str) -> String {
        let (code, out, err) = command(args, input.as_bytes());
        assert_eq!((code, err.as_str()), (0, ""), "{args:?}");
        out
    }

    /// CoNLL-U of a whole text segmented and annotated at once
    fn whole(text: &str) -> String {
        let mut document = Segmenter::new("es").unwrap().segment(text);
        Dictionary::bundled()
            .unwrap()
            .annotate(&mut document)
            .unwrap();
        document.to_conllu()
    }

    /// A file of the temporary directory with a name of this process
    fn temporary(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("esmorph-cli-{}-{name}", std::process::id()))
    }

    const TEXT: &str =
        "Dámelo, dime la verdad del caso.\nOtra frase\ncontinúa aquí.\n\n\nVoy a verlas.";

    #[test]
    fn lemmas_by_paragraphs() {
        let lemmas = "dar yo él , decir yo el verdad de el caso .\notro frase continuar aquí .\n\n\
                      ir a ver él .\n";
        assert_eq!(output(&[], TEXT), lemmas);
        // CR and CRLF are line breaks as LF
        assert_eq!(output(&[], &TEXT.replace('\n', "\r")), lemmas);
        assert_eq!(output(&[], &TEXT.replace('\n', "\r\n")), lemmas);
        assert_eq!(output(&[], ""), "");
        assert_eq!(output(&[], "\n \n\t\n"), "");
    }

    #[test]
    fn conllu_as_annotate() {
        // Line breaks of every kind, blank lines of separators, a byte order mark, no final line
        // break, multiword tokens
        for text in [
            TEXT,
            "\u{feff}Uno. Dos\r\ntres\r\n \r\nCuatro\u{2028}cinco.\n\u{200b}\nSeis\u{2029}Siete del mar\n\n\n",
        ] {
            assert_eq!(output(&["-f", "conllu"], text), whole(text), "{text:?}");
        }
        // Paragraphs in pieces of about the input buffer
        let text = (0..2_000)
            .map(|number| format!("Párrafo {number}. Dímelo\n\n"))
            .collect::<String>();
        assert!(text.len() > 4 * INPUT_BUFFER);
        assert_eq!(output(&["-f", "conllu"], &text), whole(&text));
        // A long text without blank lines in pieces, in one paragraph
        let sentence = output(&[], "Mamá come la fruta.");
        assert_eq!(sentence, "mamá comer el fruta .\n");
        for text in [
            "Mamá come la fruta.\n".repeat(PARAGRAPH / 2),
            "Mamá come\nla fruta.\r".repeat(PARAGRAPH / 2),
            "Mamá come la fruta. ".repeat(PARAGRAPH / 2),
        ] {
            let conllu = output(&["-f", "conllu"], &text);
            assert_eq!(conllu.matches("# newpar").count(), 1);
            assert_eq!(conllu, whole(&text));
            assert_eq!(output(&[], &text), sentence.repeat(PARAGRAPH / 2));
        }
        // A long line cut after the first word of a line: the segmenter ends a sentence before it,
        // as nothing follows
        let lines = "Muchos marineros solicitaban permiso para vivir en\x0bla ciudad, y el \
                     vapor quedaba a la guarda del tío.\x0b";
        let mut text = lines.repeat((PARAGRAPH - 100) / lines.len());
        text.push_str("El pequeño Ferragut era el capitán del\x0bMare ");
        text.push_str(&"o".repeat(INPUT_BUFFER + 100));
        text.push_str(".\x0bFin.");
        assert_eq!(output(&["-f", "conllu"], &text), whole(&text));
        // A line without spaces longer than PARAGRAPH is cut at a character
        let word = "ñ".repeat(PARAGRAPH);
        assert_eq!(output(&[], &word).replace(['\n', ' '], ""), word);
    }

    #[test]
    fn conllu_numbers_sentences_through_files() {
        let path = temporary("second.txt");
        std::fs::write(&path, "Dime.").unwrap();
        let file = path.to_str().unwrap();
        let conllu = output(&["-f", "conllu", "-", file], TEXT);
        std::fs::remove_file(&path).unwrap();
        let second = whole("Dime.").replace("# sent_id = 1", "# sent_id = 4");
        assert_eq!(conllu, format!("{}{second}", whole(TEXT)));
    }

    #[test]
    fn formats() {
        assert_eq!(
            output(&["-f", "tsv"], "Dime la verdad."),
            "Di\tdecir\tVERB\tMood=Imp|Number=Sing|Person=2|VerbForm=Fin\t1\n\
             me\tyo\tPRON\tCase=Dat|Number=Sing|Person=1|PrepCase=Npr|PronType=Prs\t0.45766\n\
             la\tel\tDET\tDefinite=Def|Gender=Fem|Number=Sing|PronType=Art\t0.9774450451099099\n\
             verdad\tverdad\tNOUN\tGender=Fem|Number=Sing\t0.999997\n\
             .\t.\tPUNCT\tPunctType=Peri\t1\n\n"
        );
        let jsonl = output(&["-f", "jsonl"], "Dime.\n\nqzxw\n");
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].starts_with(
                "{\"text\": \"Dime.\", \"tokens\": [{\"word\": \"Dime\", \"analyses\": [{\"word\": \"Dime\", \
                 \"lemma\": \"Dime\", \"pos\": \"PROPN\", \"feats\": {}, \"prob\": 0.9375, \
                 \"words\": [\"Dime\"], \"source\": \"rule\"}, {\"word\": \"dime\", \"lemma\": \"decir\", \
                 \"pos\": \"VERB\", \"feats\": {\"Mood\": \"Imp\", \"Number\": \"Sing\", \"Person\": \"2\", \
                 \"VerbForm\": \"Fin\"}, \"prob\": 0.0625, \"words\": [\"di\", \"me\"], \
                 \"source\": \"clitics\"}]}"
            ),
            "{jsonl}"
        );
        // The probability of a word chosen by its token or as a clitic is among the analyses that fit
        let tsv = output(&["-f", "tsv"], "Quiero verla.");
        assert!(tsv.contains("\nla\tél\tPRON\tCase=Acc|Gender=Fem|Number=Sing|Person=3|PrepCase=Npr|PronType=Prs\t1\n"), "{tsv}");
        // A token without analyses has none
        assert_eq!(
            lines[1],
            "{\"text\": \"qzxw\", \"tokens\": [{\"word\": \"qzxw\", \"analyses\": []}]}"
        );
        // Tabs and line breaks of a word are spaces in its line of TSV
        let tsv = output(
            &["-f", "tsv", "-i", "conllu"],
            "1\tq\u{2028}z\t_\t_\t_\t_\t_\t_\t_\t_\n",
        );
        assert_eq!(tsv, "q z\tq z\tX\t_\t0\n\n");
    }

    #[test]
    fn tokens() {
        // A token per line up to a tab, separators at its ends dropped; blank lines between
        // sentences
        let input = "dime\nla\nverdad\n \n\n\u{200b}del\u{200b}\tADP\n\tX\ncaso";
        assert_eq!(
            output(&["-i", "tokens"], input),
            "decir el verdad\nde caso\n"
        );
        let conllu = output(&["-i", "tokens", "-f", "conllu"], input);
        assert!(
            conllu.starts_with(
                "# newdoc\n# newpar\n# sent_id = 1\n# text = dime la verdad\n1\tdime\tdecir\tVERB"
            ),
            "{conllu}"
        );
        assert!(
            conllu.contains("# sent_id = 2\n# text = del caso\n1\tdel\tde\tADP"),
            "{conllu}"
        );
        let jsonl = output(&["-i", "tokens", "-f", "jsonl"], "dime\n");
        assert!(
            jsonl.starts_with("{\"tokens\": [{\"word\": \"dime\""),
            "{jsonl}"
        );
        // Many sentences go in pieces, in one paragraph of one document; a line break inside a
        // token does not end it
        let many = "dime\nla\nverdad\n\n".repeat(INPUT_BUFFER) + "a\u{2029}b\n";
        let conllu = output(&["-i", "tokens", "-f", "conllu"], &many);
        assert_eq!(conllu.matches("# newdoc").count(), 1);
        assert_eq!(conllu.matches("# newpar").count(), 1);
        let last = INPUT_BUFFER + 1;
        assert!(
            conllu.ends_with(&format!(
                "# sent_id = {last}\n# text = a b\n1\ta b\ta b\tX\t_\t_\t_\t_\t_\t_\n\n"
            )),
            "{}",
            &conllu[conllu.len() - 200..]
        );
        let lemmas = output(&["-i", "tokens"], &many);
        assert_eq!(lemmas.lines().count(), last);
        // A list without blank lines is closed in sentences of a fixed number of tokens
        let size = INPUT_BUFFER / 8;
        let list = output(&["-i", "tokens"], &"mar\n".repeat(2 * size + 1));
        let sentence = vec!["mar"; size].join(" ") + "\n";
        assert_eq!(list, format!("{sentence}{sentence}mar\n"));
    }

    #[test]
    fn conllu_input() {
        let input = "# newdoc id = d1\n# text = Dime del mar\n1-2\tDime\t_\t_\t_\t_\t_\t_\t_\t_\n\
                     1\tDi\tx\tX\tvmm\tA=B\t0\troot\t_\t_\n2\tme\t_\t_\t_\t_\t1\tiobj\t_\t_\n\
                     3-4\tdel\t_\t_\t_\t_\t_\t_\t_\t_\n3\tde\t_\t_\t_\t_\t5\tcase\t_\t_\n\
                     4\tel\t_\t_\t_\t_\t5\tdet\t_\t_\n5\tmar\t_\t_\t_\t_\t1\tobl\t_\tSpaceAfter=No\n";
        let conllu = output(&["-i", "conllu", "-f", "conllu"], input);
        assert_eq!(
            conllu,
            "# newdoc id = d1\n# text = Dime del mar\n1-2\tDime\t_\t_\t_\t_\t_\t_\t_\t_\n\
             1\tDi\tdecir\tVERB\tvmm\tMood=Imp|Number=Sing|Person=2|VerbForm=Fin\t0\troot\t_\t_\n\
             2\tme\tyo\tPRON\t_\tCase=Dat|Number=Sing|Person=1|PrepCase=Npr|PronType=Prs\t1\tiobj\t_\t_\n\
             3-4\tdel\t_\t_\t_\t_\t_\t_\t_\t_\n3\tde\tde\tADP\t_\t_\t5\tcase\t_\t_\n\
             4\tel\tel\tDET\t_\tDefinite=Def|Gender=Masc|Number=Sing|PronType=Art\t5\tdet\t_\t_\n\
             5\tmar\tmar\tNOUN\t_\tGender=Masc|Number=Sing\t1\tobl\t_\tSpaceAfter=No\n\n"
        );
        assert_eq!(output(&["-i", "conllu"], input), "decir yo de el mar\n");
        // A wrong line in a later piece: its number in the file
        let sentence = "1\tmar\t_\t_\t_\t_\t_\t_\t_\t_\n\n";
        let mut many = sentence.repeat(INPUT_BUFFER);
        many.push_str("1\tmar\n");
        let (code, out, err) = command(&["-i", "conllu"], many.as_bytes());
        assert_eq!(code, 1);
        assert_eq!(out, "mar\n".repeat(INPUT_BUFFER));
        let line = 2 * INPUT_BUFFER + 1;
        assert!(err.contains(&format!("-: line {line}: ")), "{err}");
        // The sentences of the piece before the wrong one are written
        let few = sentence.repeat(2) + "1\tmar\n" + sentence;
        let (code, out, err) = command(&["-i", "conllu"], few.as_bytes());
        assert_eq!((code, out.as_str()), (1, "mar\nmar\n"));
        assert!(err.contains("-: line 5: "), "{err}");
        let wrong_text = format!("{sentence}# text = mares\n{sentence}");
        let (code, out, err) = command(&["-i", "conllu"], wrong_text.as_bytes());
        assert_eq!((code, out.as_str()), (1, "mar\n"));
        assert!(err.contains("-: line 3: "), "{err}");
        // A wrong sentence is skipped, or written as it is in CoNLL-U, and the file goes on
        let wrong = "# sent_id = 2\n1\tmar\n\n";
        let input = format!("{sentence}{wrong}{sentence}{wrong}{sentence}");
        let (code, out, err) = command(&["-i", "conllu"], input.as_bytes());
        assert_eq!((code, out.as_str()), (1, "mar\nmar\nmar\n"));
        assert_eq!(
            err,
            "esmorph: -: line 4: expected 10 tab-separated columns, got 2; the sentence is skipped\n\
             esmorph: -: line 9: expected 10 tab-separated columns, got 2; the sentence is skipped\n"
        );
        let analyzed = "1\tmar\tmar\tNOUN\t_\tGender=Masc|Number=Sing\t_\t_\t_\t_\n\n";
        let (code, out, err) = command(&["-i", "conllu", "-f", "conllu"], input.as_bytes());
        assert_eq!(code, 1);
        assert_eq!(out, format!("{analyzed}{wrong}{analyzed}{wrong}{analyzed}"));
        assert!(
            err.contains("; the sentence is written as it is\n"),
            "{err}"
        );
    }

    #[test]
    fn arguments() {
        let (code, out, _) = command(&["--help"], b"");
        assert_eq!((code, out.as_str()), (0, HELP));
        assert_eq!(
            command(&["-V"], b""),
            (0, "esmorph 1.2.3\n".into(), String::new())
        );
        for (args, message) in [
            (
                &["-f", "json"][..],
                "unknown format json, expected one of: lemmas, conllu, tsv, jsonl",
            ),
            (
                &["--input=xml"],
                "unknown input xml, expected one of: text, tokens, conllu",
            ),
            (&["--frmat"], "unknown option --frmat"),
            (&["-o"], "option -o needs a value"),
            (&["--help=yes"], "unknown option --help=yes"),
        ] {
            let (code, out, err) = command(args, b"");
            assert_eq!((code, out.as_str()), (2, ""), "{args:?}");
            assert_eq!(err, format!("esmorph: {message}\nHelp: esmorph --help\n"));
        }
        let parsed = parse(
            [
                "-ftsv",
                "--input=tokens",
                "-d",
                "es.dict",
                "-o",
                "-",
                "--",
                "-f",
                "a.txt",
            ]
            .iter()
            .map(OsString::from)
            .collect(),
        );
        assert_eq!(
            parsed,
            Ok(Command::Run {
                format: Format::Tsv,
                input: Input::Tokens,
                dictionary: Some(PathBuf::from("es.dict")),
                output: None,
                files: vec!["-f".into(), "a.txt".into()],
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn option_not_in_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let args = vec![OsString::from_vec(b"--output=\xff.out".to_vec())];
        let code = execute(
            args,
            "",
            None,
            false,
            false,
            &mut &b""[..],
            &mut stdout,
            &mut stderr,
        );
        assert_eq!(code, 2);
        let message = String::from_utf8_lossy(&stderr);
        assert!(
            message.contains("cannot read the option --output="),
            "{message}"
        );
    }

    #[test]
    fn encodings() {
        let (code, out, err) = command(&[], &[0xFF, 0xFE, b'a', 0]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.contains("not in UTF-8 (line 1)"), "{err}");
        // Latin-1 before any UTF-8: another encoding, nothing is written
        let (code, out, err) = command(&[], b"ok\ncaf\xe9\n");
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(err, "esmorph: -: the file is not in UTF-8 (line 2)\n");
        // A damaged line after UTF-8: its wrong bytes are U+FFFD
        let mut input = "Mamá.\n".as_bytes().to_vec();
        input.extend(b"a\xffb\n");
        let (code, out, err) = command(&[], &input);
        assert_eq!((code, out.as_str()), (1, "mamá . a \u{fffd} b\n"));
        assert_eq!(
            err,
            "esmorph: -: line 2: not UTF-8, the wrong bytes are replaced with U+FFFD\n"
        );
    }

    #[test]
    fn files_and_dictionaries() {
        // A file that does not open is reported, and the next files are read
        let (code, out, err) = command(&["/no/such/file", "-"], "Dime".as_bytes());
        assert_eq!((code, out.as_str()), (1, "decir yo\n"));
        assert_eq!(err, "esmorph: /no/such/file: no such file\n");
        let (code, _, err) = command(&["-d", "/no/such.dict"], b"");
        assert_eq!(
            (code, err.as_str()),
            (1, "esmorph: /no/such.dict: no such file\n")
        );
        let broken = temporary("broken.dict");
        std::fs::write(&broken, "not a dictionary").unwrap();
        let (code, _, err) = command(&["-d", broken.to_str().unwrap()], b"");
        assert_eq!(code, 1);
        assert!(err.contains("wrong dictionary format"), "{err}");
        // A damaged automaton with a right checksum: the dictionary of the option is verified
        let clean = esmorph_dict_es::DICTIONARY.to_vec();
        let layout = crate::format::layout(&clean, true).unwrap();
        let damaged = (layout.words.clone())
            .map(|at| {
                let mut damaged = clean.clone();
                damaged[at] ^= 0xFF;
                crate::format::tests::with_checksum(damaged)
            })
            .find(|damaged| {
                Dictionary::from_bytes(damaged).is_ok_and(|dictionary| dictionary.verify().is_err())
            })
            .unwrap();
        std::fs::write(&broken, damaged).unwrap();
        let (code, _, err) = command(&["-d", broken.to_str().unwrap()], b"Dime");
        assert_eq!(code, 1);
        assert!(
            err.ends_with(
                "broken.dict: wrong dictionary format: the automaton of word forms is damaged\n"
            ),
            "{err}"
        );
        let dictionary = temporary("es.dict");
        std::fs::write(&dictionary, esmorph_dict_es::DICTIONARY).unwrap();
        // The output file is created anew, but not over an input or the dictionary
        let (input, written) = (temporary("in.txt"), temporary("out.txt"));
        std::fs::write(&input, "Dime").unwrap();
        let path = |path: &Path| path.to_str().unwrap().to_owned();
        let (code, out, err) = command(&["-o", &path(&written), &path(&input)], b"");
        assert_eq!((code, out.as_str(), err.as_str()), (0, "", ""));
        assert_eq!(std::fs::read_to_string(&written).unwrap(), "decir yo\n");
        for args in [
            vec!["-o".to_owned(), path(&input), path(&input)],
            vec![
                "-d".to_owned(),
                path(&dictionary),
                "-o".to_owned(),
                path(&dictionary),
            ],
        ] {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let (code, _, err) = command(&args, b"");
            assert_eq!(code, 1, "{args:?}");
            assert!(
                err.ends_with("the output file is an input file or the dictionary\n"),
                "{err}"
            );
        }
        #[cfg(unix)]
        {
            let link = temporary("link.txt");
            std::fs::hard_link(&input, &link).unwrap();
            let (code, _, _) = command(&["-o", &path(&link), &path(&input)], b"");
            assert_eq!(code, 1);
            std::fs::remove_file(&link).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&input).unwrap(), "Dime");
        // A directory, as an input and as the output
        let dir = std::env::temp_dir();
        let (code, _, err) = command(&[&path(&dir)], b"");
        assert_eq!(
            (code, err),
            (1, format!("esmorph: {}: is a directory\n", dir.display()))
        );
        let (code, _, err) = command(&["-o", &path(&dir)], b"");
        assert_eq!(
            (code, err),
            (1, format!("esmorph: {}: is a directory\n", dir.display()))
        );
        for file in [broken, dictionary, input, written] {
            std::fs::remove_file(file).unwrap();
        }
    }

    /// An output that counts flushes and may fail on them
    struct Output {
        data: Vec<u8>,
        flushes: usize,
        failing: bool,
    }

    impl Write for Output {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.data.extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            if self.failing {
                return Err(io::Error::other("disk full"));
            }
            Ok(())
        }
    }

    fn run_with(interactive: bool, input: &mut dyn Read, output: &mut Output) -> (u8, String) {
        let mut stderr = Vec::new();
        let code = execute(
            Vec::new(),
            "",
            None,
            interactive,
            false,
            input,
            output,
            &mut stderr,
        );
        (code, String::from_utf8(stderr).unwrap())
    }

    #[test]
    fn flushes() {
        let mut output = Output {
            data: Vec::new(),
            flushes: 0,
            failing: false,
        };
        // In a terminal after every piece and at the end
        let (code, _) = run_with(true, &mut "Dime\n\nDime\n".as_bytes(), &mut output);
        assert_eq!(code, 0);
        assert_eq!(output.data, "decir yo\n\ndecir yo\n".as_bytes());
        assert_eq!(output.flushes, 3);
        // To a file or a pipe before a read that may wait for input, and at the end
        struct Pieces(Vec<&'static [u8]>);
        impl Read for Pieces {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let Some(piece) = self.0.pop() else {
                    return Ok(0);
                };
                buffer[..piece.len()].copy_from_slice(piece);
                Ok(piece.len())
            }
        }
        output.data.clear();
        output.flushes = 0;
        let mut input = Pieces(vec![b"Dime\n\n", b"Dime\n\n"]);
        assert_eq!(run_with(false, &mut input, &mut output).0, 0);
        assert_eq!(output.data, "decir yo\n\ndecir yo\n".as_bytes());
        assert_eq!(output.flushes, 3);
        // A failed last flush is an error
        output.failing = true;
        assert_eq!(
            run_with(false, &mut "Dime".as_bytes(), &mut output),
            (1, "esmorph: write error: disk full\n".into())
        );
    }

    #[test]
    fn end_of_input_read_once() {
        // In a terminal every read after the end would wait for another Ctrl+D
        struct Terminal(Vec<&'static [u8]>);
        impl Read for Terminal {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let piece = self.0.pop().expect("a read after the end of the input");
                buffer[..piece.len()].copy_from_slice(piece);
                Ok(piece.len())
            }
        }
        for input in ["Dime".as_bytes(), b""] {
            let mut output = Output {
                data: Vec::new(),
                flushes: 0,
                failing: false,
            };
            let mut terminal = Terminal(vec![b"", input]);
            assert_eq!(run_with(true, &mut terminal, &mut output).0, 0);
        }
    }

    #[test]
    fn closed_outputs() {
        // A closed pipe is no error, a closed standard output is
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
        }
        let mut stderr = Vec::new();
        let code = execute(
            Vec::new(),
            "",
            None,
            false,
            false,
            &mut "Dime\n".repeat(100_000).as_bytes(),
            &mut Closed,
            &mut stderr,
        );
        assert_eq!((code, stderr.len()), (0, 0));
        for args in [vec![], vec![OsString::from("--help")]] {
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            let code = execute(
                args,
                "",
                None,
                false,
                true,
                &mut &b"Dime"[..],
                &mut stdout,
                &mut stderr,
            );
            assert_eq!(code, 1);
            assert_eq!(stderr, b"esmorph: the standard output is closed\n");
        }
    }

    #[test]
    fn escapes_json() {
        let mut out = String::new();
        json_string(&mut out, "a\"b\\c\n\t\r\u{1}ñ\u{85}\u{2028}\u{2029}");
        assert_eq!(out, "\"a\\\"b\\\\c\\n\\t\\r\\u0001ñ\\u0085\\u2028\\u2029\"");
    }
}
