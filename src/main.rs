use chrono::Utc;
use std::env;
use std::fs::OpenOptions;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::process;

fn main() {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    let mut sep_template = String::new();
    let mut files: Vec<String> = Vec::new();

    // managed indexed loop for parsing
    let mut i = 0;
    while i < raw_args.len() {
        let a = &raw_args[i];
        if a.starts_with("--sep=") {
            sep_template = a["--sep=".len()..].to_string();
            i += 1;
        } else if a == "--sep" {
            i += 1;
            if i < raw_args.len() {
                sep_template = raw_args[i].clone();
                i += 1;
            } else {
                eprintln!("Missing value after --sep");
            }
        } else {
            files.push(a.clone());
            i += 1;
        }
    }

    if files.is_empty() {
        eprintln!(
            "Usage: concat [\"--sep=SEPARATOR\" | \"--sep\" \"SEPARATOR\"] <file1> <file2> ..."
        );
        eprintln!(
            "include any text, as well as ####?#### - replace ? with r for \\r (same for n and t), file,name,ext for full path, just filename, or extension. index,total - ordinal in list of files."
        );
        eprintln!("examples:");
        eprintln!(
            "... \"--sep=####r########n########r########n####;####file########r########n####\""
        );
        eprintln!(
            "... \"--sep=####r########n########r########n####;####index####/####total########r########n####\""
        );
        process::exit(2);
    }

    // validate
    let mut valid: Vec<String> = Vec::new();
    for f in files {
        let p = Path::new(&f);
        match p.metadata() {
            Ok(md) if md.is_file() => valid.push(f),
            Ok(_) => eprintln!("Skipping (not a file): {}", p.display()),
            Err(e) => eprintln!("Skipping (error accessing): {}: {}", p.display(), e),
        }
    }
    if valid.is_empty() {
        eprintln!("No valid input files.");
        process::exit(3);
    }

    let total = valid.len();
    let out_name = format!("{}.txt", Utc::now().format("%Y%m%d%H%M%S"));
    let out_file = match OpenOptions::new()
        .create(true)
        .write(true)
        .append(true)
        .open(&out_name)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error opening '{}': {}", out_name, e);
            process::exit(4);
        }
    };
    let mut writer = BufWriter::with_capacity(16 * 1024, out_file);
    let mut had_error = false;

    for (idx, path_str) in valid.iter().enumerate() {
        let index = idx + 1;
        let path = Path::new(path_str);
        let filename_only = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path_str);
        let extension_only = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or(path_str);

        if !sep_template.is_empty() {
            let sep = sep_template
                .replace("####r####", "\r")
                .replace("####n####", "\n")
                .replace("####t####", "\t")
                .replace("####file####", path_str)
                .replace("####name####", filename_only)
                .replace("####ext####", extension_only)
                .replace("####index####", &index.to_string())
                .replace("####total####", &total.to_string());
            if let Err(e) = writer.write_all(sep.as_bytes()) {
                eprintln!("Error writing sep: {}", e);
                had_error = true;
            }
            if let Err(e) = writer.flush() {
                eprintln!("Error flushing sep: {}", e);
                had_error = true;
            }
        }

        match OpenOptions::new().read(true).open(path) {
            Ok(f) => {
                let mut r = BufReader::with_capacity(8 * 1024, f);
                let mut buf = [0u8; 8 * 1024];
                let mut copied: u64 = 0;
                loop {
                    match r.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Err(e) = writer.write_all(&buf[..n]) {
                                eprintln!("Write error '{}': {}", path.display(), e);
                                had_error = true;
                                break;
                            }
                            copied += n as u64;
                        }
                        Err(e) => {
                            eprintln!("Read error '{}': {}", path.display(), e);
                            had_error = true;
                            break;
                        }
                    }
                }
                if let Err(e) = writer.flush() {
                    eprintln!("Flush error after '{}': {}", path.display(), e);
                    had_error = true;
                }
                eprintln!("Appended '{}' ({} bytes).", path.display(), copied);
            }
            Err(e) => {
                eprintln!("Error opening '{}': {}", path.display(), e);
                had_error = true;
            }
        }
    }

    if let Err(e) = writer.flush() {
        eprintln!("Final flush error '{}': {}", out_name, e);
        process::exit(5);
    }
    drop(writer);

    if had_error {
        eprintln!("Completed with some errors. Output: {}", out_name);
        process::exit(6);
    }

    println!("Wrote concatenated output to {}", out_name);
}
