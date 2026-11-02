fn main() {
    if let Err(err) = tandem::cli::run() {
        if let Some(tandem::cli::CliError::NotImplemented) =
            err.downcast_ref::<tandem::cli::CliError>()
        {
            eprintln!("{err}");
            std::process::exit(70);
        }
        eprintln!("{err}");
        std::process::exit(1);
    }
}
