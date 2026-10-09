//! Append-only spotter's logbook.
//!
//! Writes one CSV row per aircraft when it leaves the sky (and for whatever is
//! still up when you quit): the time, its identity, the highest elevation it
//! reached and how close it came. The format is part of the 1.0 stability
//! promise, so new files start with a version comment and the exact header.

use std::borrow::Cow;
use std::io::Write;
use std::path::PathBuf;

/// The format version written as the first line of a new file.
pub const FORMAT_VERSION: &str = "# overflight logbook v1";
/// The CSV header, pinned by a test.
pub const HEADER: &str = "time,hex,callsign,registration,type,max_elevation_deg,closest_km";

/// One logged aircraft.
#[derive(Debug, Clone)]
pub struct Record {
    pub time: String,
    /// ICAO 24-bit hex address.
    pub hex: String,
    pub callsign: Option<String>,
    pub registration: Option<String>,
    pub type_code: Option<String>,
    pub max_elevation_deg: f64,
    /// The closest this aircraft came, in kilometres.
    pub closest_km: f64,
}

/// A field, quoted only when it needs to be, so most rows stay readable.
fn csv_field(value: &str) -> Cow<'_, str> {
    if value.contains([',', '"', '\n', '\r']) {
        Cow::Owned(format!("\"{}\"", value.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(value)
    }
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
            let _ = writeln!(file, "{FORMAT_VERSION}");
            let _ = writeln!(file, "{HEADER}");
        }
        self.header_written = true;
        let _ = writeln!(
            file,
            "{},{},{},{},{},{:.1},{:.1}",
            csv_field(&record.time),
            csv_field(&record.hex),
            csv_field(record.callsign.as_deref().unwrap_or("")),
            csv_field(record.registration.as_deref().unwrap_or("")),
            csv_field(record.type_code.as_deref().unwrap_or("")),
            record.max_elevation_deg,
            record.closest_km,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Record {
        Record {
            time: "2026-10-09T12:00:00Z".to_string(),
            hex: "3c675a".to_string(),
            callsign: Some("DLH4AB".to_string()),
            registration: Some("D-AIZZ".to_string()),
            type_code: Some("A20N".to_string()),
            max_elevation_deg: 72.4,
            closest_km: 3.2,
        }
    }

    #[test]
    fn writes_the_version_and_pinned_header_then_a_row() {
        let path = std::env::temp_dir().join(format!("overflight-log-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut logger = Logger::new(&path);
        logger.append(&record());

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("# overflight logbook v1\ntime,hex,callsign,registration,type,max_elevation_deg,closest_km\n"),
            "{text}"
        );
        assert!(
            text.contains("2026-10-09T12:00:00Z,3c675a,DLH4AB,D-AIZZ,A20N,72.4,3.2"),
            "{text}"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn quotes_fields_that_contain_commas_or_quotes() {
        let path = std::env::temp_dir().join(format!("overflight-logq-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let mut logger = Logger::new(&path);
        logger.append(&Record {
            callsign: Some("A,B".to_string()),
            registration: Some("he said \"hi\"".to_string()),
            ..record()
        });

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"A,B\""), "{text}");
        assert!(text.contains("\"he said \"\"hi\"\"\""), "{text}");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_second_open_does_not_repeat_the_header() {
        let path = std::env::temp_dir().join(format!("overflight-logh-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);

        Logger::new(&path).append(&record());
        Logger::new(&path).append(&record());

        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("time,hex,callsign").count(), 1, "{text}");

        let _ = std::fs::remove_file(&path);
    }
}
