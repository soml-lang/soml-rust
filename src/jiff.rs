/*!
Conversions between jiff's types and SOML's, and modules for `#[serde(with)]`, with the `jiff` feature.

jiff's `Timestamp` and `SignedDuration` read SOML instants and durations without this, because they ask for text. But they write themselves as strings, so a document gets a quoted `'2026-09-19T14:00:00Z'` instead of an instant. A `civil::Date` needs its module in both directions, because SOML keeps a date distinct from a string. The modules here write native values:

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct Deploy {
	#[serde(with = "soml::jiff::timestamp")]
	at: jiff::Timestamp,
	#[serde(with = "soml::jiff::signed_duration")]
	took: jiff::SignedDuration,
	#[serde(with = "soml::jiff::timestamp::option", default)]
	finished: Option<jiff::Timestamp>,
}

let deploy: Deploy = soml::from_str("at: 2026-09-19T14:00:00Z\ntook: 1m30s")?;
assert_eq!(soml::to_string(&deploy)?, "at: 2026-09-19T14:00:00Z\ntook: 1m30s\nfinished: null\n");
# Ok::<(), soml::Error>(())
```

jiff's timestamps end at `9999-12-30T22:00:00.999999999Z`, about a day before SOML's, so a later instant cannot be converted.
*/

use crate::{Date, Duration, Error, Instant};
use ::jiff::{SignedDuration, Timestamp};

impl TryFrom<Timestamp> for Instant {
	type Error = Error;

	/**
	Fails when the timestamp is outside the years 0001 to 9999.
	*/
	fn try_from(timestamp: Timestamp) -> Result<Self, Error> {
		Self::from_unix_nanoseconds(timestamp.as_nanosecond()).ok_or_else(|| {
			Error::data(format!(
				"The timestamp {timestamp} is outside the years 0001 to 9999"
			))
		})
	}
}

impl TryFrom<Instant> for Timestamp {
	type Error = Error;

	/**
	Fails for an instant after `9999-12-30T22:00:00.999999999Z`, where jiff's range ends.
	*/
	fn try_from(instant: Instant) -> Result<Self, Error> {
		// jiff adds the nanoseconds to the seconds whatever their signs, as `Instant` does, and nanoseconds below one billion always fit an `i32`.
		Self::new(instant.unix_seconds(), instant.nanoseconds() as i32).map_err(|error| {
			Error::data(format!(
				"The instant {instant} is outside the range of a jiff timestamp: {error}"
			))
		})
	}
}

impl TryFrom<SignedDuration> for Duration {
	type Error = Error;

	/**
	Fails when the duration is longer than `i64::MAX` nanoseconds, about 292 years either way.
	*/
	fn try_from(duration: SignedDuration) -> Result<Self, Error> {
		i64::try_from(duration.as_nanos())
			.map(Self::from_nanoseconds)
			.map_err(|_| {
				Error::data(format!(
					"The duration {duration:?} is outside the SOML range of about 292 years"
				))
			})
	}
}

impl From<Duration> for SignedDuration {
	fn from(duration: Duration) -> Self {
		Self::from_nanos(duration.nanoseconds())
	}
}

serde_with_module! {
	/**
	For a `jiff::Timestamp` field: `#[serde(with = "soml::jiff::timestamp")]`.
	*/
	timestamp, ::jiff::Timestamp, crate::Instant
}

serde_with_module! {
	/**
	For a `jiff::SignedDuration` field: `#[serde(with = "soml::jiff::signed_duration")]`.
	*/
	signed_duration, ::jiff::SignedDuration, crate::Duration
}

impl TryFrom<::jiff::civil::Date> for Date {
	type Error = Error;

	/**
	Fails when the native date's year is outside 1 through 9999. Components are converted directly, without a time or timezone.
	*/
	fn try_from(date: ::jiff::civil::Date) -> Result<Self, Error> {
		u16::try_from(date.year())
			.ok()
			.and_then(|year| Self::new(year, date.month() as u8, date.day() as u8))
			.ok_or_else(|| {
				Error::data(format!("The date {date} is outside the years 0001 to 9999"))
			})
	}
}

impl From<Date> for ::jiff::civil::Date {
	fn from(date: Date) -> Self {
		Self::new(date.year() as i16, date.month() as i8, date.day() as i8)
			.expect("the native Gregorian calendar holds every SOML date")
	}
}

serde_with_module! {
	/**
	For a `::jiff::civil::Date` field: `#[serde(with = "soml::jiff::date")]`. Preserves the native date type and components in both directions.
	*/
	date, ::jiff::civil::Date, crate::Date
}
