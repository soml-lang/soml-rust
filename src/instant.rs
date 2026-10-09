use crate::Error;
use crate::scalar::{MAX_DIAGNOSED_LENGTH, has_date_prefix};
use serde_core::de::{Deserialize, Deserializer};
use serde_core::ser::{Serialize, Serializer};
use std::fmt::{self, Display, Write};
use std::str::FromStr;
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

/**
A SOML instant: a point in time with nanosecond precision, from `0001-01-01T00:00:00Z` to `9999-12-31T23:59:59.999999999Z`.

This is a UTC timestamp, not a monotonic clock like `std::time::Instant`, so it is meant to be used qualified, as `soml::Instant`.

The offset a document writes an instant with is not part of the value, so `2026-09-19T21:00:00+07:00` and `2026-09-19T14:00:00Z` are the same instant. `Display` writes the canonical form, which is always UTC.

```rust
let instant: soml::Instant = "2026-09-19T21:00:00.5+07:00".parse()?;

assert_eq!(instant.to_string(), "2026-09-19T14:00:00.5Z");
assert_eq!(instant.unix_seconds(), 1_789_826_400);
assert_eq!(instant.nanoseconds(), 500_000_000);
# Ok::<(), soml::Error>(())
```

With serde, it reads only from a SOML instant, not from a string, and it is written as a SOML instant. Other formats, such as JSON, see its canonical text.
*/
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant {
	seconds: i64,
	nanoseconds: u32,
}

// The Unix seconds of 0001-01-01T00:00:00Z and of 9999-12-31T23:59:59Z.
const MIN_SECONDS: i64 = -62_135_596_800;
const MAX_SECONDS: i64 = 253_402_300_799;

/**
The name that tells the SOML serializer and deserializer that a newtype struct is an instant. Other formats treat a newtype struct as its content, so they see the canonical text.
*/
pub(crate) const TOKEN: &str = "$soml::Instant";

impl Instant {
	/**
	The earliest instant, `0001-01-01T00:00:00Z`.
	*/
	pub const MIN: Self = Self {
		seconds: MIN_SECONDS,
		nanoseconds: 0,
	};

	/**
	The latest instant, `9999-12-31T23:59:59.999999999Z`.
	*/
	pub const MAX: Self = Self {
		seconds: MAX_SECONDS,
		nanoseconds: 999_999_999,
	};

	/**
	Creates an instant from whole seconds since 1970-01-01T00:00:00Z, and nanoseconds after them.

	Returns `None` when `nanoseconds` is one billion or more, or when the instant is outside the years 0001 to 9999.

	```rust
	let instant = soml::Instant::from_unix(0, 0).unwrap();
	assert_eq!(instant.to_string(), "1970-01-01T00:00:00Z");

	assert!(soml::Instant::from_unix(-62_135_596_801, 0).is_none());
	```
	*/
	#[must_use]
	pub const fn from_unix(seconds: i64, nanoseconds: u32) -> Option<Self> {
		if nanoseconds >= 1_000_000_000 || seconds < MIN_SECONDS || seconds > MAX_SECONDS {
			return None;
		}

		Some(Self {
			seconds,
			nanoseconds,
		})
	}

	/**
	Creates an instant from nanoseconds since 1970-01-01T00:00:00Z. Returns `None` when the instant is outside the years 0001 to 9999.

	```rust
	let instant = soml::Instant::from_unix_nanoseconds(1_500_000_000).unwrap();
	assert_eq!(instant.to_string(), "1970-01-01T00:00:01.5Z");
	```
	*/
	#[must_use]
	pub fn from_unix_nanoseconds(nanoseconds: i128) -> Option<Self> {
		// Euclidean division keeps the nanoseconds from 0 to 999,999,999 for an instant before 1970, as `from_unix` requires, so the cast is lossless.
		let seconds = i64::try_from(nanoseconds.div_euclid(1_000_000_000)).ok()?;
		Self::from_unix(seconds, nanoseconds.rem_euclid(1_000_000_000) as u32)
	}

	/**
	Nanoseconds since 1970-01-01T00:00:00Z. Negative for an instant before 1970.
	*/
	#[must_use]
	pub const fn unix_nanoseconds(self) -> i128 {
		self.seconds as i128 * 1_000_000_000 + self.nanoseconds as i128
	}

	/**
	Whole seconds since 1970-01-01T00:00:00Z. Negative for an instant before 1970, which is then made later by `nanoseconds()`.
	*/
	#[must_use]
	pub const fn unix_seconds(self) -> i64 {
		self.seconds
	}

