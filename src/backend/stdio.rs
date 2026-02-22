use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::error::BackendError;
use crate::config::types::BackendConfig;

#[derive(Clone)]
pub struct StdioBackend {
    name: String,
    stdin_tx: mpsc::Sender<String>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>,
    pid: Arc<AtomicU32>,
    timeout: Duration,
}

impl StdioBackend {
    pub fn spawn(config: &BackendConfig) -> Result<(Self, JoinHandle<()>, JoinHandle<()>), BackendError> {
        let command = config.command.as_deref().ok_or_else(|| {
            BackendError::InvalidResponse("stdio backend requires 'command' field".to_string())
        })?;

        let mut cmd = Command::new(command);
        cmd.args(&config.args);
        for (k, v) in &config.env {
            cmd.env(k, v);
        }
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .process_group(0)
            .kill_on_drop(false);

        let mut child = cmd.spawn().map_err(|e| {
            BackendError::ProcessExited(format!("failed to spawn '{}': {}", command, e))
        })?;

        let child_pid = child.id().unwrap_or(0);
        let pid = Arc::new(AtomicU32::new(child_pid));

        let child_stdin = child.stdin.take().ok_or(BackendError::StdinClosed)?;
        let child_stdout = child.stdout.take().ok_or_else(|| {
            BackendError::ProcessExited("failed to capture stdout".to_string())
        })?;
        // stderr is left attached to child for future logging

        let (stdin_tx, stdin_rx) = mpsc::channel::<String>(64);
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let name = config.name.clone();
        let timeout = Duration::from_secs(config.timeout_secs);

        // Spawn stdin writer task
        let writer_name = name.clone();
        let stdin_handle = tokio::spawn(stdin_writer(writer_name, child_stdin, stdin_rx));

        // Spawn stdout reader task
        let reader_name = name.clone();
        let reader_pending = pending.clone();
        let stdout_handle = tokio::spawn(stdout_reader(reader_name, child_stdout, reader_pending));

        let backend = StdioBackend {
            name,
            stdin_tx,
            pending,
            pid,
            timeout,
        };

        Ok((backend, stdin_handle, stdout_handle))
    }

    pub async fn send(&self, json_rpc_body: &str) -> Result<String, BackendError> {
        let parsed: serde_json::Value = serde_json::from_str(json_rpc_body)
            .map_err(|e| BackendError::InvalidResponse(format!("invalid JSON: {e}")))?;

        let id = parsed
            .get("id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| BackendError::InvalidResponse("missing or non-numeric 'id' field".to_string()))?;

        let (tx, rx) = oneshot::channel();

        {
            let mut map = self.pending.lock().expect("pending map poisoned");
            map.insert(id, tx);
        }

        if let Err(_) = self.stdin_tx.send(json_rpc_body.to_string()).await {
            // Remove from pending before returning error
            let mut map = self.pending.lock().expect("pending map poisoned");
            map.remove(&id);
            return Err(BackendError::StdinClosed);
        }

        match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                // Channel dropped -- process exited
                Err(BackendError::ProcessExited("response channel dropped".to_string()))
            }
            Err(_) => {
                // Timeout -- clean up pending entry
                let mut map = self.pending.lock().expect("pending map poisoned");
                map.remove(&id);
                Err(BackendError::InvalidResponse("request timed out".to_string()))
            }
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn pid(&self) -> u32 {
        self.pid.load(Ordering::Relaxed)
    }
}

async fn stdin_writer(
    name: String,
    mut stdin: tokio::process::ChildStdin,
    mut rx: mpsc::Receiver<String>,
) {
    while let Some(msg) = rx.recv().await {
        let line = if msg.ends_with('\n') {
            msg
        } else {
            format!("{msg}\n")
        };
        if let Err(e) = stdin.write_all(line.as_bytes()).await {
            tracing::warn!(backend = %name, error = %e, "stdin write failed");
            break;
        }
        if let Err(e) = stdin.flush().await {
            tracing::warn!(backend = %name, error = %e, "stdin flush failed");
            break;
        }
    }
    tracing::debug!(backend = %name, "stdin writer exiting");
}

async fn stdout_reader(
    name: String,
    stdout: tokio::process::ChildStdout,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>,
) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // EOF -- process exited
                tracing::debug!(backend = %name, "stdout EOF, child process exited");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::debug!(backend = %name, error = %e, line = %trimmed, "non-JSON line from stdout");
                        continue;
                    }
                };

                let id = match parsed.get("id").and_then(|v| v.as_u64()) {
                    Some(id) => id,
                    None => {
                        tracing::debug!(backend = %name, "stdout line has no 'id' field, skipping");
                        continue;
                    }
                };

                let sender = {
                    let mut map = pending.lock().expect("pending map poisoned");
                    map.remove(&id)
                };

                match sender {
                    Some(tx) => {
                        let _ = tx.send(trimmed.to_string());
                    }
                    None => {
                        tracing::debug!(backend = %name, id = id, "no pending request for response id");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(backend = %name, error = %e, "stdout read error");
                break;
            }
        }
    }

    drain_pending(&pending);
}

