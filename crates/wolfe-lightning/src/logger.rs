use lightning::util::logger::{Level, Logger, Record};

/// Bridges LDK's Logger trait to the `tracing` crate.
pub struct WolfeLogger;

impl Logger for WolfeLogger {
    fn log(&self, record: Record) {
        let module = record.module_path;
        let line = record.line;

        match record.level {
            Level::Gossip | Level::Trace => {
                tracing::trace!(target: "ldk", module, line, "{}", record.args);
            }
            Level::Debug => {
                tracing::debug!(target: "ldk", module, line, "{}", record.args);
            }
            Level::Info => {
                tracing::info!(target: "ldk", module, line, "{}", record.args);
            }
            Level::Warn => {
                tracing::warn!(target: "ldk", module, line, "{}", record.args);
            }
            Level::Error => {
                tracing::error!(target: "ldk", module, line, "{}", record.args);
            }
        }
    }
}
