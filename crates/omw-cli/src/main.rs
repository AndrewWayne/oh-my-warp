//! `omw` binary — thin wrapper around `omw_cli::run`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = omw_cli::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(code);
}
