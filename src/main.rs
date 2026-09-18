use is_terminal::IsTerminal;
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

/// Version of this program
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Buffer size for I/O operations (64 KB)
const BUFFER_SIZE: usize = 64 * 1024;

/// Threshold for displaying file sizes in different units (1 MB)
const SIZE_THRESHOLD_MB: u64 = 1024 * 1024;

/// Threshold for displaying file sizes in different units (1 GB)
const SIZE_THRESHOLD_GB: u64 = 1024 * 1024 * 1024;

// ============================================================================
// OPTION PARSING STRUCTURES
// ============================================================================

/// Configuration options parsed from command-line arguments
#[derive(Debug)]
struct Options {
    /// Input file paths
    files: Vec<String>,
    /// Bytes to insert between each input value
    separator: Vec<u8>,
    /// Output file path (None = stdout)
    output: Option<PathBuf>,
    /// Positions where stdin should be inserted
    stdin_positions: Vec<usize>,
    /// Whether to ignore stdin entirely
    stdin_ignored: bool,
    /// Whether to display help message
    show_help: bool,
    /// Whether to remove control and ANSI escape sequences
    clean: bool,
    /// Whether to trim leading/trailing whitespace
    trim: bool,
    /// Whether to show verbose output with filenames and sizes
    verbose: bool,
}

// ============================================================================
// HELP AND VERSION OUTPUT
// ============================================================================

/// Prints the help message with usage information and examples
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

  --verbose
      Show filenames and human-readable file sizes during concatenation.

  --version
      Show version information and exit.

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

  {program} --verbose a.txt b.txt
"#
    );
}

/// Prints the version information and exits
fn print_version() {
    println!("concat {}", VERSION);
}

// ============================================================================
// ARGUMENT PARSING
// ============================================================================

/// Parses command-line arguments into an `Options` structure
///
/// # Returns
///
/// Returns `Ok(Options)` on successful parsing, or `Err(String)` with an
/// error message if an unknown option or invalid value is encountered.
fn parse_args() -> Result<Options, String> {
    let mut files = Vec::new();
    let mut separator = Vec::new();
    let mut output = None;
    let mut stdin_positions = Vec::new();
    let mut stdin_ignored = false;
    let mut show_help = false;
    let mut clean = false;
    let mut trim = false;
    let mut verbose = false;
    let mut separator_was_set = false;

    let args: Vec<String> = env::args().skip(1).collect();

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                show_help = true;
            }
            "--version" => {
                print_version();
                process::exit(0);
            }
            "--clean" => {
                clean = true;
            }
            "--trim" => {
                trim = true;
            }
            "--verbose" => {
                verbose = true;
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--sep=") {
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
                            let position = number.parse::<usize>().map_err(|_| {
                                format!("invalid --push-stdin-at value: {value}")
                            })?;

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
        verbose,
    })
}

/// Parses a separator string, handling special values and escape sequences
///
/// # Arguments
///
/// * `value` - The separator string to parse
///
/// # Special Values
///
/// - `"EOL"` - Converts to the system-specific line ending (`\r\n` on Windows, `\n` elsewhere)
///
/// # Escape Sequences
///
/// Recognizes the following escape sequences:
/// - `\\n` - Newline (LF)
/// - `\\r` - Carriage return (CR)
/// - `\\t` - Tab
/// - `\\\\` - Backslash
/// - `\\0` - Null byte
///
/// # Returns
///
/// Returns `Ok(Vec<u8>)` on successful parsing, or `Err(String)` if an invalid
/// value is provided.
fn parse_separator(value: &str) -> Result<Vec<u8>, String> {
    // Handle the special "EOL" value
    if value == "EOL" {
        return Ok(if cfg!(windows) {
            b"\r\n".to_vec()
        } else {
            b"\n".to_vec()
        });
    }

    // Allow common escaped byte values while preserving all other bytes
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

// ============================================================================
// BYTE CLASSIFICATION UTILITIES
// ============================================================================

/// Checks if a byte is any kind of whitespace (space, tab, newline, etc.)
///
/// Includes: space, tab, line feed (LF), carriage return (CR), vertical tab, form feed
#[inline]
fn is_whitespace(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C
    )
}