	/**
	The nanoseconds after `unix_seconds()`, from 0 to 999,999,999.
	*/
	#[must_use]
	pub const fn nanoseconds(self) -> u32 {
		self.nanoseconds
	}

	/**
	Reads an instant in SOML syntax, with the reason it is invalid as the error.
	*/
	pub(crate) fn parse(text: &str) -> Result<Self, String> {
		let Some(parts) = Parts::scan(text) else {
			return Err(describe_bad_instant(text, "", false, None));
		};

		let reason =
			|reason: &str| format!("Invalid instant {}: {reason}", crate::abbreviate(text, 40));

		if parts.fraction.len() > 9 {
			return Err(reason("a fractional second has at most nine digits"));
		}

		// The year has four digits, so it cannot be more than 9999.
		if parts.year == 0 {
			return Err(reason("the year must be 0001 to 9999"));
		}

		if !(1..=12).contains(&parts.month) {
			return Err(reason("the month must be 01 to 12"));
		}

		let last_day = days_in_month(parts.year, parts.month);

		if parts.day < 1 || parts.day > last_day {
			return Err(reason(&format!(
				"the day must be 01 to {last_day} in that month"
			)));
		}

		if parts.hour > 23 {
			return Err(reason("the hour must be 00 to 23"));
		}

		if parts.minute > 59 {
			return Err(reason("the minute must be 00 to 59"));
		}

		if parts.second > 59 {
			return Err(reason(
				"the second must be 00 to 59, and a leap second is not representable",
			));
		}

		if let Some((sign, hour, minute)) = parts.offset {
			if hour > 23 {
				return Err(reason("the offset hour must be 00 to 23"));
			}

			if minute > 59 {
				return Err(reason("the offset minute must be 00 to 59"));
			}

			if sign < 0 && hour == 0 && minute == 0 {
				return Err(reason(
					"-00:00 means “offset unknown” in RFC 3339, which is not representable; use Z or +00:00",
				));
			}
		}

		let offset_seconds = parts.offset.map_or(0, |(sign, hour, minute)| {
			sign * (i64::from(hour) * 3600 + i64::from(minute) * 60)
		});
		let seconds = days_from_civil(i64::from(parts.year), parts.month, parts.day) * 86_400
			+ i64::from(parts.hour) * 3600
			+ i64::from(parts.minute) * 60
			+ i64::from(parts.second)
			- offset_seconds;

		let mut nanoseconds = 0;

		// The length check above keeps `index` at 8 or less, so the exponent does not underflow.
		for (index, digit) in parts.fraction.bytes().enumerate() {
			nanoseconds += u32::from(digit - b'0') * 10u32.pow(8 - index as u32);
		}

		Self::from_unix(seconds, nanoseconds)
			.ok_or_else(|| reason("in UTC it falls outside the years 0001 to 9999"))
	}

	/**
	The date and time in UTC: year, month, day, hour, minute, and second.
	*/
	fn components(self) -> (i64, u32, u32, u32, u32, u32) {
		let days = self.seconds.div_euclid(86_400);
		let second_of_day = self.seconds.rem_euclid(86_400) as u32;
		let (year, month, day) = civil_from_days(days);
		(
			year,
			month,
			day,
			second_of_day / 3600,
			second_of_day / 60 % 60,
			second_of_day % 60,
		)
	}
}

/**
The parts of an instant's text, before any range check. The text must match `YYYY-MM-DDTHH:MM:SS[.fraction](Z|±HH:MM)` exactly.
*/
struct Parts<'a> {
	year: u32,
	month: u32,
	day: u32,
	hour: u32,
	minute: u32,
	second: u32,
	fraction: &'a str,
	offset: Option<(i64, u32, u32)>,
}

