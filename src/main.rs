use jetgame::app::RunOptions;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut options = RunOptions {
        network: jetgame::networking::NetworkConfig::load_default()?,
        ..Default::default()
    };
    let mut mesh_override = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke-test" => options.smoke_test = true,
            "--preview" => options.preview = true,
            "--offline" => options.offline = true,
            "--directory" => {
                options.network.directory =
                    Some(args.next().ok_or("--directory requires HOST:PORT")?)
            }
            "--server-python" => {
                options.network.python = Some(
                    args.next()
                        .ok_or("--server-python requires an interpreter path")?
                        .into(),
                )
            }
            "--host-port" => {
                options.network.host_port =
                    args.next().ok_or("--host-port requires a port")?.parse()?
            }
            "--public-port" => {
                options.network.public_port = Some(
                    args.next()
                        .ok_or("--public-port requires a port")?
                        .parse()?,
                )
            }
            "--connect" => {
                options.connect = Some(args.next().ok_or("--connect requires HOST:PORT")?)
            }
            "--mesh" => {
                options.mesh = args.next().ok_or("--mesh requires a file path")?.into();
                mesh_override = true;
            }
            "--help" | "-h" => {
                println!(
                    "JetGame [--preview | --offline] [--mesh FILE.glb] [--smoke-test]\n  [--directory HOST:PORT] [--connect HOST:PORT]\n  [--server-python PATH] [--host-port PORT] [--public-port PORT]"
                );
                return Ok(());
            }
            _ => return Err(format!("Unknown option: {arg}").into()),
        }
    }
    if options.connect.is_some() && (options.offline || options.preview) {
        return Err("--connect cannot be combined with --offline or --preview".into());
    }
    // Packaged resources are located beside the executable, regardless of its launch directory.
    if options.network.root.join("assets").is_dir() {
        if mesh_override && options.mesh.is_relative() {
            options.mesh = std::env::current_dir()?.join(&options.mesh);
        }
        std::env::set_current_dir(&options.network.root)?;
    }
    jetgame::app::run(options)
}
