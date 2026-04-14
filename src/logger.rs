use std::sync::Arc;

pub trait Logger: Send + Sync {
    fn debug(&self, message: &str);
    fn info(&self, message: &str);
    fn warn(&self, message: &str);
    fn error(&self, message: &str);
}

pub type SharedLogger = Arc<dyn Logger>;

#[derive(Default)]
pub struct NoopLogger;

impl Logger for NoopLogger {
    fn debug(&self, _message: &str) {}
    fn info(&self, _message: &str) {}
    fn warn(&self, _message: &str) {}
    fn error(&self, _message: &str) {}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

pub struct ConsoleLogger {
    level: LogLevel,
}

impl ConsoleLogger {
    pub fn new(level: LogLevel) -> Self {
        Self { level }
    }

    fn should_log(&self, level: LogLevel) -> bool {
        level >= self.level
    }
}

impl Logger for ConsoleLogger {
    fn debug(&self, message: &str) {
        if self.should_log(LogLevel::Debug) {
            eprintln!("[Limitless SDK][DEBUG] {message}");
        }
    }

    fn info(&self, message: &str) {
        if self.should_log(LogLevel::Info) {
            eprintln!("[Limitless SDK][INFO] {message}");
        }
    }

    fn warn(&self, message: &str) {
        if self.should_log(LogLevel::Warn) {
            eprintln!("[Limitless SDK][WARN] {message}");
        }
    }

    fn error(&self, message: &str) {
        if self.should_log(LogLevel::Error) {
            eprintln!("[Limitless SDK][ERROR] {message}");
        }
    }
}

pub fn noop_logger() -> SharedLogger {
    Arc::new(NoopLogger)
}