impl<'a> Parts<'a> {
	/**
	The parts, or `None` when the text does not have the form of an instant. A huge text, which only a huge fraction can make, is not read as one, as in the JS reference, so its error is the general one.
	*/
	fn scan(text: &'a str) -> Option<Self> {
		let bytes = text.as_bytes();

		// The shortest instant, `YYYY-MM-DDTHH:MM:SSZ`, has 20 bytes, so the indexes up to 19 below are in bounds.
		if bytes.len() < 20
			|| bytes.len() > MAX_DIAGNOSED_LENGTH
			|| bytes[4] != b'-'
			|| bytes[7] != b'-'
			|| bytes[10] != b'T'
			|| bytes[13] != b':'
			|| bytes[16] != b':'
		{
			return None;
		}

		let mut index = 19;
		let mut fraction = "";

		if bytes[index] == b'.' {
			let start = index + 1;
			index = start;

			while index < bytes.len() && bytes[index].is_ascii_digit() {
				index += 1;
			}

			if index == start {
				return None;
			}

			fraction = &text[start..index];
		}

		let offset = match &bytes[index..] {
			[b'Z'] => None,
			[sign @ (b'+' | b'-'), hour @ .., b':', _, _] if hour.len() == 2 => {
				let sign = if *sign == b'-' { -1 } else { 1 };
				Some((
					sign,
					digits(&bytes[index + 1..index + 3])?,
					digits(&bytes[index + 4..index + 6])?,
				))
			}
			_ => return None,
		};

		Some(Self {
			year: digits(&bytes[0..4])?,
			month: digits(&bytes[5..7])?,
			day: digits(&bytes[8..10])?,
			hour: digits(&bytes[11..13])?,
			minute: digits(&bytes[14..16])?,
			second: digits(&bytes[17..19])?,
			fraction,
			offset,
		})
	}
}

/**
The value of a run of ASCII digits, or `None` if any byte is not a digit.
*/
fn digits(bytes: &[u8]) -> Option<u32> {
	bytes.iter().try_fold(0, |value, &byte| {
		byte.is_ascii_digit()
			.then(|| value * 10 + u32::from(byte - b'0'))
	})
}

/**
The reason `text` does not have the form of an instant, or `None` when it has that form, whatever its values. `time` is the token after a space that follows `text` in a document, or an empty string, `is_whole_value` is whether nothing that may be part of the instant follows `text` there, and `offset` is an offset after a space that follows `text` there, or `None`.
*/
pub(crate) fn describe_malformed_instant(
	text: &str,
	time: &str,
	is_whole_value: bool,
	offset: Option<&str>,
) -> Option<String> {
	Parts::scan(text)
		.is_none()
		.then(|| describe_bad_instant(text, time, is_whole_value, offset))
}

const INSTANT_FORMAT: &str = "An instant is written as 2026-09-19T14:00:00Z, with an optional fraction of up to nine digits and an offset of Z or ±HH:MM";

fn describe_bad_instant(
	text: &str,
	time: &str,
	is_whole_value: bool,
	offset: Option<&str>,
) -> String {
	let bytes = text.as_bytes();
	let general = || {
		format!(
			"Invalid instant “{}”. {INSTANT_FORMAT}",
			crate::abbreviate(text, 40)
		)
	};

	if text.len() > MAX_DIAGNOSED_LENGTH {
		return general();
	}

	if bytes.len() == 10 && has_date_prefix(bytes) {
		let time_bytes = time.as_bytes();
		let is_time = time_bytes.len() >= 5
			&& time_bytes[..2].iter().all(u8::is_ascii_digit)
			&& time_bytes[2] == b':'
			&& time_bytes[3..5].iter().all(u8::is_ascii_digit);

		return if is_time {
			describe_space_separated_instant(text, time, is_whole_value)
		} else {
			describe_date(text)
		};
	}

	if bytes.len() > 10 && has_date_prefix(bytes) && bytes[10] == b't' {
		return "The date and time separator in an instant is an uppercase “T”".to_owned();
	}

	if text.ends_with('z') {
		return "The UTC offset in an instant is an uppercase “Z”".to_owned();
	}

	if let [.., b'+' | b'-', a, b, c, d] = bytes
		&& [a, b, c, d].iter().all(|byte| byte.is_ascii_digit())
	{
		return "An instant's offset is written with a colon, as in +07:00".to_owned();
	}

	if !is_local_date_time(bytes) {
		return general();
	}

	if let Some(offset) = offset {
		// The instant has the offset it was meant in, so it is not a local time to write as a string.
		let reason = "An instant's offset follows its time directly, without a space";
		let instant = format!("{text}{offset}");

		return if crate::parse::is_valid_value(&instant) {
			format!("{reason}, as in {}", crate::abbreviate(&instant, 40))
		} else {
			reason.to_owned()
		};
	}

	if is_whole_value {
		format!(
			"An instant needs an offset: Z or ±HH:MM. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '{}', or add the offset it was meant in",
			crate::abbreviate(text, 40)
		)
	} else {
		"An instant needs an offset: Z or ±HH:MM".to_owned()
	}
}

