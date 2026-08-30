use clap::{error::ErrorKind, Parser};
use local_agent_adapter::{run_cli, BridgeError, Cli, ExitCode};

fn main() {
    let code = match Cli::try_parse() {
        Ok(cli) => run_cli(cli) as i32,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let code = error.exit_code();
            let _ = error.print();
            code
        }
        Err(_) => {
            let error = BridgeError::invalid(
                "The command line arguments are invalid; run infinite-canvas --help.",
            );
            let encoded = serde_json::to_string(&error.envelope()).unwrap_or_else(|_| {
                "{\"ok\":false,\"error\":{\"code\":\"INTERNAL\",\"message\":\"JSON encoding failed\"}}"
                    .to_owned()
            });
            println!("{encoded}");
            ExitCode::Usage as i32
        }
    };
    std::process::exit(code);
}
