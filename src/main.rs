use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

const BUFFER_SIZE: usize = 64 * 1024;

#[derive(Debug)]
struct Options {
    files: Vec<String>,
    separator: Vec<u8>,
    output: Option<PathBuf>,
    stdin_positions: Vec<usize>,
    stdin_ignored: bool,
    show_help: bool,
    clean: bool,
    trim: bool,
}

fn print_help(program: &str) {
    println!(
        r#"Usage:
  {program} [OPTIONS] [FILES...]

Concatenate files in the order given.

Options:
  --sep=VALUE
      Bytes placed between each input value.

      Special value:
        EOL     Use the current operating-system line ending.

      Examples:
        --sep=EOL
        --sep=---
        --sep="\\n"

  --out=FILE
      Write output to FILE instead of stdout.

  --out
  --out=
      Write output to a file named with the current timestamp:
        yyyymmddhhmmss.txt

  --push-stdin-at=POSITION
      Insert non-interactive stdin at the specified position.

      POSITION may be:
        first   Same as position 0
        last    After all files
        ignore  Do not include stdin
        NUMBER  Zero-based insertion position

      Position 0 places stdin before the first file.
      Position 3 places stdin after the second file and before
      the third file.

      Positions larger than the number of files mean last.

      This option may be repeated. Duplicate positions are merged.

      If stdin is non-interactive and this option is not used,
      stdin is inserted first by default.

  --clean
      Remove all control and terminal characters (ANSI escape sequences,
      control characters, etc.) from the input.

  --trim
      Remove all leading and trailing whitespace from each input
      (files and stdin) before concatenation.

  -h, --help
      Show this help.

Examples:
  {program} a.txt b.txt

  {program} --sep=EOL a.txt b.txt

  cat header.txt | {program} --sep=EOL a.txt b.txt

  cat header.txt | {program} \
      --push-stdin-at=last \
      --sep=EOL a.txt b.txt

  {program} --out=combined.txt a.txt b.txt

  {program} --clean --trim --sep=EOL a.txt b.txt
"#
    );
}

fn parse_args() -> Result<Options, String> {
    let mut files = Vec::new();
    let mut separator = Vec::new();
    let mut output = None;
    let mut stdin_positions = Vec::new();
    let mut stdin_ignored = false;
    let mut show_help = false;
    let mut clean = false;
    let mut trim = false;
    let mut separator_was_set = false;

    let args: Vec<String> = env::args().skip(1).collect();

    for arg in args {
        if arg == "-h" || arg == "--help" {
            show_help = true;
        } else if arg == "--clean" {
            clean = true;
        } else if arg == "--trim" {
            trim = true;
        } else if let Some(value) = arg.strip_prefix("--sep=") {
            separator = parse_separator(value)?;
            separator_was_set = true;
        } else if arg == "--out" || arg == "--out=" {
            output = Some(PathBuf::new());
        } else if let Some(value) = arg.strip_prefix("--out=") {
            output = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--push-stdin-at=") {
            match value.to_ascii_lowercase().as_str() {
                "ignore" => {
                    stdin_ignored = true;
                    stdin_positions.clear();
                }
                "first" => {
                    if !stdin_ignored {
                        stdin_positions.push(0);
                    }
                }
                "last" => {
                    if !stdin_ignored {
                        stdin_positions.push(usize::MAX);
                    }
                }
                number => {
                    let position = number
                        .parse::<usize>()
                        .map_err(|_| format!("invalid --push-stdin-at value: {value}"))?;

                    if !stdin_ignored {
                        stdin_positions.push(position);
                    }
                }
            }
        } else if arg.starts_with("--") {
            return Err(format!("unknown option: {arg}"));
        } else {
            files.push(arg);
        }
    }

    if !separator_was_set {
        separator.clear();
    }

    Ok(Options {
        files,
        separator,
        output,
        stdin_positions,
        stdin_ignored,
        show_help,
        clean,
        trim,
    })
}

fn parse_separator(value: &str) -> Result<Vec<u8>, String> {
    if value == "EOL" {
        return Ok(if cfg!(windows) {
            b"\r\n".to_vec()
        } else {
            b"\n".to_vec()
        });
    }

    // Allow common escaped byte values while preserving all other bytes.
    let mut result = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 1 < bytes.len() {
            index += 1;

            match bytes[index] {
                b'n' => result.push(b'\n'),
                b'r' => result.push(b'\r'),
                b't' => result.push(b'\t'),
                b'\\' => result.push(b'\\'),
                b'0' => result.push(0),
                other => {
                    result.push(b'\\');
                    result.push(other);
                }
            }
        } else {
            result.push(bytes[index]);
        }

        index += 1;
    }

    Ok(result)
}

