use crate::Error;
use serde_core::de::{self, Deserialize, Deserializer, Visitor};
use serde_core::ser::{Serialize, Serializer};
use std::fmt::{self, Display};
use std::str::FromStr;
use std::time::Duration as StdDuration;

/**
A SOML duration: a signed length of time, as a whole number of nanoseconds in the `i64` range, which is about 292 years either way.

Unlike `std::time::Duration`, it can be negative, so it is meant to be used qualified, as `soml::Duration`.

`Display` writes the canonical form, in hours, minutes, and seconds with a fraction:

```rust
let duration: soml::Duration = "90m".parse()?;
assert_eq!(duration.to_string(), "1h30m");

let duration = soml::Duration::from_nanoseconds(-1_500_000_000);
assert_eq!(duration.to_string(), "-1.5s");
# Ok::<(), soml::Error>(())
```

With serde, it reads only from a SOML duration, not from a string, and it is written as a SOML duration. Other formats, such as JSON, see its canonical text.
*/
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration {
	nanoseconds: i64,
}

/**
The name that tells the SOML serializer and deserializer that a newtype struct is a duration. Other formats treat a newtype struct as its content, so they see the canonical text.
*/
pub(crate) const TOKEN: &str = "$soml::Duration";

/**
The units in the order a duration must write them, with their length in nanoseconds.
*/
const UNITS: [(&str, u64); 6] = [
	("h", 3_600_000_000_000),
	("m", 60_000_000_000),
	("s", 1_000_000_000),
	("ms", 1_000_000),
	("us", 1000),
	("ns", 1),
];

impl Duration {
	/**
	The most negative duration, `-9223372036854775808ns`.
	*/
	pub const MIN: Self = Self::from_nanoseconds(i64::MIN);

	/**
	The longest duration, `9223372036854775807ns`.
	*/
	pub const MAX: Self = Self::from_nanoseconds(i64::MAX);

	/**
	The zero duration, `0s`.
	*/
	pub const ZERO: Self = Self::from_nanoseconds(0);

	/**
	Creates a duration from a signed count of nanoseconds.
	*/
	#[must_use]
	pub const fn from_nanoseconds(nanoseconds: i64) -> Self {
		Self { nanoseconds }
	}

	/**
	The signed count of nanoseconds.
	*/
	#[must_use]
	pub const fn nanoseconds(self) -> i64 {
		self.nanoseconds
	}

	/**
	Whether the duration is less than zero.
	*/
	#[must_use]
	pub const fn is_negative(self) -> bool {
		self.nanoseconds < 0
	}

	/**
	Reads a duration in SOML syntax, with the reason it is invalid as the error.

	Every part is read before the value is checked, so a syntax error is reported before an error about the value, and it names the part it is about.
	*/
	pub(crate) fn parse(text: &str) -> Result<Self, String> {
		let fail = |reason: &str| {
			Err(format!(
				"Invalid duration {}: {reason}",
				crate::abbreviate(text, 40)
			))
		};
		let bytes = text.as_bytes();
		let is_negative = bytes.first() == Some(&b'-');
		let mut index = usize::from(is_negative);

		if index == bytes.len() {
			return fail("a duration needs at least one part, as in 30s");
		}
		let mut previous_rank = None;
		// The magnitude, which goes past the `i64` range only to be reported as out of range.
		let mut total: u128 = 0;
		// The fraction of the last part without trailing zeros, and that part's unit length. Only the last part may have one, so the value checks wait until every part is read.
		let mut fraction: &[u8] = &[];
		let mut unit_length = 0;

		while index < bytes.len() {
			let digits_end = crate::scalar::skip_integer_part(bytes, index);
			let next = bytes.get(digits_end).copied();

			if digits_end == index {
				if bytes[index] == b'-' && bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
					return fail("only the whole duration takes a sign, as in -1h30m");
				}

				if bytes[index] == b'+' {
					return fail("a “+” sign is not allowed");
				}

				return fail(&format!(
					"expected a number at “{}”",
					crate::abbreviate(&text[index..], 10)
				));
			}

			if bytes[index] == b'0'
				&& next.is_some_and(|byte| byte.is_ascii_digit() || byte == b'_')
			{
				return fail("leading zeros are not allowed");
			}

			if next == Some(b'_') {
				return fail("an underscore must be between two digits");
			}

			fraction = &[];
			let mut unit_start = digits_end;

			if next == Some(b'.') {
				unit_start = crate::scalar::skip_digits(bytes, digits_end + 1, u8::is_ascii_digit);

				if unit_start == digits_end + 1 {
					return fail("a “.” must be followed by a digit");
				}

				if bytes.get(unit_start) == Some(&b'_') {
					return fail("an underscore must be between two digits");
				}

				fraction = &bytes[digits_end + 1..unit_start];
			}

			let mut unit_end = unit_start;

			while bytes.get(unit_end).is_some_and(u8::is_ascii_alphabetic) {
				unit_end += 1;
			}

			let unit = &text[unit_start..unit_end];

			let Some(rank) = UNITS.iter().position(|(name, _)| *name == unit) else {
				return fail(&describe_bad_unit(unit, text));
			};

			if previous_rank.is_some_and(|previous| rank <= previous) {
				return fail(
					"the units are in the order h, m, s, ms, us, ns, and each appears at most once",
				);
			}

			if next == Some(b'.') && bytes.get(unit_end).is_some_and(u8::is_ascii_digit) {
				return fail("only the last part may have a fraction");
			}

			unit_length = UNITS[rank].1;
			total = total.saturating_add(
				integer_digits(&bytes[index..digits_end]).saturating_mul(u128::from(unit_length)),
			);
			index = unit_end;
			previous_rank = Some(rank);
		}