pub fn kill_process_group(pid: u32) {
    let pgid = Pid::from_raw(pid as i32);
    match signal::killpg(pgid, Signal::SIGTERM) {
        Ok(()) => {
            tracing::debug!(pid = pid, "sent SIGTERM to process group");
        }
        Err(nix::errno::Errno::ESRCH) => {
            tracing::debug!(pid = pid, "process group already dead");
        }
        Err(e) => {
            tracing::warn!(pid = pid, error = %e, "failed to kill process group");
        }
    }
}

pub fn drain_pending(pending: &Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>) {
    let mut map = pending.lock().expect("pending map poisoned");
    let count = map.len();
    map.drain();
    if count > 0 {
        tracing::debug!(count = count, "drained pending requests");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kill_process_group() {
        // Spawn a sleep process in its own process group
        let mut child = Command::new("sleep")
            .arg("60")
            .process_group(0)
            .spawn()
            .expect("failed to spawn sleep");

        let pid = child.id().expect("no pid");

        // Kill the process group
        kill_process_group(pid);

        // Verify child exits
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .expect("child didn't exit after SIGTERM")
            .expect("wait failed");

        assert!(!status.success(), "child should have been terminated");
    }

    #[test]
    fn test_drain_pending() {
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let (tx1, mut rx1) = oneshot::channel();
        let (tx2, mut rx2) = oneshot::channel();

        {
            let mut map = pending.lock().unwrap();
            map.insert(1, tx1);
            map.insert(2, tx2);
        }

        assert_eq!(pending.lock().unwrap().len(), 2);

        drain_pending(&pending);

        assert_eq!(pending.lock().unwrap().len(), 0);

        // Receivers should get errors since senders were dropped
        assert!(rx1.try_recv().is_err());
        assert!(rx2.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_spawn_and_send_with_cat() {
        // cat echoes stdin to stdout line-by-line
        let config = BackendConfig {
            name: "test-cat".to_string(),
            backend_type: crate::config::types::BackendType::Stdio,
            url: None,
            command: Some("cat".to_string()),
            args: vec![],
            env: HashMap::new(),
            timeout_secs: 5,
            retries: 0,
            restart_on_exit: false,
            max_restarts: 0,
            health_interval_secs: 300,
            circuit_breaker_threshold: 5,
            circuit_breaker_recovery_secs: 30,
        };

        let (backend, stdin_handle, stdout_handle) =
            StdioBackend::spawn(&config).expect("spawn failed");

        // Send a JSON-RPC request -- cat will echo it back
        let request = r#"{"jsonrpc":"2.0","id":42,"method":"test","params":{}}"#;
        let response = backend.send(request).await.expect("send failed");

        let parsed: serde_json::Value = serde_json::from_str(&response).expect("response not JSON");
        assert_eq!(parsed["id"], 42);
        assert_eq!(parsed["method"], "test");

        // Clean up
        let pid = backend.pid();
        drop(backend);
        kill_process_group(pid);

        // Wait for tasks to finish
        let _ = tokio::time::timeout(Duration::from_secs(2), stdin_handle).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), stdout_handle).await;
    }
}