/**
Whether `text` is a date and a time with an optional fraction, but no offset.
*/
fn is_local_date_time(bytes: &[u8]) -> bool {
	let has_time = bytes.len() >= 19
		&& has_date_prefix(bytes)
		&& bytes[10] == b'T'
		&& digits(&bytes[11..13]).is_some()
		&& bytes[13] == b':'
		&& digits(&bytes[14..16]).is_some()
		&& bytes[16] == b':'
		&& digits(&bytes[17..19]).is_some();
	let rest = bytes.get(19..).unwrap_or_default();

	has_time
		&& (rest.is_empty()
			|| (rest.len() > 1 && rest[0] == b'.' && rest[1..].iter().all(u8::is_ascii_digit)))
}

/**
A date alone, which is not an instant. The instant it could be is only shown when the date exists, so that the example is valid.
*/
fn describe_date(date: &str) -> String {
	let bytes = date.as_bytes();
	// The caller checked `has_date_prefix`, so these are digits and the defaults are never used.
	let (year, month, day) = (
		digits(&bytes[0..4]).unwrap_or_default(),
		digits(&bytes[5..7]).unwrap_or_default(),
		digits(&bytes[8..10]).unwrap_or_default(),
	);
	let is_existing =
		year >= 1 && (1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month);
	let instant = if is_existing {
		format!(". An instant needs a time and an offset, as in {date}T00:00:00Z")
	} else {
		String::new()
	};

	format!("{date} is a date, not an instant. Write a date as a string, as in '{date}'{instant}")
}

/**
A date and a time with a space between them, where an instant has a “T”. Following the example must not turn a local time into UTC silently, so a time without an offset is described as what it is. An instant is only shown when it is valid, so a date that does not exist, a time out of range, or an instant outside the years 0001 to 9999 in UTC gets the general format instead.
*/
fn describe_space_separated_instant(date: &str, time: &str, is_whole_value: bool) -> String {
	const SEPARATOR: &str =
		"The date and time separator in an instant is an uppercase “T”, not a space";
	let instant = format!("{date}T{time}");

	if instant.len() > MAX_DIAGNOSED_LENGTH {
		return format!("{SEPARATOR}. {INSTANT_FORMAT}");
	}

	if crate::parse::is_valid_value(&instant) {
		return format!("{SEPARATOR}, as in {}", crate::abbreviate(&instant, 40));
	}

	if is_whole_value
		&& is_local_date_time(instant.as_bytes())
		&& crate::parse::is_valid_value(&format!("{instant}Z"))
	{
		return format!(
			"{SEPARATOR}, and an instant needs the offset it was meant in, as in {}Z for UTC. A date and time without an offset is a local time, which is not an instant, so write it as a string, as in '{}'",
			crate::abbreviate(&instant, 40),
			crate::abbreviate(&format!("{date} {time}"), 40)
		);
	}

	format!("{SEPARATOR}. {INSTANT_FORMAT}")
}

const fn is_leap_year(year: u32) -> bool {
	year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

const fn days_in_month(year: u32, month: u32) -> u32 {
	match month {
		2 if is_leap_year(year) => 29,
		2 => 28,
		4 | 6 | 9 | 11 => 30,
		_ => 31,
	}
}

/**
Days since 1970-01-01 in the proleptic Gregorian calendar. Howard Hinnant's `days_from_civil`.
*/
const fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
	// The year starts on March 1 here, so that a leap day is the last day of its year.
	let year = if month <= 2 { year - 1 } else { year };
	let era = year.div_euclid(400);
	let year_of_era = year - era * 400;
	let month_from_march = (month as i64 + 9) % 12;
	let day_of_year = (153 * month_from_march + 2) / 5 + day as i64 - 1;
	let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
	// 719,468 is the number of days from 0000-03-01 to 1970-01-01.
	era * 146_097 + day_of_era - 719_468
}

/**
The year, month, and day of a count of days since 1970-01-01. Howard Hinnant's `civil_from_days`.
*/
const fn civil_from_days(days: i64) -> (i64, u32, u32) {
	let days = days + 719_468;
	let era = days.div_euclid(146_097);
	let day_of_era = days - era * 146_097;
	let year_of_era =
		(day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
	let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
	let month_from_march = (5 * day_of_year + 2) / 153;
	let day = (day_of_year - (153 * month_from_march + 2) / 5 + 1) as u32;
	let month = if month_from_march < 10 {
		month_from_march + 3
	} else {
		month_from_march - 9
	} as u32;
	let year = year_of_era + era * 400 + if month <= 2 { 1 } else { 0 };
	(year, month, day)
}

/**
Writes the canonical form: UTC with `Z`, and a fraction without trailing zeros, left out when it is zero.
*/
impl Display for Instant {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		let (year, month, day, hour, minute, second) = self.components();
		// The digits are put in place, because `write!` is several times slower, and a large document can have thousands of instants. Each field has a fixed width, and the year is from 1 to 9999.
		let mut text = *b"0000-00-00T00:00:00";

		for (end, width, value) in [
			(4, 4, year as u32),
			(7, 2, month),
			(10, 2, day),
			(13, 2, hour),
			(16, 2, minute),
			(19, 2, second),
		] {
			put_digits(&mut text[end - width..end], value);
		}

		formatter.write_str(std::str::from_utf8(&text).expect("the digits are ASCII"))?;
		write_fraction(formatter, self.nanoseconds)?;
		formatter.write_char('Z')
	}
}