/// Checks if a byte is a control character that should be removed with `--clean`
///
/// Removes most control characters (0x00-0x1F) except tab, plus the DEL character (0x7F).
/// Does not remove ANSI escape sequences, which are handled separately.
#[inline]
fn is_control_or_ansi(b: u8) -> bool {
    b < 0x20 && b != b'\t' || b == 0x7F
}

// ============================================================================
// BUFFER PROCESSING
// ============================================================================

/// Processes a buffer for cleaning and/or trimming
///
/// When `--clean` is enabled, removes control characters and ANSI escape sequences.
/// When `--trim` is enabled, removes leading and trailing whitespace.
///
/// # Arguments
///
/// * `buffer` - Input buffer to process
/// * `out_buf` - Output buffer where processed bytes are written
/// * `clean` - Whether to remove control and ANSI characters
/// * `trim` - Whether to remove leading/trailing whitespace
///
/// # Returns
///
/// The number of bytes written to `out_buf`
fn process_buffer(
    buffer: &[u8],
    out_buf: &mut [u8],
    clean: bool,
    trim: bool,
) -> usize {
    // Fast path: no processing needed
    if !clean && !trim {
        return 0;
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

        // Handle ANSI escape sequences when cleaning
        if clean && b == 0x1B {
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

        // Skip control characters when cleaning (but preserve tabs)
        if clean && is_control_or_ansi(b) {
            i += 1;
            continue;
        }

        // Copy the byte to output
        out_buf[out_idx] = b;
        out_idx += 1;
        i += 1;
    }

    out_idx
}

// ============================================================================
// PROCESSING READER
// ============================================================================

/// A wrapper around any `Read` source that applies cleaning and/or trimming
///
/// Buffers data internally to allow processing without external knowledge
/// of buffer sizes.
struct ProcessingReader<R: Read> {
    /// The underlying reader
    inner: R,
    /// Whether to remove control and ANSI characters
    clean: bool,
    /// Whether to remove leading/trailing whitespace
    trim: bool,
    /// Temporary buffer for raw input
    process_buf: Vec<u8>,
    /// Temporary buffer for processed output
    output_buf: Vec<u8>,
    /// Any processed data not yet consumed by the caller
    pending: Option<Vec<u8>>,
}

impl<R: Read> ProcessingReader<R> {
    /// Creates a new `ProcessingReader`
    ///
    /// # Arguments
    ///
    /// * `inner` - The underlying reader
    /// * `clean` - Whether to remove control and ANSI characters
    /// * `trim` - Whether to remove leading/trailing whitespace
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

        // Read from inner source
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

        // Copy what we can to the output buffer
        let to_copy = processed.min(buf.len());
        buf[..to_copy].copy_from_slice(&self.output_buf[..to_copy]);

        // Store any overflow for the next read
        if processed > to_copy {
            self.pending = Some(self.output_buf[to_copy..processed].to_vec());
        }

        Ok(to_copy)
    }
}

// ============================================================================
// STREAM COPYING UTILITIES
// ============================================================================

/// Copies data from a reader to a writer using the provided buffer
///
/// # Arguments
///
/// * `reader` - The source reader
/// * `writer` - The destination writer
/// * `buffer` - The buffer to use for copying
///
/// # Errors
///
/// Returns an I/O error if reading or writing fails.
fn copy_stream<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    buffer: &mut [u8],
) -> io::Result<()> {
    loop {
        let n = reader.read(buffer)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n])?;
    }
    Ok(())
}

/// Copies a file to a writer with optional cleaning and trimming
///
/// # Arguments
///
/// * `path` - Path to the file to copy
/// * `writer` - The destination writer
/// * `buffer` - The buffer to use for copying
/// * `clean` - Whether to remove control and ANSI characters
/// * `trim` - Whether to remove leading/trailing whitespace
/// * `verbose` - Whether to print the filename and size
///
/// # Errors
///
/// Returns an I/O error if the file cannot be opened or read.
fn copy_file_to_writer<W: Write>(
    path: &Path,
    writer: &mut BufWriter<W>,
    buffer: &mut [u8],
    clean: bool,
    trim: bool,
    verbose: bool,
) -> io::Result<()> {
    let file = File::open(path)?;

    // Display verbose output if requested
    if verbose {
        if let Ok(metadata) = file.metadata() {
            let size = metadata.len();
            let size_str = format_file_size(size);
            let filename = path.display();
            eprintln!("[{}] {}", filename, size_str);
        }
    }

    let reader = BufReader::new(file);

    if clean || trim {
        let mut processing_reader = ProcessingReader::new(reader, clean, trim);
        copy_stream(&mut processing_reader, writer, buffer)?;
    } else {
        let mut reader = BufReader::new(File::open(path)?);
        copy_stream(&mut reader, writer, buffer)?;
    }

    Ok(())
}

