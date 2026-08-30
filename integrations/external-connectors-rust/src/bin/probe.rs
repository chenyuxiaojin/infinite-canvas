use infinite_canvas_external_connectors::{
    probe_all, DaVinciProvider, EagleProvider, ProviderReport, SystemDaVinciRuntime,
    SystemEagleRuntime,
};

fn main() {
    let reports = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => probe_all(),
        [provider] if provider == "all" => probe_all(),
        [provider] if provider == "eagle" => {
            vec![EagleProvider::new(SystemEagleRuntime).probe()]
        }
        [provider] if provider == "davinci" => {
            vec![DaVinciProvider::new(SystemDaVinciRuntime).probe()]
        }
        _ => {
            eprintln!("usage: external-connectors-probe [all|eagle|davinci]");
            std::process::exit(2);
        }
    };

    print_reports(&reports);
}

fn print_reports(reports: &[ProviderReport]) {
    match serde_json::to_string_pretty(reports) {
        Ok(json) => println!("{json}"),
        Err(_) => {
            eprintln!("failed to serialize provider reports");
            std::process::exit(1);
        }
    }
}
