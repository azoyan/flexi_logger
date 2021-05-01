use crate::primary_writer::PrimaryWriter;
use crate::writers::{FileLogWriterBuilder, LogWriter};
use crate::{FlexiLoggerError, LogSpecification};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Allows reconfiguring the logger programmatically.
///
/// # Example
///
/// Obtain the `LoggerHandle` (using `.start()`):
/// ```rust
/// # use flexi_logger::{Logger, LogSpecBuilder};
/// let mut logger = Logger::with_str("info")
///     // ... your logger configuration goes here, as usual
///     .start()
///     .unwrap_or_else(|e| panic!("Logger initialization failed with {}", e));
///
/// // ...
/// ```
///
/// You can permanently exchange the log specification programmatically, anywhere in your code:
///
/// ```rust
/// # use flexi_logger::{Logger, LogSpecBuilder};
/// # let mut logger = Logger::with_str("info")
/// #     .start()
/// #     .unwrap_or_else(|e| panic!("Logger initialization failed with {}", e));
/// // ...
/// logger.parse_new_spec("warn");
/// // ...
/// ```
///
/// However, when debugging, you often want to modify the log spec only temporarily, for  
/// one or few method calls only; this is easier done with the following method, because
/// it allows switching back to the previous spec:
///
/// ```rust
/// # use flexi_logger::{Logger, LogSpecBuilder};
/// #    let mut logger = Logger::with_str("info")
/// #        .start()
/// #        .unwrap_or_else(|e| panic!("Logger initialization failed with {}", e));
/// logger.parse_and_push_temp_spec("trace");
/// // ...
/// // critical calls
/// // ...
///
/// logger.pop_temp_spec();
/// // Continue with the log spec you had before.
/// // ...
/// ```
#[derive(Clone)]
pub struct LoggerHandle {
    spec: Arc<RwLock<LogSpecification>>,
    spec_stack: Vec<LogSpecification>,
    primary_writer: Arc<PrimaryWriter>,
    other_writers: Arc<HashMap<String, Box<dyn LogWriter>>>,
}

impl LoggerHandle {
    pub(crate) fn new(
        spec: Arc<RwLock<LogSpecification>>,
        primary_writer: Arc<PrimaryWriter>,
        other_writers: Arc<HashMap<String, Box<dyn LogWriter>>>,
    ) -> Self {
        Self {
            spec,
            spec_stack: Vec::default(),
            primary_writer,
            other_writers,
        }
    }

    #[cfg(feature = "specfile_without_notification")]
    pub(crate) fn current_spec(&self) -> Arc<RwLock<LogSpecification>> {
        Arc::clone(&self.spec)
    }

    //
    pub(crate) fn reconfigure(&self, mut max_level: log::LevelFilter) {
        for w in self.other_writers.as_ref().values() {
            max_level = std::cmp::max(max_level, w.max_log_level());
        }
        log::set_max_level(max_level);
    }

    /// Replaces the active `LogSpecification`.
    #[allow(clippy::missing_panics_doc)]
    pub fn set_new_spec(&mut self, new_spec: LogSpecification) {
        let max_level = new_spec.max_level();
        self.spec.write().unwrap(/* catch and expose error? */).update_from(new_spec);
        self.reconfigure(max_level);
    }

    /// Tries to replace the active `LogSpecification` with the result from parsing the given String.
    pub fn parse_new_spec(&mut self, spec: &str) {
        self.set_new_spec(LogSpecification::parse(spec).unwrap_or_else(|e| {
            eprintln!(
                "[flexi_logger] LoggerHandle::parse_new_spec(): failed with {}",
                e
            );
            LogSpecification::off()
        }))
    }

    /// Replaces the active `LogSpecification` and pushes the previous one to a Stack.
    #[allow(clippy::missing_panics_doc)]
    pub fn push_temp_spec(&mut self, new_spec: LogSpecification) {
        self.spec_stack
            .push(self.spec.read().unwrap(/* catch and expose error? */).clone());
        self.set_new_spec(new_spec);
    }

    /// Tries to replace the active `LogSpecification` with the result from parsing the given String
    ///  and pushes the previous one to a Stack.
    #[allow(clippy::missing_panics_doc)]
    pub fn parse_and_push_temp_spec<S: AsRef<str>>(&mut self, new_spec: S) {
        self.spec_stack
            .push(self.spec.read().unwrap(/* catch and expose error? */).clone());
        self.set_new_spec(LogSpecification::parse(new_spec).unwrap_or_else(|e| {
            eprintln!(
                "[flexi_logger] LoggerHandle::parse_new_spec(): failed with {}, \
                 falling back to empty log spec",
                e
            );
            LogSpecification::off()
        }));
    }

    /// Reverts to the previous `LogSpecification`, if any.
    pub fn pop_temp_spec(&mut self) {
        if let Some(previous_spec) = self.spec_stack.pop() {
            self.set_new_spec(previous_spec);
        }
    }

    /// Flush all writers.
    pub fn flush(&self) {
        self.primary_writer.flush().ok();
        for writer in self.other_writers.values() {
            writer.flush().ok();
        }
    }

    /// Replace the configuration of the file log writer.
    ///
    /// Note that there are two exceptions:
    /// neither the format function nor the line ending can be reset.
    ///
    /// # Errors
    ///
    /// `FlexiLoggerError::Reset` if no file log writer is configured.
    pub fn reset(&self, flwb: &FileLogWriterBuilder) -> Result<(), FlexiLoggerError> {
        if let PrimaryWriter::Multi(ref mw) = &*self.primary_writer {
            mw.reset_file_log_writer(flwb)
        } else {
            Err(FlexiLoggerError::Reset)
        }
    }

    /// Shutdown all participating writers.
    ///
    /// This method is supposed to be called at the very end of your program, if
    ///
    /// - you use some [`Cleanup`](crate::Cleanup) strategy with compression:
    ///   then you want to ensure that a termination of your program
    ///   does not interrput the cleanup-thread when it is compressing a log file,
    ///   which could leave unexpected files in the filesystem
    /// - you use your own writer(s), and they need to clean up resources
    ///
    /// See also [`writers::LogWriter::shutdown`](crate::writers::LogWriter::shutdown).
    pub fn shutdown(&self) {
        self.primary_writer.shutdown();
        for writer in self.other_writers.values() {
            writer.shutdown();
        }
    }

    // Allows checking the logs written so far to the writer
    #[doc(hidden)]
    pub fn validate_logs(&self, expected: &[(&'static str, &'static str, &'static str)]) {
        self.primary_writer.validate_logs(expected)
    }
}

impl Drop for LoggerHandle {
    fn drop(&mut self) {
        self.primary_writer.flush().ok();
        for writer in self.other_writers.values() {
            writer.flush().ok();
        }
    }
}