use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::mpsc;

/// Reads newline-delimited messages from stdin and sends them into a channel.
///
/// Trims trailing whitespace from each line. Skips empty lines.
/// Exits on EOF (0 bytes read) or when the receiver is dropped.
pub async fn stdio_reader(tx: mpsc::Sender<String>) {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim_end().to_string();
                if !trimmed.is_empty() && tx.send(trimmed).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "stdin read error");
                break;
            }
        }
    }
}

/// Reads messages from a channel and writes them as newline-delimited output to stdout.
///
/// Flushes after every message to ensure the client receives it immediately.
/// Exits when the sender is dropped or on write error.
pub async fn stdio_writer(mut rx: mpsc::Receiver<String>) {
    let stdout = tokio::io::stdout();
    let mut writer = BufWriter::new(stdout);
    while let Some(msg) = rx.recv().await {
        if writer.write_all(msg.as_bytes()).await.is_err() {
            break;
        }
        if writer.write_all(b"\n").await.is_err() {
            break;
        }
        if writer.flush().await.is_err() {
            break;
        }
    }
}
