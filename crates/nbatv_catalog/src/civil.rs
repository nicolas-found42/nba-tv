//! Shared civil-date math: `YYYY-MM-DD` ⇄ days since the Unix epoch.
//!
//! Howard Hinnant's `days_from_civil` / `civil_from_days` pair, std-only
//! (no date dependency). One copy for the whole crate: the sweep's rescan
//! windows and the runner's default `--now` stamp parse and format dates
//! through here, so the two directions can never drift apart.

/// Days since the Unix epoch for a proleptic-Gregorian civil date
/// (Hinnant's `days_from_civil`).
pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = if month <= 2 { year - 1 } else { year };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month_prime = (month + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719_468
}

/// The civil date for days since the Unix epoch as `(year, month, day)`
/// (Hinnant's `civil_from_days`, the [`days_from_civil`] inverse).
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = (if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    }) as u32;
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_dates_map_to_their_epoch_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0, "the epoch itself");
        assert_eq!(days_from_civil(1946, 11, 1), -8_462, "pre-epoch archive");
        assert_eq!(days_from_civil(2000, 3, 1), 11_017, "post-leap-day 2000");
    }

    #[test]
    fn round_trips_cover_leap_years_and_centuries() {
        for (year, month, day) in [
            (1970i64, 1u32, 1u32),
            (1946, 11, 1),
            (1990, 6, 14),
            (2000, 2, 29), // 400-year leap day
            (2000, 3, 1),  // the day after it
            (1996, 2, 29), // ordinary leap day
            (1900, 3, 1),  // century non-leap
            (2100, 2, 28), // the next one
            (2026, 9, 8),
        ] {
            let days = days_from_civil(year, i64::from(month), i64::from(day));
            assert_eq!(
                civil_from_days(days),
                (year, month, day),
                "{year}-{month:02}-{day:02} round-trips"
            );
        }
    }

    #[test]
    fn epoch_inverse_is_the_epoch_date() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(1), (1970, 1, 2));
    }
}