/// Copies stdin to a temporary file for later processing
///
/// This is necessary because stdin can only be read once.
///
/// # Arguments
///
/// * `temp_path` - Path where the temporary file should be created
/// * `clean` - Whether to remove control and ANSI characters
/// * `trim` - Whether to remove leading/trailing whitespace
/// * `verbose` - Whether to print verbose output
///
/// # Errors
///
/// Returns an I/O error if the temporary file cannot be created or written.
fn copy_stdin_to_temp(
    temp_path: &Path,
    clean: bool,
    trim: bool,
    verbose: bool,
) -> io::Result<()> {
    let stdin = io::stdin();
    let mut buffer = vec![0u8; BUFFER_SIZE];

    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(temp_path)?;

    let mut writer = BufWriter::with_capacity(BUFFER_SIZE, file);

    if clean || trim {
        let stdin_lock = stdin.lock();
        let mut processing_reader = ProcessingReader::new(stdin_lock, clean, trim);
        copy_stream(&mut processing_reader, &mut writer, &mut buffer)?;
    } else {
        let mut stdin_lock = stdin.lock();
        copy_stream(&mut stdin_lock, &mut writer, &mut buffer)?;
    }

    writer.flush()?;

    if verbose {
        if let Ok(metadata) = fs::metadata(temp_path) {
            let size = metadata.len();
            let size_str = format_file_size(size);
            eprintln!("[stdin] {}", size_str);
        }
    }

    Ok(())
}

/// Copies a temporary file (containing stdin data) to a writer
///
/// # Arguments
///
/// * `temp_path` - Path to the temporary file
/// * `writer` - The destination writer
/// * `buffer` - The buffer to use for copying
///
/// # Errors
///
/// Returns an I/O error if the file cannot be opened or read.
fn copy_temp_stdin(
    temp_path: &Path,
    writer: &mut BufWriter<Box<dyn Write>>,
    buffer: &mut [u8],
) -> io::Result<()> {
    let file = BufReader::new(File::open(temp_path)?);
    let mut reader = file;
    copy_stream(&mut reader, writer, buffer)?;
    Ok(())
}

/// Formats a file size as a human-readable string
///
/// # Arguments
///
/// * `size` - The size in bytes
///
/// # Returns
///
/// A formatted string such as "1.5 MB" or "256 bytes"
fn format_file_size(size: u64) -> String {
    if size >= SIZE_THRESHOLD_GB {
        format!("{:.1} GB", size as f64 / SIZE_THRESHOLD_GB as f64)
    } else if size >= SIZE_THRESHOLD_MB {
        format!("{:.1} MB", size as f64 / SIZE_THRESHOLD_MB as f64)
    } else if size >= 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else if size == 1 {
        "1 byte".to_string()
    } else {
        format!("{} bytes", size)
    }
}

// ============================================================================
// TEMPORARY FILE MANAGEMENT
// ============================================================================

/// Generates a timestamp-based filename in the format `yyyymmddhhmmss.txt`
///
/// Uses the system time to create a unique filename based on the current
/// date and time.
///
/// # Returns
///
/// A `PathBuf` containing the generated filename
fn timestamp_filename() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before Unix epoch");

    let total_secs = now.as_secs();
    let days_since_epoch = total_secs / 86400;

    // Convert days since epoch to year, month, day
    let mut year = 1970;
    let mut day_of_year = days_since_epoch;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if day_of_year < days_in_year {
            break;
        }
        day_of_year -= days_in_year;
        year += 1;
    }

    let (month, day) = day_of_year_to_month_day(year, day_of_year as u32);

    // Convert seconds within day to hour, minute, second
    let secs_in_day = total_secs % 86400;
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;

    let filename = format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}.txt",
        year, month, day, hour, minute, second
    );

    PathBuf::from(filename)
}

