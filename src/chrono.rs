/*!
Conversions between chrono's types and SOML's, and modules for `#[serde(with)]`, with the `chrono` feature.

chrono's `DateTime<Utc>` reads SOML instants without this, because it asks for text. But it writes itself as a string, so a document gets a quoted `'2026-09-19T14:00:00Z'` instead of an instant. `TimeDelta` does not ask for text, so it reads a duration only through its module. `NaiveDate` needs its module in both directions, because SOML keeps a date distinct from a string. The modules here write native values:

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct Deploy {
	#[serde(with = "soml::chrono::date_time")]
	at: chrono::DateTime<chrono::Utc>,
	#[serde(with = "soml::chrono::time_delta")]
	took: chrono::TimeDelta,
}

let deploy: Deploy = soml::from_str("at: 2026-09-19T14:00:00Z\ntook: 1m30s")?;
assert_eq!(soml::to_string(&deploy)?, "at: 2026-09-19T14:00:00Z\ntook: 1m30s\n");
# Ok::<(), soml::Error>(())
```
*/

use crate::{Date, Duration, Error, Instant};
use ::chrono::{DateTime, Datelike, TimeDelta, Utc};

impl TryFrom<DateTime<Utc>> for Instant {
	type Error = Error;

	/**
	Fails when the time is outside the years 0001 to 9999, or is a leap second.
	*/
	fn try_from(time: DateTime<Utc>) -> Result<Self, Error> {
		// chrono writes a leap second as nanoseconds past one billion, which SOML cannot represent.
		let nanoseconds = time.timestamp_subsec_nanos();
		if nanoseconds >= 1_000_000_000 {
			return Err(Error::data(format!(
				"The time {time} is a leap second, which SOML cannot represent"
			)));
		}

		// chrono, like `Instant`, counts the nanoseconds forward from the whole second, so a time before 1970 converts directly.
		Self::from_unix(time.timestamp(), nanoseconds).ok_or_else(|| {
			Error::data(format!("The time {time} is outside the years 0001 to 9999"))
		})
	}
}

impl From<Instant> for DateTime<Utc> {
	fn from(instant: Instant) -> Self {
		Self::from_timestamp(instant.unix_seconds(), instant.nanoseconds())
			.expect("chrono's range holds every SOML instant")
	}
}

impl TryFrom<TimeDelta> for Duration {
	type Error = Error;

	/**
	Fails when the duration is longer than `i64::MAX` nanoseconds, about 292 years either way.
	*/
	fn try_from(delta: TimeDelta) -> Result<Self, Error> {
		delta
			.num_nanoseconds()
			.map(Self::from_nanoseconds)
			.ok_or_else(|| {
				Error::data(format!(
					"The duration {delta} is outside the SOML range of about 292 years"
				))
			})
	}
}

impl From<Duration> for TimeDelta {
	fn from(duration: Duration) -> Self {
		Self::nanoseconds(duration.nanoseconds())
	}
}

serde_with_module! {
	/**
	For a `chrono::DateTime<Utc>` field: `#[serde(with = "soml::chrono::date_time")]`.
	*/
	date_time, ::chrono::DateTime<::chrono::Utc>, crate::Instant
}

serde_with_module! {
	/**
	For a `chrono::TimeDelta` field: `#[serde(with = "soml::chrono::time_delta")]`.
	*/
	time_delta, ::chrono::TimeDelta, crate::Duration
}

impl TryFrom<::chrono::NaiveDate> for Date {
	type Error = Error;

	/**
	Fails when the native date's year is outside 1 through 9999. Components are converted directly, without a time or timezone.
	*/
	fn try_from(date: ::chrono::NaiveDate) -> Result<Self, Error> {
		u16::try_from(date.year())
			.ok()
			.and_then(|year| Self::new(year, date.month() as u8, date.day() as u8))
			.ok_or_else(|| {
				Error::data(format!("The date {date} is outside the years 0001 to 9999"))
			})
	}
}

impl From<Date> for ::chrono::NaiveDate {
	fn from(date: Date) -> Self {
		Self::from_ymd_opt(
			i32::from(date.year()),
			u32::from(date.month()),
			u32::from(date.day()),
		)
		.expect("the native Gregorian calendar holds every SOML date")
	}
}

serde_with_module! {
	/**
	For a `::chrono::NaiveDate` field: `#[serde(with = "soml::chrono::naive_date")]`. Preserves the native date type and components in both directions.
	*/
	naive_date, ::chrono::NaiveDate, crate::Date
}
