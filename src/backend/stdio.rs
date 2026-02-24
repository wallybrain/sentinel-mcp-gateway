use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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

        // Clear inherited environment to prevent secret leakage (e.g. JWT_SECRET_KEY,
        // DATABASE_URL) to child processes. Only pass through safe system vars and
        // any explicit per-backend env entries from sentinel.toml.
        cmd.env_clear();
        for key in &["PATH", "HOME", "USER", "LANG", "NODE_PATH", "NVM_DIR", "NVM_BIN", "TMPDIR", "TERM"] {
            if let Ok(val) = std::env::var(key) {
                cmd.env(key, val);
            }
        }
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

    pub fn stdin_sender(&self) -> &mpsc::Sender<String> {
        &self.stdin_tx
    }
}

pub async fn discover_stdio_tools(backend: &StdioBackend) -> anyhow::Result<Vec<rmcp::model::Tool>> {
    // Step 1: Send MCP initialize request
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "sentinel-gateway",
                "version": "0.1.0"
            }
        }
    });
    let init_body = serde_json::to_string(&init_req)?;
    let init_response = backend
        .send(&init_body)
        .await
        .map_err(|e| anyhow::anyhow!("MCP initialize failed: {e}"))?;
    tracing::debug!(backend = %backend.name(), response = %init_response, "MCP initialize response");

    // Step 2: Send notifications/initialized (no id, no response expected)
    let notif = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    let notif_body = serde_json::to_string(&notif)?;
    backend
        .stdin_sender()
        .send(notif_body)
        .await
        .map_err(|_| anyhow::anyhow!("failed to send notifications/initialized: stdin closed"))?;

    // Step 3: Brief pause for server to process notification
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Step 4: Send tools/list request
    let list_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    let list_body = serde_json::to_string(&list_req)?;
    let list_response = backend
        .send(&list_body)
        .await
        .map_err(|e| anyhow::anyhow!("MCP tools/list failed: {e}"))?;
    tracing::debug!(backend = %backend.name(), response = %list_response, "MCP tools/list response");

    // Step 5: Parse tools from response
    let parsed: serde_json::Value = serde_json::from_str(&list_response)?;
    let tools_value = parsed
        .get("result")
        .and_then(|r| r.get("tools"))
        .ok_or_else(|| anyhow::anyhow!("no tools in tools/list response"))?;

    let tools: Vec<rmcp::model::Tool> = serde_json::from_value(tools_value.clone())?;
    tracing::info!(
        backend = %backend.name(),
        count = tools.len(),
        "discovered tools from stdio backend"
    );

    Ok(tools)
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

pub async fn run_supervisor(
    config: BackendConfig,
    cancel: CancellationToken,
    on_tools_discovered: mpsc::Sender<(String, Vec<rmcp::model::Tool>, StdioBackend)>,
) {
    const BACKOFF_BASE_SECS: f64 = 1.0;
    const BACKOFF_MAX_SECS: f64 = 60.0;
    const HEALTHY_THRESHOLD: Duration = Duration::from_secs(60);

    let mut restart_count: u32 = 0;

    loop {
        if cancel.is_cancelled() {
            tracing::info!(backend = %config.name, "supervisor stopping (cancelled before spawn)");
            break;
        }

        let spawn_result = StdioBackend::spawn(&config);
        let (backend, _stdin_handle, stdout_handle) = match spawn_result {
            Ok(result) => result,
            Err(e) => {
                tracing::error!(backend = %config.name, error = %e, "failed to spawn child process");
                restart_count += 1;
                if config.max_restarts > 0 && restart_count >= config.max_restarts {
                    tracing::error!(
                        backend = %config.name,
                        restarts = restart_count,
                        max = config.max_restarts,
                        "max restarts reached, supervisor stopping"
                    );
                    break;
                }
                let delay = backoff_delay(restart_count, BACKOFF_BASE_SECS, BACKOFF_MAX_SECS);
                tokio::select! {
                    _ = tokio::time::sleep(delay) => continue,
                    _ = cancel.cancelled() => {
                        tracing::info!(backend = %config.name, "supervisor stopping during backoff");
                        break;
                    }
                }
            }
        };

        let pid = backend.pid();
        let spawn_time = Instant::now();

        // Perform MCP handshake
        match discover_stdio_tools(&backend).await {
            Ok(tools) => {
                if let Err(_) = on_tools_discovered
                    .send((config.name.clone(), tools, backend.clone()))
                    .await
                {
                    tracing::warn!(backend = %config.name, "tools channel closed, supervisor stopping");
                    kill_process_group(pid);
                    break;
                }
            }
            Err(e) => {
                tracing::error!(backend = %config.name, error = %e, "MCP handshake failed after spawn");
                kill_process_group(pid);
                restart_count += 1;
                if config.max_restarts > 0 && restart_count >= config.max_restarts {
                    tracing::error!(
                        backend = %config.name,
                        restarts = restart_count,
                        max = config.max_restarts,
                        "max restarts reached, supervisor stopping"
                    );
                    break;
                }
                let delay = backoff_delay(restart_count, BACKOFF_BASE_SECS, BACKOFF_MAX_SECS);
                tokio::select! {
                    _ = tokio::time::sleep(delay) => continue,
                    _ = cancel.cancelled() => {
                        tracing::info!(backend = %config.name, "supervisor stopping during backoff");
                        break;
                    }
                }
            }
        }

        // Monitor for child exit or cancellation
        tokio::select! {
            _ = stdout_handle => {
                // stdout_reader completed = child exited (EOF)
                tracing::warn!(backend = %config.name, pid = pid, "child process exited (stdout EOF)");
                drain_pending(&backend.pending);
                kill_process_group(pid);

                // Reset restart count if process was healthy for a while
                if spawn_time.elapsed() > HEALTHY_THRESHOLD {
                    restart_count = 0;
                }

                restart_count += 1;
                if config.max_restarts > 0 && restart_count >= config.max_restarts {
                    tracing::error!(
                        backend = %config.name,
                        restarts = restart_count,
                        max = config.max_restarts,
                        "max restarts reached, supervisor stopping"
                    );
                    break;
                }

                let delay = backoff_delay(restart_count, BACKOFF_BASE_SECS, BACKOFF_MAX_SECS);
                tracing::info!(
                    backend = %config.name,
                    restart = restart_count,
                    delay_ms = delay.as_millis() as u64,
                    "restarting after backoff"
                );
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = cancel.cancelled() => {
                        tracing::info!(backend = %config.name, "supervisor stopping during backoff");
                        break;
                    }
                }
            }
            _ = cancel.cancelled() => {
                tracing::info!(backend = %config.name, "supervisor shutting down");
                kill_process_group(pid);
                // Give child a moment to exit
                let _ = tokio::time::timeout(Duration::from_secs(2), async {
                    // stdin_handle and stdout_handle will complete when pipes close
                    loop { tokio::time::sleep(Duration::from_millis(50)).await; }
                }).await;
                break;
            }
        }
    }
}

