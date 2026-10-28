//! Owned Python server process; joining and discovery never launch Python.

use super::{HostOptions, NetworkConfig};
use serde::Deserialize;
use std::{
    io::{self, BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

pub struct HostedServer {
    child: Child,
    pub endpoint: String,
    pub admin_token: String,
}

#[derive(Deserialize)]
struct Ready {
    kind: String,
    port: u16,
    admin_token: String,
}

impl HostedServer {
    pub fn start(config: &NetworkConfig, options: &HostOptions) -> io::Result<Self> {
        let python = config.python_executable()?;
        let script = config.server_script.as_ref().map_or_else(
            || config.root.join("networking/server_main.py"),
            |p| config.resolve(p),
        );
        if !script.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Python hosting server was not packaged",
            ));
        }
        let mut command = Command::new(python);
        command
            .arg("-I")
            .arg("-u")
            .arg(script)
            .arg("--parent-stdin")
            .arg("--bind")
            .arg(&config.host_bind)
            .arg("--port")
            .arg(config.host_port.to_string())
            .arg("--name")
            .arg(&options.name)
            .arg("--max-players")
            .arg(options.max_players.to_string())
            .arg("--map")
            .arg(&options.map)
            .arg("--mode")
            .arg(&options.mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(endpoint) = &config.directory {
            command.arg("--directory").arg(endpoint);
        }
        if let Some(port) = config.public_port {
            command.arg("--public-port").arg(port.to_string());
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            // Bootstrap is a single bounded JSON line, not an unbounded log stream.
            use std::io::Read;
            let result = reader
                .by_ref()
                .take(4096)
                .read_line(&mut line)
                .map(|_| line);
            let _ = tx.send(result);
        });
        let result = (|| {
            let line = rx.recv_timeout(Duration::from_secs(5)).map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "Python host startup timed out")
            })??;
            let ready: Ready = serde_json::from_str(&line).map_err(super::protocol::invalid)?;
            if ready.kind != "ready" || ready.port == 0 || ready.admin_token.len() > 128 {
                return Err(super::protocol::invalid("invalid host bootstrap"));
            }
            Ok(ready)
        })();
        let local_host = match config.host_bind.as_str() {
            "0.0.0.0" => "127.0.0.1",
            "::" => "::1",
            host => host,
        };
        let local_host = if local_host.contains(':') {
            format!("[{local_host}]")
        } else {
            local_host.to_string()
        };
        match result {
            Ok(ready) => Ok(Self {
                child,
                endpoint: format!("{local_host}:{}", ready.port),
                admin_token: ready.admin_token,
            }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }
}

impl Drop for HostedServer {
    fn drop(&mut self) {
        // EOF tells only this owned server to shut down, unregister, and close its clients.
        drop(self.child.stdin.take());
        for _ in 0..20 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
