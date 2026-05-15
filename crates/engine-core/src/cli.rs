use std::io::{self, Read};

use engine_core::{analyze, emit_go, AnalyzeRequest, EmitGoRequest};

use crate::error::CliError;

pub fn run() -> Result<(), CliError> {
    let mut args = std::env::args();
    let _bin = args.next();

    match args.next().as_deref() {
        Some("analyze") => {
            let mut input = String::new();
            io::stdin()
                .read_to_string(&mut input)
                .expect("failed to read stdin");

            let request: AnalyzeRequest =
                serde_json::from_str(&input).expect("failed to parse AnalyzeRequest JSON");
            let response = analyze(request);
            let output =
                serde_json::to_string_pretty(&response).expect("failed to encode response");
            println!("{output}");
            Ok(())
        }
        Some("emit-go") => {
            let mut input = String::new();
            io::stdin()
                .read_to_string(&mut input)
                .expect("failed to read stdin");

            let request: EmitGoRequest =
                serde_json::from_str(&input).expect("failed to parse EmitGoRequest JSON");
            let response = emit_go(request);
            let output =
                serde_json::to_string_pretty(&response).expect("failed to encode response");
            println!("{output}");
            Ok(())
        }
        _ => Err(CliError::Usage),
    }
}