/// Check if byte is any kind of whitespace (space, tab, newline, carriage return, etc.)
#[inline]
fn is_whitespace(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C // space, tab, LF, CR, vtab, form feed
    )
}

/// Check if byte is a control character that should be removed with --clean
#[inline]
fn is_control_or_ansi(b: u8) -> bool {
    b < 0x20 && b != b'\t' // control chars except tab
        || b == 0x7F // DEL
}

/// Process a buffer for cleaning and/or trimming, writing valid output to `out_buf`
/// Returns the number of bytes written to `out_buf`
fn process_buffer(
    buffer: &[u8],
    out_buf: &mut [u8],
    clean: bool,
    trim: bool,
) -> usize {
    if !clean && !trim {
        return 0; // No processing needed
    }

    let mut out_idx = 0;
    let mut i = 0;

    // Skip leading whitespace if trimming
    if trim {
        while i < buffer.len() && is_whitespace(buffer[i]) {
            i += 1;
        }
    }

    // Find the end position (before trailing whitespace if trimming)
    let mut end = buffer.len();
    if trim {
        while end > i && is_whitespace(buffer[end - 1]) {
            end -= 1;
        }
    }

    // Process the content, handling ANSI/control chars if cleaning
    while i < end && out_idx < out_buf.len() {
        let b = buffer[i];

        if clean && b == 0x1B {
            // Handle ANSI escape sequences
            i += 1;
            if i >= end {
                break;
            }

            let next = buffer[i];
            if next == b'[' {
                // CSI sequence: ESC [ ... (letter)
                i += 1;
                while i < end {
                    let c = buffer[i];
                    if (0x40..=0x7E).contains(&c) {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            } else if next == b']' {
                // OSC sequence: ESC ] ... (BEL or ST)
                i += 1;
                while i < end {
                    let c = buffer[i];
                    if c == 0x07 {
                        i += 1;
                        break;
                    }
                    if c == 0x1B && i + 1 < end && buffer[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            } else if matches!(next, b'@'..=b'Z' | b'\\'..=b'_') {
                // Fe sequence
                i += 1;
                continue;
            }
            // Unrecognized ESC sequence, skip the next byte
            i += 1;
            continue;
        }

        if clean && is_control_or_ansi(b) {
            // Skip control characters (but preserve tabs)
            i += 1;
            continue;
        }

        // Copy the byte
        out_buf[out_idx] = b;
        out_idx += 1;
        i += 1;
    }

    out_idx
}

/// Wrapper for reading and processing a source with optional cleaning and trimming
struct ProcessingReader<R: Read> {
    inner: R,
    clean: bool,
    trim: bool,
    process_buf: Vec<u8>,
    output_buf: Vec<u8>,
    pending: Option<Vec<u8>>,
}

impl<R: Read> ProcessingReader<R> {
    fn new(inner: R, clean: bool, trim: bool) -> Self {
        let buf_size = BUFFER_SIZE * 2; // Extra space for processing
        Self {
            inner,
            clean,
            trim,
            process_buf: vec![0u8; buf_size],
            output_buf: vec![0u8; buf_size],
            pending: None,
        }
    }
}
impl<R: Read> Read for ProcessingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // If we have pending processed data, return that first
        if let Some(ref mut pending) = self.pending {
            if !pending.is_empty() {
                let to_copy = pending.len().min(buf.len());
                buf[..to_copy].copy_from_slice(&pending[..to_copy]);
                pending.drain(..to_copy);
                if pending.is_empty() {
                    self.pending = None;
                }
                return Ok(to_copy);
            }
        }

        // Read from inner
        let n = self.inner.read(&mut self.process_buf)?;

        if n == 0 {
            return Ok(0);
        }

        // Process the buffer
        let processed = process_buffer(
            &self.process_buf[..n],
            &mut self.output_buf,
            self.clean,
            self.trim,
        );

        if processed == 0 {
            return Ok(0);
        }

        let to_copy = processed.min(buf.len());
        buf[..to_copy].copy_from_slice(&self.output_buf[..to_copy]);

        // Store overflow for next read
        if processed > to_copy {
            self.pending = Some(self.output_buf[to_copy..processed].to_vec());
        }

        Ok(to_copy)
    }
}


fn copy_stream<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    buffer: &mut [u8],
) -> io::Result<()> {
    loop {
        let count = reader.read(buffer)?;

        if count == 0 {
            break;
        }

        writer.write_all(&buffer[..count])?;
    }

    Ok(())
}

fn copy_file_to_writer(
    path: &Path,
    writer: &mut BufWriter<Box<dyn Write>>,
    buffer: &mut [u8],
    clean: bool,
    trim: bool,
) -> io::Result<()> {
    let file = File::open(path)?;
    let file_reader = BufReader::with_capacity(BUFFER_SIZE, file);

    if clean || trim {
        let mut processing_reader = ProcessingReader::new(file_reader, clean, trim);
        copy_stream(&mut processing_reader, writer, buffer)
    } else {
        let mut reader = file_reader;
        copy_stream(&mut reader, writer, buffer)
    }
}

fn copy_stdin_to_temp(temp_path: &Path, clean: bool, trim: bool) -> io::Result<()> {
    let stdin = io::stdin();
    let stdin_reader = BufReader::with_capacity(BUFFER_SIZE, stdin.lock());

    let temp_file = File::create(temp_path)?;
    let mut temp_writer = BufWriter::with_capacity(BUFFER_SIZE, temp_file);

    let mut buffer = [0u8; BUFFER_SIZE];

    if clean || trim {
        let mut processing_reader = ProcessingReader::new(stdin_reader, clean, trim);
        copy_stream(&mut processing_reader, &mut temp_writer, &mut buffer)?;
    } else {
        let mut reader = stdin_reader;
        copy_stream(&mut reader, &mut temp_writer, &mut buffer)?;
    }

    temp_writer.flush()
}

fn copy_temp_stdin(
    temp_path: &Path,
    writer: &mut BufWriter<Box<dyn Write>>,
    buffer: &mut [u8],
) -> io::Result<()> {
    let file = File::open(temp_path)?;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);
    copy_stream(&mut reader, writer, buffer)
}


