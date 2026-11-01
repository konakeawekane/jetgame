//! Owned Python agent with bounded pipe IPC. No sockets, discovery, or connection policy.

use super::{NetworkConfig, NetworkEvent, Request};
use serde::Serialize;
use serde_json::json;
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    time::Duration,
};

const MAX_IPC: usize = 65536;

pub struct NetworkClient {
    requests: SyncSender<Request>,
    events: Receiver<NetworkEvent>,
    stop: Arc<AtomicBool>,
    reported_stop: bool,
    failure: Option<String>,
}
impl NetworkClient {
    pub fn new(config: NetworkConfig) -> Self {
        let (tx, requests) = mpsc::sync_channel(16);
        let (events, rx) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            if let Err(error) = worker(config, requests, &events, worker_stop) {
                let _ = events.try_send(NetworkEvent::Error {
                    op: 0,
                    message: error.to_string(),
                });
            }
        });
        Self {
            requests: tx,
            events: rx,
            stop,
            reported_stop: false,
            failure: None,
        }
    }
    pub fn submit(&self, request: Request) -> Result<(), String> {
        self.requests
            .try_send(request)
            .map_err(|_| "Networking request queue is unavailable or full".into())
    }
    pub fn poll(&mut self) -> Vec<NetworkEvent> {
        let mut events = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => {
                    if let NetworkEvent::Error { op: 0, message } = &event {
                        self.failure = Some(message.clone());
                    }
                    events.push(event);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.reported_stop {
                        events.push(NetworkEvent::WorkerStopped {
                            reason: self
                                .failure
                                .take()
                                .unwrap_or_else(|| "Network service stopped".into()),
                        });
                        self.reported_stop = true;
                    }
                    break;
                }
            }
        }
        events
    }
}
impl Drop for NetworkClient {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

struct Agent(Child);
impl Drop for Agent {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        for _ in 0..80 {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn write_message(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_IPC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC command too large",
        ));
    }
    writer.write_all(&bytes)?;
    writer.flush()
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<NetworkEvent>> {
    let mut line = String::new();
    if reader.take(MAX_IPC as u64 + 1).read_line(&mut line)? == 0 {
        return Ok(None);
    }
    if line.len() > MAX_IPC || !line.ends_with('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid IPC event length",
        ));
    }
    serde_json::from_str(&line)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn worker(
    config: NetworkConfig,
    requests: Receiver<Request>,
    events: &SyncSender<NetworkEvent>,
    stop: Arc<AtomicBool>,
) -> io::Result<()> {
    let script = config.agent_script.as_ref().map_or_else(
        || config.root.join("networking/agent_main.py"),
        |p| config.resolve(p),
    );
    if !script.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Python networking agent was not packaged",
        ));
    }
    let python = config.python_executable()?;
    let mut command = Command::new(&python);
    command
        .current_dir(python.parent().unwrap_or(&config.root))
        .args(["-I", "-u"])
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(if cfg!(debug_assertions) {
            Stdio::inherit()
        } else {
            Stdio::null()
        });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut agent = Agent(command.spawn().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("Cannot start bundled networking Python: {error}. Restore runtime/python with scripts/setup-runtime.ps1"),
        )
    })?);
    let stdout = agent.0.stdout.take().unwrap();
    let (tx, incoming) = mpsc::sync_channel(64);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        while let Ok(Some(event)) = read_message(&mut reader) {
            if tx.try_send(event).is_err() {
                break;
            }
        }
    });
    let stdin = agent.0.stdin.as_mut().unwrap();
    write_message(
        stdin,
        &json!({"cmd":"configure", "settings":config.settings}),
    )?;
    while !stop.load(Ordering::Acquire) {
        match requests.recv_timeout(Duration::from_millis(10)) {
            Ok(request) => write_message(stdin, &request)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        loop {
            match incoming.try_recv() {
                Ok(event) => {
                    if events.try_send(event).is_err() {
                        return Ok(());
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pipe_messages_are_bounded_and_never_execute_content() {
        let data = b"{\"event\":\"error\",\"op\":3,\"message\":\"$(literal)\"}\n";
        assert!(matches!(
            read_message(&mut &data[..]).unwrap(),
            Some(NetworkEvent::Error { op: 3, .. })
        ));
        assert!(read_message(&mut &vec![b'x'; MAX_IPC + 1][..]).is_err());
    }
}
