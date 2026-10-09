use crate::Error;
use std::fmt::{self, Display};
use std::str::FromStr;

/**
A calendar date in the proleptic Gregorian calendar, from `0001-01-01` to `9999-12-31`. It has no timezone, time of day, or duration, and its components never pass through an instant.

With serde, it reads only from a native SOML date, not from a string, and it is written as a native date. Other formats, including JSON, see its exact `YYYY-MM-DD` text.

```rust
let date = soml::Date::new(1582, 10, 10).unwrap();
assert_eq!(date.to_string(), "1582-10-10");
assert_eq!((date.year(), date.month(), date.day()), (1582, 10, 10));
```
*/
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
	year: u16,
	month: u8,
	day: u8,
}

/**
The name that tells the SOML serializer and deserializer that a newtype struct is a date. Other formats treat a newtype struct as its content, so they see the canonical text.
*/
pub(crate) const TOKEN: &str = "$soml::Date";

impl Date {
	/**
	Creates a date from Gregorian components. Returns `None` for a year outside 1 through 9999, or a month or day that does not exist.
	*/
	#[must_use]
	pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
		Self::calendar_error(u32::from(year), u32::from(month), u32::from(day))
			.is_none()
			.then_some(Self { year, month, day })
	}

	/**
	The year, from 1 through 9999.
	*/
	#[must_use]
	pub const fn year(self) -> u16 {
		self.year
	}

	/**
	The month, from 1 through 12.
	*/
	#[must_use]
	pub const fn month(self) -> u8 {
		self.month
	}

	/**
	The day, from 1 through the last day of its month.
	*/
	#[must_use]
	pub const fn day(self) -> u8 {
		self.day
	}

	/**
	Reads exact ASCII date text, without accepting the date prefix of a longer token.
	*/
	pub(crate) fn parse(text: &str) -> Result<Self, String> {
		let bytes = text.as_bytes();
		if bytes.len() != 10 || !crate::scalar::has_date_prefix(bytes) {
			return Err(format!(
				"Invalid date “{}”. A date is written as YYYY-MM-DD",
				crate::abbreviate(text, 40)
			));
		}
		let year = u32::from(bytes[0] - b'0') * 1000
			+ u32::from(bytes[1] - b'0') * 100
			+ u32::from(bytes[2] - b'0') * 10
			+ u32::from(bytes[3] - b'0');
		let month = u32::from(bytes[5] - b'0') * 10 + u32::from(bytes[6] - b'0');
		let day = u32::from(bytes[8] - b'0') * 10 + u32::from(bytes[9] - b'0');
		if let Some(problem) = Self::calendar_error(year, month, day) {
			return Err(format!("Invalid date {text}: {problem}"));
		}
		Ok(Self {
			year: year as u16,
			month: month as u8,
			day: day as u8,
		})
	}

	/**
	The first invalid Gregorian component, with a reason, or `None` when the date exists in the proleptic Gregorian calendar. Shared with instant validation.
	*/
	pub(crate) fn calendar_error(year: u32, month: u32, day: u32) -> Option<String> {
		if !(1..=9999).contains(&year) {
			return Some("the year must be 0001 to 9999".to_owned());
		}
		if !(1..=12).contains(&month) {
			return Some("the month must be 01 to 12".to_owned());
		}
		let last_day = Self::days_in_month(year, month);
		if day < 1 || day > last_day {
			return Some(format!("the day must be 01 to {last_day} in that month"));
		}
		None
	}

	/**
	The length of a month from 1 through 12 in the proleptic Gregorian calendar.
	*/
	const fn days_in_month(year: u32, month: u32) -> u32 {
		let is_leap_year =
			year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
		match month {
			2 if is_leap_year => 29,
			2 => 28,
			4 | 6 | 9 | 11 => 30,
			_ => 31,
		}
	}
}

/**
Writes the canonical form, `YYYY-MM-DD`.
*/
impl Display for Date {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		let mut text = *b"0000-00-00";
		crate::instant::put_digits(&mut text[..4], u32::from(self.year));
		crate::instant::put_digits(&mut text[5..7], u32::from(self.month));
		crate::instant::put_digits(&mut text[8..], u32::from(self.day));
		formatter.write_str(std::str::from_utf8(&text).expect("the digits are ASCII"))
	}
}

impl fmt::Debug for Date {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "Date({self})")
	}
}

/**
Reads an exact `YYYY-MM-DD` date, rejecting impossible components.

```rust
let date: soml::Date = "1582-10-10".parse()?;
assert_eq!(date.to_string(), "1582-10-10");

assert!("2026-02-30".parse::<soml::Date>().is_err());
# Ok::<(), soml::Error>(())
```
*/
impl FromStr for Date {
	type Err = Error;

	fn from_str(text: &str) -> Result<Self, Error> {
		Self::parse(text).map_err(Error::syntax)
	}
}

serde_temporal_scalar!(Date, "a date");