fn timestamp_filename() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);

    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;


    let z = days + 719_468;
    let era = if z >= 0 {
        z
    } else {
        z - 146_096
    } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let day_of_year = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };

    year += if month <= 2 { 1 } else { 0 };

    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}.txt",
        year, month, day, hour, minute, second
    )
}

fn make_temp_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    env::temp_dir().join(format!(
        "rust-concat-{}-{}.tmp",
        process::id(),
        timestamp
    ))
}

fn main() -> io::Result<()> {
    let options = match parse_args() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!("Try --help for usage.");
            process::exit(2);
        }
    };

    if options.show_help {
        let program = env::args()
            .next()
            .unwrap_or_else(|| "concat".to_string());

        print_help(&program);
        return Ok(());
    }

    let stdin_is_available = !io::stdin().is_terminal();

    let mut positions = Vec::new();

    if stdin_is_available && !options.stdin_ignored {
        if options.stdin_positions.is_empty() {

            positions.push(0);
        } else {
            let file_count = options.files.len();

            for &position in &options.stdin_positions {
                let normalized = if position == usize::MAX || position > file_count {
                    file_count
                } else {
                    position
                };

                if !positions.contains(&normalized) {
                    positions.push(normalized);
                }
            }

            positions.sort_unstable();
        }
    }

    let stdin_temp_path = if !positions.is_empty() {
        let path = make_temp_path();

        if let Err(error) = copy_stdin_to_temp(&path, options.clean, options.trim) {
            let _ = fs::remove_file(&path);
            return Err(error);
        }

        Some(path)
    } else {
        None
    };

    let result = run_concat(&options, &positions, stdin_temp_path.as_deref());

    if let Some(path) = stdin_temp_path {
        let _ = fs::remove_file(path);
    }

    result
}

fn run_concat(
    options: &Options,
    stdin_positions: &[usize],
    stdin_temp_path: Option<&Path>,
) -> io::Result<()> {
    let output: Box<dyn Write> = match &options.output {
        None => Box::new(io::stdout().lock()),

        Some(path) if path.as_os_str().is_empty() => {
            let filename = timestamp_filename();
            Box::new(
                OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(filename)?,
            )
        }

        Some(path) => Box::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)?,
        ),
    };

    let mut writer = BufWriter::with_capacity(BUFFER_SIZE, output);
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut value_count = 0usize;

    let mut write_separator =
        |writer: &mut BufWriter<Box<dyn Write>>| -> io::Result<()> {
            if value_count > 0 {
                writer.write_all(&options.separator)?;
            }

            value_count += 1;
            Ok(())
        };

    for position in 0..=options.files.len() {
        if stdin_positions.contains(&position) {
            write_separator(&mut writer)?;

            if let Some(path) = stdin_temp_path {
                copy_temp_stdin(path, &mut writer, &mut buffer)?;
            }
        }

        if position < options.files.len() {
            write_separator(&mut writer)?;

            copy_file_to_writer(
                Path::new(&options.files[position]),
                &mut writer,
                &mut buffer,
                options.clean,
                options.trim,
            )?;
        }
    }

    writer.flush()
}
