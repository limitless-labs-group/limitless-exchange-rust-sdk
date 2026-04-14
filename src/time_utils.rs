pub(crate) fn chrono_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();

    let datetime = chrono_like::DateTime::from_unix(secs as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        datetime.year,
        datetime.month,
        datetime.day,
        datetime.hour,
        datetime.minute,
        datetime.second,
        millis
    )
}

mod chrono_like {
    pub struct DateTime {
        pub year: i32,
        pub month: u32,
        pub day: u32,
        pub hour: u32,
        pub minute: u32,
        pub second: u32,
    }

    impl DateTime {
        pub fn from_unix(timestamp: i64) -> Self {
            let secs_per_day = 86_400;
            let days = timestamp.div_euclid(secs_per_day);
            let secs_of_day = timestamp.rem_euclid(secs_per_day);

            let (year, month, day) = civil_from_days(days);
            Self {
                year,
                month,
                day,
                hour: (secs_of_day / 3600) as u32,
                minute: ((secs_of_day % 3600) / 60) as u32,
                second: (secs_of_day % 60) as u32,
            }
        }
    }

    fn civil_from_days(days: i64) -> (i32, u32, u32) {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = mp + if mp < 10 { 3 } else { -9 };
        let year = y + if m <= 2 { 1 } else { 0 };
        (year as i32, m as u32, d as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::chrono_like::DateTime;
    use super::chrono_timestamp;

    #[test]
    fn date_time_handles_leap_day() {
        let value = DateTime::from_unix(1_709_164_800);
        assert_eq!(value.year, 2024);
        assert_eq!(value.month, 2);
        assert_eq!(value.day, 29);
    }

    #[test]
    fn date_time_handles_century_boundary() {
        let value = DateTime::from_unix(4_102_444_800);
        assert_eq!(value.year, 2100);
        assert_eq!(value.month, 1);
        assert_eq!(value.day, 1);
    }

    #[test]
    fn timestamp_is_iso_8601_utc_like() {
        let value = chrono_timestamp();
        assert!(value.ends_with('Z'));
        assert_eq!(value.len(), 24);
        assert_eq!(&value[4..5], "-");
        assert_eq!(&value[7..8], "-");
        assert_eq!(&value[10..11], "T");
    }
}
