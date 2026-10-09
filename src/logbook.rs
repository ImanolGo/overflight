//! Append-only spotter's logbook.
//!
//! Writes one CSV row per aircraft when it leaves the sky: the time, its
//! identity, and the highest elevation it reached.

use std::io::Write;
use std::path::PathBuf;

/// One logged aircraft.
#[derive(Debug, Clone)]
pub struct Record {
    pub time: String,
    pub callsign: Option<String>,
    pub registration: Option<String>,
    pub type_code: Option<String>,
    pub max_elevation_deg: f64,
}

/// Appends records to a CSV file.
#[derive(Debug)]
pub struct Logger {
    path: PathBuf,
    header_written: bool,
}

impl Logger {
    /// A logger writing to `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            header_written: false,
        }
    }

    /// Append a record. Errors are ignored: a full disk should not take the
    /// display down.
    pub fn append(&mut self, record: &Record) {
        let existed = self.path.exists();
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        if !existed && !self.header_written {
            let _ = writeln!(file, "time,callsign,registration,type,max_elevation_deg");
        }
        self.header_written = true;
        let _ = writeln!(
            file,
            "{},{},{},{},{:.1}",
            record.time,
            record.callsign.as_deref().unwrap_or(""),
            record.registration.as_deref().unwrap_or(""),
            record.type_code.as_deref().unwrap_or(""),
            record.max_elevation_deg,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_header_and_a_row() {
        let path = std::env::temp_dir().join(format!("overflight-log-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut logger = Logger::new(&path);
        logger.append(&Record {
            time: "2026-10-09T12:00:00Z".to_string(),
            callsign: Some("DLH4AB".to_string()),
            registration: Some("D-AIZZ".to_string()),
            type_code: Some("A20N".to_string()),
            max_elevation_deg: 72.4,
        });

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("time,callsign,registration,type,max_elevation_deg\n"));
        assert!(text.contains("2026-10-09T12:00:00Z,DLH4AB,D-AIZZ,A20N,72.4"));

        let _ = std::fs::remove_file(&path);
    }
}
