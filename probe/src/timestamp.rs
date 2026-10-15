use std::time::{SystemTime, UNIX_EPOCH};

const SECONDS_PER_DAY: u64 = 86_400;
const SECONDS_PER_HOUR: u64 = 3_600;
const SECONDS_PER_MINUTE: u64 = 60;
const DAYS_FROM_YEAR_ZERO_TO_UNIX_EPOCH: i64 = 719_468;
const DAYS_PER_ERA: i64 = 146_097;

pub fn utc_timestamp(moment: SystemTime) -> String {
    let seconds = moment.duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or(0);
    let (year, month, day) = civil_date_from_days((seconds / SECONDS_PER_DAY) as i64);
    let seconds_of_day = seconds % SECONDS_PER_DAY;
    let hour = seconds_of_day / SECONDS_PER_HOUR;
    let minute = seconds_of_day % SECONDS_PER_HOUR / SECONDS_PER_MINUTE;
    let second = seconds_of_day % SECONDS_PER_MINUTE;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_date_from_days(days_since_unix_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_unix_epoch + DAYS_FROM_YEAR_ZERO_TO_UNIX_EPOCH;
    let era = days.div_euclid(DAYS_PER_ERA);
    let day_of_era = days.rem_euclid(DAYS_PER_ERA);
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn moment(seconds_since_unix_epoch: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds_since_unix_epoch)
    }

    #[test]
    fn starts_at_the_unix_epoch() {
        assert_eq!(utc_timestamp(moment(0)), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn writes_a_known_moment() {
        assert_eq!(utc_timestamp(moment(1_000_000_000)), "2001-09-09T01:46:40Z");
    }

    #[test]
    fn handles_a_leap_day() {
        assert_eq!(utc_timestamp(moment(951_782_400)), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn handles_the_last_second_of_a_year() {
        assert_eq!(utc_timestamp(moment(1_798_761_599)), "2026-12-31T23:59:59Z");
    }
}