/// Checks if a year is a leap year
#[inline]
fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Converts a day-of-year to month and day-of-month
///
/// # Arguments
///
/// * `year` - The year
/// * `day_of_year` - The day of the year (0-based)
///
/// # Returns
///
/// A tuple of `(month, day)` (1-based)
fn day_of_year_to_month_day(year: u64, day_of_year: u32) -> (u32, u32) {
    let is_leap = is_leap_year(year);
    let days_in_months = if is_leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut remaining_days = day_of_year;
    for (month_idx, &days_in_month) in days_in_months.iter().enumerate() {
        if remaining_days < days_in_month as u32 {
            return ((month_idx + 1) as u32, remaining_days + 1);
        }
        remaining_days -= days_in_month as u32;
    }

    (12, 31) // Fallback for edge cases
}

/// Creates a temporary file path in the system temp directory
///
/// # Returns
///
/// A `PathBuf` pointing to a unique temporary file
fn make_temp_path() -> io::Result<PathBuf> {
    let temp_dir = env::temp_dir();
    let filename = format!("concat_stdin_{}.tmp", std::process::id());
    Ok(temp_dir.join(filename))
}

// ============================================================================
// MAIN CONCATENATION LOGIC
// ============================================================================

/// Performs the actual file concatenation
///
/// This function orchestrates the concatenation of files and stdin based on
/// the provided options. It handles:
/// - Opening the output file or using stdout
/// - Inserting stdin at the correct positions
/// - Applying cleaning and trimming as needed
/// - Writing separators between inputs
///
/// # Arguments
///
/// * `options` - The parsed command-line options
/// * `stdin_positions` - The sorted and deduplicated positions for stdin
/// * `stdin_temp_path` - The path to the temporary file containing stdin (if applicable)
///
/// # Errors
///
/// Returns an I/O error if any file operation fails.
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

    // Helper closure to write separators between inputs
    let mut write_separator =
        |writer: &mut BufWriter<Box<dyn Write>>| -> io::Result<()> {
            if value_count > 0 {
                writer.write_all(&options.separator)?;
            }

            value_count += 1;
            Ok(())
        };

    // Iterate through positions and insert files and stdin in the correct order
    for position in 0..=options.files.len() {
        // Insert stdin at the current position if specified
        if stdin_positions.contains(&position) {
            write_separator(&mut writer)?;

            if let Some(path) = stdin_temp_path {
                copy_temp_stdin(path, &mut writer, &mut buffer)?;
            }
        }

        // Insert the file at the current position
        if position < options.files.len() {
            write_separator(&mut writer)?;

            copy_file_to_writer(
                Path::new(&options.files[position]),
                &mut writer,
                &mut buffer,
                options.clean,
                options.trim,
                options.verbose,
            )?;
        }
    }

    writer.flush()
}

// ============================================================================
// PROGRAM ENTRY POINT
// ============================================================================

/// The main entry point for the concatenation program
///
/// Parses arguments, determines if stdin is present, manages temporary files,
/// and orchestrates the concatenation process.
fn main() {
    let mut options = match parse_args() {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    };

    if options.show_help {
        print_help(&env::args().next().unwrap_or_else(|| "concat".to_string()));
        return;
    }

    // Determine if stdin is interactive
    let stdin_is_interactive = io::stdin().is_terminal();

    // Default to inserting stdin at position 0 if it's non-interactive
    // and no explicit position was specified
    if !stdin_is_interactive && !options.stdin_ignored && options.stdin_positions.is_empty() {
        options.stdin_positions.push(0);
    }

    // Sort and deduplicate stdin positions, cap at files.len() + 1
    options.stdin_positions.sort_unstable();
    options.stdin_positions.dedup();
    options.stdin_positions
        .iter_mut()
        .for_each(|pos| *pos = (*pos).min(options.files.len()));

    // Handle stdin
    let temp_path = if !options.stdin_ignored && !options.stdin_positions.is_empty() {
        match make_temp_path() {
            Ok(path) => {
                if let Err(e) = copy_stdin_to_temp(&path, options.clean, options.trim, options.verbose) {
                    eprintln!("Error reading stdin: {}", e);
                    process::exit(1);
                }
                Some(path)
            }
            Err(e) => {
                eprintln!("Error creating temporary file: {}", e);
                process::exit(1);
            }
        }
    } else {
        None
    };

    // Perform concatenation
    if let Err(e) = run_concat(&options, &options.stdin_positions, temp_path.as_deref()) {
        eprintln!("Error during concatenation: {}", e);
        process::exit(1);
    }

    // Clean up temporary file
    if let Some(path) = temp_path {
        let _ = fs::remove_file(path);
    }
}
