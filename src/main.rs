//! The command-line entry point: options, script mode, interactive mode, server mode.
//!
//! ```text
//! ./myTinySql --sqlfile=examples/select.sql   run a script
//! ./myTinySql                                 type statements (or pipe them in)
//! ./myTinySql --http                          browser page on http://localhost:8080
//! ```

use std::fs::File;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use mytinysql::output::render_text;
use mytinysql::server::{ServerConfig, run_server};
use mytinysql::session::Session;
use mytinysql::util::text_of;

/// The text printed by `--help` and after a wrong option.
const USAGE: &str = "Usage: myTinySql [options]

  --sqlfile=FILE   run the statements in FILE ('-' for standard input)
  --http           start the browser server (default port 8080)
  --port=N         listen on port N instead of 8080
  --allow=LIST     comma-separated client IP prefixes the server accepts,
                   e.g. 127.,192.168. (default: everyone)
  --data=DIR       directory with SYSTEM_TABLE and the *_DATA files (default: data)
  --web=DIR        directory with the browser page's files (default: web)
  --help           show this help

With no --sqlfile and no --http, statements are read from standard input.
";

/// The command-line options, after parsing.
#[derive(Debug)]
struct Options {
    /// `--sqlfile`: the script to run (`-` for standard input).
    sql_file: Option<String>,
    /// `--http`: start the browser server.
    http: bool,
    /// `--port`: the server port, if not the default.
    port: Option<u16>,
    /// `--allow`: client IP prefixes, if given.
    allow: Option<Vec<String>>,
    /// `--data`: the directory holding the tables.
    data_dir: PathBuf,
    /// `--web`: the directory holding the browser page.
    web_dir: PathBuf,
}

/// Splits a comma-separated list such as "127.,192.168." into its non-empty items.
fn split_list(text: &str) -> Vec<String> {
    text.split(',').filter(|item| !item.is_empty()).map(String::from).collect()
}

/// Parses the command line (without the program name). Options take their value as `--name=value`
/// or `--name value`.
///
/// Fails, with the message to show above the usage, on an unknown option, a missing or bad value,
/// or `--sqlfile` together with `--http`. (C++ throws `std::invalid_argument` for these.)
fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        sql_file: None,
        http: false,
        port: None,
        allow: None,
        data_dir: PathBuf::from("data"),
        web_dir: PathBuf::from("web"),
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (name, inline_value) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (arg.as_str(), None),
        };
        // A closure that takes the value: after the `=`, or else the next argument.
        let mut require_value = || -> Result<String, String> {
            match inline_value {
                Some(value) => Ok(value.to_string()),
                None => args.next().cloned().ok_or_else(|| format!("{name} needs a value")),
            }
        };

        match name {
            "--help" | "-h" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--sqlfile" => options.sql_file = Some(require_value()?),
            "--http" => options.http = true,
            "--port" => {
                let text = require_value()?;
                // Only digits: `str::parse` would also accept a leading `+`, which C++ `from_chars` refuses.
                let port = text.parse::<u16>().ok().filter(|&p| p != 0 && text.bytes().all(|c| c.is_ascii_digit()));
                options.port = Some(port.ok_or_else(|| format!("invalid port '{text}'"))?);
            }
            "--allow" => options.allow = Some(split_list(&require_value()?)),
            "--data" => options.data_dir = PathBuf::from(require_value()?),
            "--web" => options.web_dir = PathBuf::from(require_value()?),
            _ => return Err(format!("unknown option '{arg}'")),
        }
    }
    if options.sql_file.is_some() && options.http {
        return Err("choose either --sqlfile or --http, not both".to_string());
    }
    Ok(options)
}

/// Prints text on standard output. An error (e.g. the output was piped into `head`, which has
/// exited) is ignored, where `print!` would stop the program.
fn print_out(text: &str) {
    let _ = io::stdout().write_all(text.as_bytes());
}

/// Runs a whole script read from `input` (a file or standard input) and prints every result.
///
/// The exit code is 0 if every statement succeeded, 1 otherwise. `&mut dyn Read` takes any reader,
/// as `std::istream&` does in C++.
fn run_script(session: &mut Session, input: &mut dyn Read) -> ExitCode {
    let mut bytes = Vec::new();
    if let Err(e) = input.read_to_end(&mut bytes) {
        eprintln!("myTinySql: {e}");
        return ExitCode::FAILURE;
    }
    let mut exit_code = ExitCode::SUCCESS;
    for result in session.execute_script(&text_of(&bytes)) {
        print_out(&render_text(&result));
        if result.failed {
            exit_code = ExitCode::FAILURE;
        }
    }
    exit_code
}

/// Reads statements at a `tinysql>` prompt until end of input (Ctrl+D). The exit code is always 0.
fn run_interactive(session: &mut Session) -> ExitCode {
    let mut input = io::stdin().lock();
    let mut line = Vec::new();
    loop {
        print_out("tinysql> ");
        let _ = io::stdout().flush();
        line.clear();
        match input.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                for result in session.execute_script(&text_of(&line)) {
                    print_out(&render_text(&result));
                }
            }
        }
    }
    print_out("\n");
    ExitCode::SUCCESS
}

/// Program entry point.
///
/// The exit code is 0 on success; 1 if a script statement failed or an error occurred; 2 for a usage
/// error or a missing file or data directory.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse_options(&args) {
        Ok(options) => options,
        Err(message) => {
            eprint!("myTinySql: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if !options.data_dir.is_dir() {
        eprintln!("data directory '{}' not found (use --data=DIR)", options.data_dir.display());
        return ExitCode::from(2);
    }

    if options.http {
        let index = options.web_dir.join("index.html");
        if !index.is_file() {
            eprintln!("web page '{}' not found (use --web=DIR)", index.display());
            return ExitCode::from(2);
        }
        let config = ServerConfig {
            port: options.port.unwrap_or(8080),
            data_dir: options.data_dir,
            web_dir: options.web_dir,
            allowed_clients: options.allow.unwrap_or_default(),
        };
        return match run_server(config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("myTinySql: {e}");
                ExitCode::FAILURE
            }
        };
    }

    let mut session = Session::new(options.data_dir);
    match options.sql_file.as_deref() {
        None if io::stdin().is_terminal() => run_interactive(&mut session),
        None | Some("-") => run_script(&mut session, &mut io::stdin()),
        Some(path) => match File::open(path) {
            Ok(mut file) => run_script(&mut session, &mut file),
            Err(_) => {
                eprintln!("cannot open '{path}'");
                ExitCode::from(2)
            }
        },
    }
}
