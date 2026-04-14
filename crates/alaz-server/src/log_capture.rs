//! Custom tracing layer that captures structured logs and writes them to the database.
//!
//! Architecture:
//! - The layer implements `tracing_subscriber::Layer` and filters for warn/error events.
//! - Events are pushed to a bounded mpsc channel (non-blocking, drops on overflow).
//! - A background task reads from the channel in batches and writes to PostgreSQL.
//! - Errors compute a fingerprint from target + normalized message for aggregation.

use std::collections::HashMap;
use std::sync::OnceLock;

use alaz_db::repos::{NewLog, StructuredLogRepo};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

const CHANNEL_CAPACITY: usize = 10_000;
const BATCH_SIZE: usize = 50;
const BATCH_INTERVAL_MS: u64 = 5_000;

/// Global sender for the log capture channel.
/// Set once at startup by `init_log_capture`.
static LOG_SENDER: OnceLock<mpsc::Sender<NewLog>> = OnceLock::new();

/// Custom tracing layer that captures warn/error events.
pub struct LogCaptureLayer;

impl<S> Layer<S> for LogCaptureLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level = *metadata.level();

        // Only capture warn and error
        if level > tracing::Level::WARN {
            return;
        }

        let sender = match LOG_SENDER.get() {
            Some(s) => s,
            None => return, // Not initialized yet
        };

        // Extract fields from the event
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let level_str = match level {
            tracing::Level::ERROR => "error",
            tracing::Level::WARN => "warn",
            tracing::Level::INFO => "info",
            tracing::Level::DEBUG => "debug",
            tracing::Level::TRACE => "trace",
        };

        let target = metadata.target().to_string();
        let message = visitor.message.unwrap_or_else(|| "".to_string());

        // Compute fingerprint for errors (for aggregation)
        let fingerprint = if level <= tracing::Level::WARN {
            Some(compute_fingerprint(&target, &message))
        } else {
            None
        };

        let fields = if visitor.fields.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(
                visitor
                    .fields
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            ))
        };

        let log = NewLog {
            level: level_str.to_string(),
            target,
            message,
            fields,
            fingerprint,
        };

        // Non-blocking send — drop on overflow to protect server performance
        let _ = sender.try_send(log);
    }
}

/// Compute a fingerprint for error grouping.
///
/// Normalizes the message by removing numbers, UUIDs, and common variable data,
/// then takes sha256 of (target + normalized message).
fn compute_fingerprint(target: &str, message: &str) -> String {
    let normalized = normalize_message(message);
    let mut hasher = Sha256::new();
    hasher.update(target.as_bytes());
    hasher.update(b"::");
    hasher.update(normalized.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..16]) // 32-char fingerprint
}

fn normalize_message(msg: &str) -> String {
    // Replace common variable patterns with placeholders
    let mut normalized = String::with_capacity(msg.len());
    let mut in_number = false;
    for c in msg.chars() {
        if c.is_ascii_digit() {
            if !in_number {
                normalized.push_str("<N>");
                in_number = true;
            }
        } else {
            in_number = false;
            // Keep only alphanumeric and basic punctuation
            if c.is_alphanumeric() || c == ' ' || c == '.' || c == ':' || c == '-' || c == '_' {
                normalized.push(c);
            }
        }
    }
    // Collapse whitespace
    normalized.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    fields: HashMap<String, String>,
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let value_str = format!("{value:?}");
        if field.name() == "message" {
            // Strip debug quotes
            self.message = Some(
                value_str
                    .trim_start_matches('"')
                    .trim_end_matches('"')
                    .to_string(),
            );
        } else {
            self.fields.insert(field.name().to_string(), value_str);
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
}

/// Initialize the log capture system. Must be called once at startup BEFORE
/// the tracing subscriber is registered. Returns the sender for the layer to use.
pub fn init_log_capture(pool: PgPool) -> LogCaptureLayer {
    let (tx, mut rx) = mpsc::channel::<NewLog>(CHANNEL_CAPACITY);

    // Store sender in global
    let _ = LOG_SENDER.set(tx);

    // Spawn background batch writer
    tokio::spawn(async move {
        let mut batch: Vec<NewLog> = Vec::with_capacity(BATCH_SIZE);
        let mut interval =
            tokio::time::interval(std::time::Duration::from_millis(BATCH_INTERVAL_MS));

        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(log) => {
                            batch.push(log);
                            if batch.len() >= BATCH_SIZE {
                                flush_batch(&pool, &mut batch).await;
                            }
                        }
                        None => {
                            // Channel closed — final flush and exit
                            flush_batch(&pool, &mut batch).await;
                            break;
                        }
                    }
                }
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        flush_batch(&pool, &mut batch).await;
                    }
                }
            }
        }
    });

    LogCaptureLayer
}

async fn flush_batch(pool: &PgPool, batch: &mut Vec<NewLog>) {
    if batch.is_empty() {
        return;
    }
    // Use a detached write to avoid blocking the loop on slow DB
    let logs = std::mem::take(batch);
    match StructuredLogRepo::insert_batch(pool, &logs).await {
        Ok(n) => {
            // Use eprintln! to avoid infinite recursion via tracing
            if n > 0 {
                // Silent success
            }
        }
        Err(e) => {
            eprintln!("log_capture: failed to write batch: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_normalizes_numbers() {
        let fp1 = compute_fingerprint("alaz_intel::llm", "request failed after 3 seconds");
        let fp2 = compute_fingerprint("alaz_intel::llm", "request failed after 5 seconds");
        assert_eq!(fp1, fp2, "numbers should be normalized");
    }

    #[test]
    fn fingerprint_different_targets() {
        let fp1 = compute_fingerprint("alaz_intel::llm", "connection refused");
        let fp2 = compute_fingerprint("alaz_server::jobs", "connection refused");
        assert_ne!(
            fp1, fp2,
            "different targets should give different fingerprints"
        );
    }

    #[test]
    fn fingerprint_different_messages() {
        let fp1 = compute_fingerprint("alaz_intel::llm", "connection refused");
        let fp2 = compute_fingerprint("alaz_intel::llm", "timeout exceeded");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn normalize_removes_punctuation() {
        let n = normalize_message("Error! Request @#$ failed with code 404");
        assert!(!n.contains('!'));
        assert!(!n.contains('@'));
        assert!(n.contains("<N>"));
    }
}