/**
Writes `value` in decimal into all of `digits`, with leading zeros.
*/
fn put_digits(digits: &mut [u8], mut value: u32) {
	for digit in digits.iter_mut().rev() {
		*digit = b'0' + (value % 10) as u8;
		value /= 10;
	}
}

/**
Writes `.` and the nanoseconds as a fraction of a second without trailing zeros, or nothing when they are zero. Instants and durations share this form.
*/
pub(crate) fn write_fraction(output: &mut impl Write, nanoseconds: u32) -> fmt::Result {
	if nanoseconds == 0 {
		return Ok(());
	}

	let mut text = *b".000000000";
	put_digits(&mut text[1..], nanoseconds);
	let end = text
		.iter()
		.rposition(|&byte| byte != b'0')
		.expect("the nanoseconds are not zero")
		+ 1;

	output.write_str(std::str::from_utf8(&text[..end]).expect("the digits are ASCII"))
}

impl fmt::Debug for Instant {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "Instant({self})")
	}
}

/**
Reads an instant in SOML syntax, with any offset.

```rust
let instant: soml::Instant = "2026-09-19T14:00:00Z".parse()?;
assert_eq!(instant.unix_seconds(), 1_789_826_400);

assert!("2026-09-19".parse::<soml::Instant>().is_err());
# Ok::<(), soml::Error>(())
```
*/
impl FromStr for Instant {
	type Err = Error;

	fn from_str(text: &str) -> Result<Self, Error> {
		Self::parse(text).map_err(Error::syntax)
	}
}

impl TryFrom<SystemTime> for Instant {
	type Error = Error;

	/**
	Fails when the time is outside the years 0001 to 9999.
	*/
	fn try_from(time: SystemTime) -> Result<Self, Error> {
		let instant = match time.duration_since(UNIX_EPOCH) {
			Ok(after) => i64::try_from(after.as_secs())
				.ok()
				.and_then(|seconds| Self::from_unix(seconds, after.subsec_nanos())),
			Err(error) => {
				let before = error.duration();
				let nanoseconds = before.subsec_nanos();
				// The nanoseconds of an `Instant` count forward from its second, so a fraction before 1970 borrows one second.
				i64::try_from(before.as_secs()).ok().and_then(|seconds| {
					if nanoseconds == 0 {
						Self::from_unix(-seconds, 0)
					} else {
						Self::from_unix(-seconds - 1, 1_000_000_000 - nanoseconds)
					}
				})
			}
		};

		instant.ok_or_else(|| Error::data("The time is outside the years 0001 to 9999"))
	}
}

impl TryFrom<Instant> for SystemTime {
	type Error = Error;

	/**
	Fails when the platform's `SystemTime` cannot hold the instant. On Windows, for example, it cannot go before the year 1601.
	*/
	fn try_from(instant: Instant) -> Result<Self, Error> {
		let time = if instant.seconds >= 0 {
			UNIX_EPOCH.checked_add(StdDuration::new(
				instant.seconds.unsigned_abs(),
				instant.nanoseconds,
			))
		} else {
			// The nanoseconds count forward from the whole second, so they are added after the seconds are subtracted.
			UNIX_EPOCH
				.checked_sub(StdDuration::from_secs(instant.seconds.unsigned_abs()))
				.and_then(|time| {
					time.checked_add(StdDuration::from_nanos(u64::from(instant.nanoseconds)))
				})
		};

		time.ok_or_else(|| {
			Error::data(format!(
				"The instant {instant} is outside the range of SystemTime on this platform"
			))
		})
	}
}

impl Serialize for Instant {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_newtype_struct(TOKEN, &crate::value::CanonicalText(self))
	}
}

impl<'de> Deserialize<'de> for Instant {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		deserializer.deserialize_newtype_struct(
			TOKEN,
			crate::value::TextVisitor {
				expecting: "an instant",
				parse: Self::parse,
			},
		)
	}
}
