#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use jetgame::{app::RunOptions, config::GameConfig, networking::NetworkConfig};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(debug_assertions)]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut options = RunOptions {
        network: NetworkConfig::load_default()?,
        ..Default::default()
    };
    let mut mesh_override: Option<PathBuf> = None;
    let mut game_config: Option<PathBuf> = None;
    let mut public_addresses = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke-test" => options.smoke_test = true,
            "--preview" => options.preview = true,
            "--python" => {
                options.network.python = Some(
                    args.next()
                        .ok_or("--python requires an interpreter path")?
                        .into(),
                );
            }
            "--host-port" => {
                let port: u16 = args.next().ok_or("--host-port requires a port")?.parse()?;
                options.network.set("host_port", port);
            }
            "--public-address" => public_addresses.push(
                args.next()
                    .ok_or("--public-address requires HOST:PORT or [IPv6]:PORT")?,
            ),
            "--connect" => {
                options.connect = Some(
                    args.next()
                        .ok_or("--connect requires an invitation or HOST:PORT")?,
                )
            }
            "--mesh" => {
                mesh_override = Some(args.next().ok_or("--mesh requires a file path")?.into())
            }
            "--config" => {
                game_config = Some(args.next().ok_or("--config requires a JSON file")?.into())
            }
            "--help" | "-h" => {
                println!(
                    "JetGame [--preview] [--config game.json] [--mesh FILE.glb] [--smoke-test]\n  [--connect INVITATION_OR_HOST:PORT] [--python PATH] [--host-port PORT]\n  [--public-address HOST:PORT]\nOnline play is the default. --preview starts local flight without networking."
                );
                return Ok(());
            }
            _ => return Err(format!("Unknown option: {arg}").into()),
        }
    }
    if options.connect.is_some() && options.preview {
        return Err("--connect cannot be combined with --preview".into());
    }
    if !public_addresses.is_empty() {
        options
            .network
            .set("public_addresses", serde_json::json!(public_addresses));
    }
    options.game = GameConfig::load(&options.network.root, game_config.as_deref())?;
    let mesh_override = mesh_override
        .map(|p| {
            if p.is_absolute() {
                Ok(p)
            } else {
                std::env::current_dir().map(|cwd| cwd.join(p))
            }
        })
        .transpose()?;
    // Packaged assets/config resolve beside the executable regardless of launch directory.
    std::env::set_current_dir(&options.network.root)?;
    options.mesh = mesh_override
        .unwrap_or_else(|| options.game.assets.root.join(&options.game.assets.aircraft));
    jetgame::app::run(options)
}
