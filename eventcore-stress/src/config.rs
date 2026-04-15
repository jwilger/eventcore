use std::fmt;
use std::time::Duration;

/// Which event store backend to stress test.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum BackendChoice {
    Memory,
    Sqlite,
    Postgres,
}

impl fmt::Display for BackendChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendChoice::Memory => write!(f, "memory"),
            BackendChoice::Sqlite => write!(f, "sqlite"),
            BackendChoice::Postgres => write!(f, "postgres"),
        }
    }
}

/// Common configuration for all stress test scenarios.
#[derive(Debug, Clone)]
pub struct StressConfig {
    pub backend: BackendChoice,
    pub concurrency: u32,
    pub duration: Option<Duration>,
    pub iterations: Option<u64>,
}

impl StressConfig {
    /// Determine the effective termination condition.
    /// If neither duration nor iterations is set, default to 10 seconds.
    pub fn effective_duration(&self) -> Option<Duration> {
        if self.iterations.is_some() {
            None
        } else {
            Some(self.duration.unwrap_or(Duration::from_secs(10)))
        }
    }

    pub fn effective_iterations(&self) -> Option<u64> {
        self.iterations
    }
}

/// Parse a duration string like "10s", "30s", "1m", "2m30s".
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration string".to_string());
    }

    // Try simple patterns first
    if let Some(secs) = s.strip_suffix('s') {
        let secs: u64 = secs
            .trim()
            .parse()
            .map_err(|e| format!("invalid seconds: {e}"))?;
        return Ok(Duration::from_secs(secs));
    }

    if let Some(mins) = s.strip_suffix('m') {
        // Check if there's an 's' component, like "2m30s" — but we already
        // stripped 's' above, so this is just "Nm"
        let mins: u64 = mins
            .trim()
            .parse()
            .map_err(|e| format!("invalid minutes: {e}"))?;
        return Ok(Duration::from_secs(mins * 60));
    }

    // Try parsing as raw seconds
    let secs: u64 = s
        .parse()
        .map_err(|_| format!("unrecognized duration format: {s}"))?;
    Ok(Duration::from_secs(secs))
}
