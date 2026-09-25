//! Running the measurements, and saying what they mean.
//!
//! Separated from the measuring itself so that `bench.rs` stays about one
//! entry -- start it, read it, stop it -- while the loop over a catalog, the
//! table, and the estimates a reading supports live where a reader looking for
//! output goes to find them.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::time::{self, SystemTime};

use super::Measurement;
use crate::catalog::Catalog;
use crate::launch::{Server, models_root};

/// Loads each entry in turn, measures it, and prints what it found.
///
/// One at a time, and never two. The number wanted is what a single entry
/// costs; two resident at once would attribute one model's pages to the
/// other. It is also why a provisional estimate cannot cause an
/// out-of-memory failure during the very run that exists to correct it.
///
/// # Errors
///
/// Returns a complaint when the catalog cannot be read or parsed, when there
/// is nowhere to resolve its locations against, when no server binary can be
/// found, or when a requested entry is not in the catalog. A single entry
/// that fails to load is reported in its row and does not stop the rest.
pub fn command(path: &Path, only: Option<&str>) -> Result<(), String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let parsed = Catalog::parse(&text).map_err(|report| format!("{report}"))?;
    let root = models_root().map_err(|failure| failure.to_string())?;
    let server = Server::located(None).map_err(|failure| failure.to_string())?;

    let wanted: Vec<_> = parsed
        .entries
        .iter()
        .filter(|entry| only.is_none_or(|id| entry.id == id))
        .collect();
    if wanted.is_empty() {
        return Err(format!(
            "no entry called '{}' in {}",
            only.unwrap_or_default(),
            path.display()
        ));
    }

    println!(
        "{:<20} {:>9} {:>9} {:>8} {:>10}",
        "entry", "declared", "measured", "load", "rate"
    );

    let mut measured = Vec::new();
    for entry in wanted {
        // Printed before the load, because a large model is minutes and a
        // silent terminal looks like a hang.
        print!("{:<20} {:>9} ", entry.id, entry.memory_estimate_mib);
        let _ = io::stdout().flush();

        match super::entry(&server, entry, &root) {
            Ok(reading) => {
                println!(
                    "{:>9} {:>7.1}s {:>10}",
                    reading
                        .measured_mib
                        .map_or_else(|| "--".to_owned(), |mib| mib.to_string()),
                    reading.load.as_secs_f64(),
                    reading.throughput.map_or_else(
                        || "--".to_owned(),
                        |rate| {
                            let (figure, unit) = rate.parts();
                            format!("{figure:.1} {unit}")
                        }
                    )
                );
                measured.push(reading);
            }
            Err(failure) => println!("{:>9} {:>8} {:>10}  {failure}", "--", "--", "--"),
        }
    }

    recommendations(&measured);
    Ok(())
}

/// What to put in the catalog, for a person to paste.
///
/// Deliberately not written back: the shipped catalog's comments carry the
/// reasoning for the numbers beside them, and the more valuable half of that
/// file is the half a naive writer would destroy.
fn recommendations(measured: &[Measurement]) {
    let corrections: Vec<_> = measured
        .iter()
        .filter_map(|reading| Some((reading, reading.recommended_mib()?)))
        .filter(|(reading, wants)| *wants != reading.declared_mib)
        .collect();

    if corrections.is_empty() {
        return;
    }

    println!("\nestimates these measurements support:\n");
    for (reading, wants) in corrections {
        let measured_mib = reading.measured_mib.unwrap_or_default();
        println!("[models.{}]", reading.id);
        println!(
            "# Measured {measured_mib} MiB resident on {}; a twentieth over,\n\
             # to a quarter gibibyte. Was {} MiB.",
            today(),
            reading.declared_mib
        );
        println!("memory_estimate_mib = {wants}\n");
    }
}

/// Today, as a date a comment can carry.
///
/// A measurement is one machine on one day: the driver, the card's other
/// tenants and the context actually used all move it. Recording when it was
/// taken is what lets a later reader distrust it by the right amount.
fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    date_of(seconds)
}

/// The date a number of seconds since the Unix epoch falls on, as `YYYY-MM-DD`.
///
/// Civil-from-days, for the one date this prints. A calendar crate would be
/// a dependency bought for a comment.
fn date_of(seconds: u64) -> String {
    let (mut year, mut remaining) = (1970_u64, seconds / 86_400);
    loop {
        let length = if leap(year) { 366 } else { 365 };
        if remaining < length {
            break;
        }
        remaining -= length;
        year += 1;
    }

    let lengths = [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0;
    while remaining >= lengths[month] {
        remaining -= lengths[month];
        month += 1;
    }
    format!("{year:04}-{:02}-{:02}", month + 1, remaining + 1)
}

/// Whether a year has a twenty-ninth of February.
const fn leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leap_year_is_every_fourth_except_centuries_not_divisible_by_four_hundred() {
        assert!(leap(2024));
        assert!(leap(2000));
        assert!(!leap(2023));
        assert!(!leap(1900));
    }

    #[test]
    fn seconds_since_the_epoch_land_on_their_calendar_date() {
        // Hand-checked instants: the epoch itself, a leap day, the first day
        // after a century that was not a leap year, and a recent date.
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(951_782_400), "2000-02-29");
        assert_eq!(date_of(4_107_542_400), "2100-03-01");
        assert_eq!(date_of(1_790_208_000), "2026-09-24");
    }

    #[test]
    fn today_is_the_date_the_clock_gives() {
        let today = today();
        assert!(
            today.len() == 10 && today.as_str() >= "2026-09-24",
            "a date, and none earlier than this test: {today:?}"
        );
    }
}