		// Trailing zeros change nothing, and leaving them out keeps the arithmetic small however many there are.
		let fraction: Vec<u8> = fraction
			.iter()
			.copied()
			.filter(|&byte| byte != b'_')
			.collect();
		let fraction = &fraction[..fraction
			.iter()
			.rposition(|&byte| byte != b'0')
			.map_or(0, |last| last + 1)];

		// No unit has more than 13 factors of 2 or of 5, so more than 13 significant fraction digits is never a whole number of nanoseconds.
		if fraction.len() > 13 {
			return fail("it is not a whole number of nanoseconds");
		}

		if !fraction.is_empty() {
			let scale = 10u128.pow(fraction.len() as u32);
			let fraction_nanoseconds = integer_digits(fraction) * u128::from(unit_length);

			if !fraction_nanoseconds.is_multiple_of(scale) {
				return fail("it is not a whole number of nanoseconds");
			}

			total = total.saturating_add(fraction_nanoseconds / scale);
		}

		if is_negative && total == 0 {
			return fail("“-” is not allowed before zero, because zero has one spelling: 0s");
		}

		let limit = if is_negative {
			i64::MIN.unsigned_abs()
		} else {
			i64::MAX.unsigned_abs()
		};

		if total > u128::from(limit) {
			return fail(
				"it is outside the 64-bit range of nanoseconds, about 292 years either way",
			);
		}

		let magnitude = total as i128;
		Ok(Self::from_nanoseconds(
			(if is_negative { -magnitude } else { magnitude }) as i64,
		))
	}
}

/**
The value of decimal digits with optional underscores, which saturates past the `u128` range. Every caller treats a value that large as out of range.
*/
fn integer_digits(digits: &[u8]) -> u128 {
	digits
		.iter()
		.filter(|&&byte| byte != b'_')
		.fold(0u128, |value, &byte| {
			value
				.saturating_mul(10)
				.saturating_add(u128::from(byte - b'0'))
		})
}

