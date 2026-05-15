mod cli;
mod error;

fn main() {
    if let Err(error) = cli::run() {
        match error {
            error::CliError::Usage => {
                eprintln!("usage: engine-core analyze|emit-go");
            }
        }
        std::process::exit(error.exit_code());
    }
}