fn backoff_delay(restart_count: u32, base_secs: f64, max_secs: f64) -> Duration {
    let exp = base_secs * 2.0_f64.powi(restart_count.saturating_sub(1) as i32);
    let capped = exp.min(max_secs);
    let jitter = rand::random::<f64>() * 0.5 * capped;
    Duration::from_secs_f64(capped + jitter)
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

    fn test_config(command: &str, args: Vec<&str>, max_restarts: u32) -> BackendConfig {
        BackendConfig {
            name: "test-supervisor".to_string(),
            backend_type: crate::config::types::BackendType::Stdio,
            url: None,
            command: Some(command.to_string()),
            args: args.into_iter().map(String::from).collect(),
            env: HashMap::new(),
            timeout_secs: 5,
            retries: 0,
            restart_on_exit: true,
            max_restarts,
            health_interval_secs: 300,
            circuit_breaker_threshold: 5,
            circuit_breaker_recovery_secs: 30,
        }
    }

    #[tokio::test]
    async fn test_supervisor_detects_child_exit_and_restarts() {
        // "true" exits immediately with code 0 -- supervisor should detect and restart
        let config = test_config("true", vec![], 3);
        let cancel = CancellationToken::new();
        let (tools_tx, mut tools_rx) = mpsc::channel(10);

        let cancel_clone = cancel.clone();
        let handle = tokio::spawn(async move {
            run_supervisor(config, cancel_clone, tools_tx).await;
        });

        // Supervisor will spawn "true" which exits immediately.
        // MCP handshake will fail (true produces no output), so it goes to restart.
        // After max_restarts=3, supervisor should stop on its own.
        let result = tokio::time::timeout(Duration::from_secs(30), handle).await;
        assert!(result.is_ok(), "supervisor should have stopped after max_restarts");

        // No tools should have been discovered (true doesn't speak MCP)
        assert!(tools_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_supervisor_respects_cancellation_during_backoff() {
        // "true" exits immediately -- supervisor will enter backoff
        let config = test_config("true", vec![], 10); // high limit so it doesn't stop on its own
        let cancel = CancellationToken::new();
        let (tools_tx, _tools_rx) = mpsc::channel(10);

        let cancel_clone = cancel.clone();
        let handle = tokio::spawn(async move {
            run_supervisor(config, cancel_clone, tools_tx).await;
        });

        // Give supervisor time to spawn, fail handshake, enter backoff
        tokio::time::sleep(Duration::from_millis(500)).await;

        let start = Instant::now();
        cancel.cancel();

        let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(result.is_ok(), "supervisor should exit promptly on cancel");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "supervisor should exit within 2s of cancel, took {:?}",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn test_supervisor_stops_after_max_restarts() {
        let config = test_config("true", vec![], 2);
        let cancel = CancellationToken::new();
        let (tools_tx, _tools_rx) = mpsc::channel(10);

        let cancel_clone = cancel.clone();
        let start = Instant::now();
        let handle = tokio::spawn(async move {
            run_supervisor(config, cancel_clone, tools_tx).await;
        });

        let result = tokio::time::timeout(Duration::from_secs(30), handle).await;
        assert!(result.is_ok(), "supervisor should stop after max_restarts=2");

        // Verify it didn't exit because of cancellation
        assert!(!cancel.is_cancelled(), "cancel should not have been triggered");
        tracing::info!(elapsed_ms = start.elapsed().as_millis() as u64, "supervisor stopped after max_restarts");
    }
}