fn describe_bad_unit(unit: &str, text: &str) -> String {
	match unit {
		"" => "every number needs a unit: h, m, s, ms, us, or ns".to_owned(),
		"d" | "day" | "days" => "there is no day unit, because a day is not a fixed length. Write 24h for a fixed 24 hours".to_owned(),
		"w" | "week" | "weeks" => "there is no week unit, because a day is not a fixed length. Write 168h for a fixed 168 hours".to_owned(),
		"M" => {
			// A number with `M` alone, as in `memory: 512M`, is more often a size than a duration, and `512m` would read as minutes.
			let is_size = text.len() <= crate::scalar::MAX_DIAGNOSED_LENGTH && text.ends_with('M') && text[..text.len() - 1].bytes().all(|byte| byte.is_ascii_digit() || byte == b'_');
			let size_hint = if is_size {
				format!(". A size, such as 512M, is a string: '{}'", crate::abbreviate(text, 40))
			} else {
				String::new()
			};
			format!("there is no month unit, because a month is not a fixed length{size_hint}")
		}
		_ if UNITS.iter().any(|(name, _)| name.eq_ignore_ascii_case(unit)) => format!("the units are lowercase: {}", unit.to_ascii_lowercase()),
		_ => format!("“{}” is not a unit. The units are h, m, s, ms, us, and ns, and a string must be quoted", crate::abbreviate(unit, 10)),
	}
}

/**
Writes the canonical form: `-` when negative, then the hours, minutes, and seconds, each left out when zero, with the seconds' fraction written like an instant's. Zero is `0s`.
*/
impl Display for Duration {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		const SECOND: u64 = 1_000_000_000;
		let magnitude = self.nanoseconds.unsigned_abs();

		if magnitude == 0 {
			return formatter.write_str("0s");
		}

		if self.nanoseconds < 0 {
			formatter.write_str("-")?;
		}

		let hours = magnitude / (3600 * SECOND);
		let minutes = magnitude / (60 * SECOND) % 60;
		let seconds = magnitude / SECOND % 60;
		let nanoseconds = (magnitude % SECOND) as u32;

		if hours > 0 {
			write!(formatter, "{hours}h")?;
		}

		if minutes > 0 {
			write!(formatter, "{minutes}m")?;
		}

		if seconds > 0 || nanoseconds > 0 {
			write!(formatter, "{seconds}")?;
			crate::instant::write_fraction(formatter, nanoseconds)?;
			formatter.write_str("s")?;
		}

		Ok(())
	}
}

impl fmt::Debug for Duration {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "Duration({self})")
	}
}

/**
Reads a duration in SOML syntax.

```rust
let duration: soml::Duration = "1.5s".parse()?;
assert_eq!(duration.nanoseconds(), 1_500_000_000);

assert!("1d".parse::<soml::Duration>().is_err());
# Ok::<(), soml::Error>(())
```
*/
impl FromStr for Duration {
	type Err = Error;

	fn from_str(text: &str) -> Result<Self, Error> {
		Self::parse(text).map_err(Error::syntax)
	}
}

impl TryFrom<StdDuration> for Duration {
	type Error = Error;

	/**
	Fails when the duration is longer than `i64::MAX` nanoseconds, about 292 years.
	*/
	fn try_from(duration: StdDuration) -> Result<Self, Error> {
		i64::try_from(duration.as_nanos())
			.map(Self::from_nanoseconds)
			.map_err(|_| {
				Error::data(format!(
					"The duration {duration:?} is outside the SOML range of about 292 years"
				))
			})
	}
}

impl TryFrom<Duration> for StdDuration {
	type Error = Error;

	/**
	Fails when the duration is negative, because a `std::time::Duration` cannot be.
	*/
	fn try_from(duration: Duration) -> Result<Self, Error> {
		u64::try_from(duration.nanoseconds)
			.map(Self::from_nanos)
			.map_err(|_| {
				Error::data(format!(
					"The duration {duration} is negative, which a std::time::Duration cannot be"
				))
			})
	}
}

impl Serialize for Duration {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_newtype_struct(TOKEN, &crate::value::CanonicalText(self))
	}
}

impl<'de> Deserialize<'de> for Duration {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		struct DurationVisitor;

		impl<'de> Visitor<'de> for DurationVisitor {
			type Value = Duration;

			fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
				formatter.write_str("a duration")
			}

			fn visit_str<E: de::Error>(self, text: &str) -> Result<Duration, E> {
				Duration::parse(text).map_err(E::custom)
			}

			fn visit_newtype_struct<D: Deserializer<'de>>(
				self,
				deserializer: D,
			) -> Result<Duration, D::Error> {
				deserializer.deserialize_str(self)
			}
		}

		deserializer.deserialize_newtype_struct(TOKEN, DurationVisitor)
	}
}
