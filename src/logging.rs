use log::{Level, LevelFilter, Log, Metadata, Record};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "lower")]
pub enum LogLevel {
    Off,
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    pub fn to_filter(self) -> LevelFilter {
        match self {
            LogLevel::Off => LevelFilter::Off,
            LogLevel::Warn => LevelFilter::Warn,
            LogLevel::Info => LevelFilter::Info,
            LogLevel::Debug => LevelFilter::Debug,
        }
    }
}

struct WebrunnerLogger;

impl Log for WebrunnerLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        match record.level() {
            Level::Error | Level::Warn => eprintln!("{}", record.args()),
            _ => println!("{}", record.args()),
        }
    }

    fn flush(&self) {}
}

static LOGGER: WebrunnerLogger = WebrunnerLogger;

/// Initialise the global logger. Must be called exactly once, after CLI
/// parsing and before any leveled output. The three fatal-startup
/// `eprintln!("Error: ...")` paths in `main.rs` do not use the logger
/// and remain unconditional.
#[allow(dead_code)] // wired in Task 3
pub fn init(level: LevelFilter) {
    log::set_logger(&LOGGER).expect("logger init");
    log::set_max_level(level);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_to_filter() {
        assert_eq!(LogLevel::Off.to_filter(), LevelFilter::Off);
        assert_eq!(LogLevel::Warn.to_filter(), LevelFilter::Warn);
        assert_eq!(LogLevel::Info.to_filter(), LevelFilter::Info);
        assert_eq!(LogLevel::Debug.to_filter(), LevelFilter::Debug);
    }
}
