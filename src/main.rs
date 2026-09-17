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

  {program} --out --sep=EOL a.txt b.txt
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
    let mut separator_was_set = false;

    let args: Vec<String> = env::args().skip(1).collect();

    for arg in args {
        if arg == "-h" || arg == "--help" {
            show_help = true;
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
) -> io::Result<()> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);
    copy_stream(&mut reader, writer, buffer)
}

fn copy_stdin_to_temp(temp_path: &Path) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin = BufReader::with_capacity(BUFFER_SIZE, stdin.lock());

    let temp_file = File::create(temp_path)?;
    let mut temp_writer = BufWriter::with_capacity(BUFFER_SIZE, temp_file);

    let mut buffer = [0u8; BUFFER_SIZE];
    copy_stream(&mut stdin, &mut temp_writer, &mut buffer)?;
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

    // Civil-date conversion based on Howard Hinnant's public-domain algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
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
            // Default behavior: stdin first.
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

        if let Err(error) = copy_stdin_to_temp(&path) {
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

    let mut write_separator = |writer: &mut BufWriter<Box<dyn Write>>| -> io::Result<()> {
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
            )?;
        }
    }

    writer.flush()
}
