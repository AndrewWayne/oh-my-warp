//! `omw` binary — thin wrapper around `omw_cli::run`.
//!
//! Process-stdio handling for `--from-stdin` lives here, not in the library.
//! `omw_cli::run` operates only on its explicit `stdout`/`stderr` sinks so it
//! stays safe to call from in-process tests.

use std::io::{BufRead, BufReader, Write};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // If the user passed `--from-stdin`, consume one line from the real
    // process stdin here and rewrite argv to `--key <line>` before handing it
    // to the library. The library never sees `--from-stdin`.
    if args.iter().any(|a| a == "--from-stdin") {
        let mut buf = String::new();
        let stdin = std::io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        if let Err(e) = reader.read_line(&mut buf) {
            let _ = writeln!(std::io::stderr(), "error: reading key from stdin: {e}");
            std::process::exit(1);
        }
        let line = buf.trim_end_matches(['\n', '\r']).to_string();

        let idx = args
            .iter()
            .position(|a| a == "--from-stdin")
            .expect("checked above");
        // Replace `--from-stdin` with `--key <line>` in place.
        args.splice(idx..=idx, ["--key".to_string(), line]);
    }

    let code = omw_cli::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(code);
}
