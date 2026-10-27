use jetgame::app::RunOptions;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut options = RunOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke-test" => options.smoke_test = true,
            "--preview" => options.preview = true,
            "--mesh" => options.mesh = args.next().ok_or("--mesh requires a file path")?.into(),
            "--help" | "-h" => {
                println!("JetGame [--mesh FILE.glb] [--preview] [--smoke-test]");
                return Ok(());
            }
            _ => return Err(format!("Unknown option: {arg}").into()),
        }
    }
    jetgame::app::run(options)
}
