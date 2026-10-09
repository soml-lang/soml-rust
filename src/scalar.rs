/*!
Reading one token: numbers, instants, durations, and strings. The parser reads every value with these functions, and the syntax tree takes its values from the parser's tree, so the two cannot disagree about a value.
*/

use crate::parse::Kind;
use crate::tree::Radix;
use crate::{Duration, Instant, abbreviate};
use std::borrow::Cow;

/**
An error at a byte offset in the source.
*/
#[derive(Debug)]
pub(crate) struct ScalarError {
	pub message: String,
	pub offset: usize,
}

impl ScalarError {
	#[cold]
	pub(crate) fn new(message: impl Into<String>, offset: usize) -> Self {
		Self {
			message: message.into(),
			offset,
		}
	}
}

/**
Letters, digits, `_`, and `-`: the characters of a bare key.
*/
pub(crate) const fn is_bare_key_byte(byte: u8) -> bool {
	// A table, which is one lookup for the many bytes of keys and numbers, rather than several comparisons.
	const TABLE: [bool; 256] = {
		let mut table = [false; 256];
		let mut index = 0;

		while index < table.len() {
			let byte = index as u8;
			table[index] = byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-';
			index += 1;
		}

		table
	};

	TABLE[byte as usize]
}

/**
Whether a key can be written without quotes: one or more ASCII letters, digits, `_`, or `-`.
*/
pub(crate) fn is_bare_key(key: &str) -> bool {
	!key.is_empty() && key.bytes().all(is_bare_key_byte)
}

/**
The index after the letters, digits, `_`, and `-` from `start`, which is the end of a bare key that starts there.
*/
pub(crate) fn bare_key_end(bytes: &[u8], start: usize) -> usize {
	start
		+ bytes[start..]
			.iter()
			.take_while(|&&byte| is_bare_key_byte(byte))
			.count()
}

/**
The index after the spaces and tabs from `index`.
*/
pub(crate) fn skip_spaces(bytes: &[u8], index: usize) -> usize {
	index
		+ bytes
			.get(index..)
			.unwrap_or_default()
			.iter()
			.take_while(|byte| matches!(byte, b' ' | b'\t'))
			.count()
}

/**
The index where the line that `index` is on starts.
*/
pub(crate) fn line_start(bytes: &[u8], index: usize) -> usize {
	bytes[..index]
		.iter()
		.rposition(|&byte| byte == b'\n')
		.map_or(0, |position| position + 1)
}

/**
The index of the line feed that ends the line `from` is on, or the length of `bytes` on the last line.
*/
pub(crate) fn line_end(bytes: &[u8], from: usize) -> usize {
	find_any(&bytes[from..], *b"\n").map_or(bytes.len(), |length| from + length)
}

/**
The end of the number, instant, or duration token that starts at `start`. Such a token is lexed as one maximal run of letters, digits, `_`, `.`, `:`, `+`, and `-`, and then checked as a whole. A keyword, such as `true`, is one such run too.
*/
pub(crate) fn number_end(bytes: &[u8], start: usize) -> usize {
	start
		+ 1 + bytes[start + 1..]
		.iter()
		.take_while(|&&byte| {
			byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'+' | b'-')
		})
		.count()
}

/**
Returns the index after a run of digits that may have single underscores between them, or `index` when there is no digit there.
*/
pub(crate) fn skip_digits(bytes: &[u8], mut index: usize, is_digit: fn(&u8) -> bool) -> usize {
	if !bytes.get(index).is_some_and(is_digit) {
		return index;
	}

	index += 1;

	loop {
		match bytes.get(index) {
			Some(byte) if is_digit(byte) => index += 1,
			Some(b'_') if bytes.get(index + 1).is_some_and(is_digit) => index += 2,
			_ => return index,
		}
	}
}

/**
Returns the index after an integer part, which is `0` or digits without a leading zero, or `index` when there is none. Anything after a leading `0`, such as the `5` in `05`, is left for the caller to reject.
*/
pub(crate) fn skip_integer_part(bytes: &[u8], index: usize) -> usize {
	if bytes.get(index) == Some(&b'0') {
		index + 1
	} else {
		skip_digits(bytes, index, u8::is_ascii_digit)
	}
}

const fn is_octal_digit(byte: &u8) -> bool {
	matches!(byte, b'0'..=b'7')
}

const fn is_binary_digit(byte: &u8) -> bool {
	matches!(byte, b'0' | b'1')
}

const fn is_hexadecimal_digit(byte: &u8) -> bool {
	matches!(byte, b'0'..=b'9' | b'A'..=b'F')
}

#[derive(Clone, Copy, PartialEq)]
enum NumberKind {
	Int(Radix),
	Float,
}

/**
Whether `text` is a decimal int, a radix int, or a float, following the grammar.
*/
fn classify_number(text: &[u8]) -> Option<NumberKind> {
	// A radix prefix is only read at the start, so a signed radix int, such as `-0x1`, is not a number here, and `describe_bad_number` reports the sign.
	if text.first() == Some(&b'0') {
		let radix = match text.get(1) {
			Some(b'x') => Some((Radix::Hexadecimal, is_hexadecimal_digit as fn(&u8) -> bool)),
			Some(b'o') => Some((Radix::Octal, is_octal_digit as fn(&u8) -> bool)),
			Some(b'b') => Some((Radix::Binary, is_binary_digit as fn(&u8) -> bool)),
			_ => None,
		};

		if let Some((radix, is_digit)) = radix {
			let end = skip_digits(text, 2, is_digit);
			return (end > 2 && end == text.len()).then_some(NumberKind::Int(radix));
		}
	}

	let mut index = usize::from(text.first() == Some(&b'-'));
	let integer_end = skip_integer_part(text, index);

	if integer_end == index {
		return None;
	}

	index = integer_end;
	let mut kind = NumberKind::Int(Radix::Decimal);

	if text.get(index) == Some(&b'.') {
		let end = skip_digits(text, index + 1, u8::is_ascii_digit);

		if end == index + 1 {
			return None;
		}

		index = end;
		kind = NumberKind::Float;
	}

	if text.get(index) == Some(&b'e') {
		let is_negative = text.get(index + 1) == Some(&b'-');
		let exponent_start = index + 1 + usize::from(is_negative);

		// The exponent follows the leading-zero rule of the integer part, and its zero has one spelling, `e0`, so `e-0` is not an exponent.
		let end = if is_negative && text.get(exponent_start) == Some(&b'0') {
			exponent_start
		} else {
			skip_integer_part(text, exponent_start)
		};

		if end == exponent_start {
			return None;
		}

		index = end;
		kind = NumberKind::Float;
	}

	(index == text.len()).then_some(kind)
}

/**
Whether a token that is not a number or an instant was meant as a duration: its first letter could begin a unit, or a day or a week, or it is a year unit. A radix prefix, an exponent, and other letters, such as the `T` in `20260919T140000Z` or the `x` in `1.5x`, are left to the number errors.
*/
fn is_duration_like(text: &[u8]) -> bool {
	let start = usize::from(text.first() == Some(&b'-'));

	if !text.get(start).is_some_and(u8::is_ascii_digit) {
		return false;
	}

	let is_digit_or_underscore = |index: usize| {
		text.get(index)
			.is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
	};
	let mut index = start;

	while is_digit_or_underscore(index) {
		index += 1;
	}

	if text.get(index) == Some(&b'.') {
		index += 1;

		while is_digit_or_underscore(index) {
			index += 1;
		}
	}

	if text.get(index).is_some_and(|byte| {
		matches!(
			byte.to_ascii_lowercase(),
			b'd' | b'h' | b'm' | b'n' | b's' | b'u' | b'w'
		)
	}) {
		return true;
	}

	// A year unit, as in `1y` or `2years`, is only one when the token or another part follows it, so that a word such as `100yen` stays a string to quote.
	let unit_end = index
		+ text[index..]
			.iter()
			.take_while(|byte| byte.is_ascii_alphabetic())
			.count();

	crate::duration::is_year_unit(&text[index..unit_end])
		&& text.get(unit_end).is_none_or(u8::is_ascii_digit)
}

/**
Digits without their leading zeros, with zero itself as `0`.
*/
fn without_leading_zeros(digits: &str) -> &str {
	match digits.trim_start_matches('0') {
		"" => "0",
		trimmed => trimmed,
	}
}

/**
Whether a token begins like a date, `YYYY-MM-DD`, so it is read as an instant.
*/
pub(crate) fn has_date_prefix(text: &[u8]) -> bool {
	text.len() >= 10
		&& text[..4].iter().all(u8::is_ascii_digit)
		&& text[4] == b'-'
		&& text[5..7].iter().all(u8::is_ascii_digit)
		&& text[7] == b'-'
		&& text[8..10].iter().all(u8::is_ascii_digit)
}

/**
Reads a number, an instant, or a duration from a whole token. The error is the reason the token is invalid, which the caller reports at the token's start. `None` when the token is no number, instant, or duration at all, which the caller describes with `describe_bad_number`, because the description depends on the text after the token.
*/
pub(crate) fn token_value(text: &str) -> Option<Result<Kind<'static>, String>> {
	if let Some(kind) = plain_number(text) {
		return Some(Ok(kind));
	}

	let bytes = text.as_bytes();

	if has_date_prefix(bytes) {
		return Some(Instant::parse(text).map(Kind::Instant));
	}

	let kind = classify_number(bytes);

	match kind {
		None if is_duration_like(bytes) => Some(Duration::parse(text).map(Kind::Duration)),
		Some(NumberKind::Int(radix)) => {
			if text == "-0" {
				return Some(Err(
					"“-0” is not allowed, because zero has one spelling: 0".to_owned()
				));
			}

			Some(integer(text, radix).map(Kind::Int))
		}
		Some(NumberKind::Float) => Some(float(text).map(Kind::Float)),
		None => None,
	}
}

/**
The common case of a short decimal int or float, such as `8080`, `-3`, or `30.5`, without the general path's checks. Anything else, including every error, returns `None` and is left to the general path.
*/
fn plain_number(text: &str) -> Option<Kind<'static>> {
	let bytes = text.as_bytes();
	let integer = bytes.strip_prefix(b"-").unwrap_or(bytes);
	let integer_length = integer
		.iter()
		.take_while(|byte| byte.is_ascii_digit())
		.count();

	// No digits, or a leading zero, which is either `0` alone or an error.
	if integer_length == 0 || (integer_length > 1 && integer[0] == b'0') {
		return None;
	}

	// At most 18 digits, so an int cannot overflow.
	if integer.len() > 18 {
		return None;
	}

	match &integer[integer_length..] {
		[] => {
			// `-0` is an error, which the general path reports.
			if text == "-0" {
				return None;
			}

			Some(Kind::Int(text.parse().ok()?))
		}
		[b'.', fraction @ ..]
			if !fraction.is_empty() && fraction.iter().all(u8::is_ascii_digit) =>
		{
			let value: f64 = text.parse().ok()?;

			// Negative zero is the same value as zero.
			Some(Kind::Float(if value == 0.0 { 0.0 } else { value }))
		}
		// A character that may continue a number, such as `e`, `_`, or `:`, needs the general path.
		_ => None,
	}
}

fn integer(text: &str, radix: Radix) -> Result<i64, String> {
	let out_of_range = || {
		format!(
			"The integer {} is outside the 64-bit range (-9223372036854775808 to 9223372036854775807)",
			abbreviate(text, 40)
		)
	};

	let (digits, is_negative) = match (radix, text.strip_prefix('-')) {
		(Radix::Decimal, Some(digits)) => (digits, true),
		(Radix::Decimal, None) => (text, false),
		_ => (&text[2..], false),
	};
	// The discriminant of each `Radix` is its base.
	let radix = radix as u32;

	// Accumulated as the magnitude, so that the most negative value, whose magnitude is one more than the largest positive value, is read too.
	let mut magnitude: u64 = 0;

	for byte in digits.bytes().filter(|&byte| byte != b'_') {
		let digit = char::from(byte)
			.to_digit(radix)
			.expect("the token was checked against the grammar");
		magnitude = magnitude
			.checked_mul(u64::from(radix))
			.and_then(|value| value.checked_add(u64::from(digit)))
			.ok_or_else(out_of_range)?;
	}

	if is_negative {
		0i64.checked_sub_unsigned(magnitude)
			.ok_or_else(out_of_range)
	} else {
		i64::try_from(magnitude).map_err(|_| out_of_range())
	}
}

fn float(text: &str) -> Result<f64, String> {
	let digits: Cow<'_, str> = if text.contains('_') {
		Cow::Owned(text.replace('_', ""))
	} else {
		Cow::Borrowed(text)
	};
	let value: f64 = digits
		.parse()
		.expect("the token was checked against the grammar");

	if !value.is_finite() {
		let infinity = if value < 0.0 { "-infinity" } else { "infinity" };
		return Err(format!(
			"{} is too large to be a finite float. Use {infinity} if you mean it",
			abbreviate(text, 40)
		));
	}

	if value == 0.0 {
		// A nonzero digit before the exponent means the literal is not zero, so it underflowed.
		let significand = text.split('e').next().unwrap_or_default();

		if significand.bytes().any(|byte| matches!(byte, b'1'..=b'9')) {
			return Err(format!(
				"{} is too small to be told apart from zero. Write 0.0 if you mean zero",
				abbreviate(text, 40)
			));
		}

		// Negative zero is the same value as zero.
		return Ok(0.0);
	}

	Ok(value)
}

/**
The longest part of an invalid token that the diagnostics look at, so a huge token does not cost a huge scan.
*/
pub(crate) const MAX_DIAGNOSED_LENGTH: usize = 1000;

/**
The example in a message that a string must be quoted, which quotes `text` as a `'...'` string. That cannot hold a `'`, so then there is no simple example.
*/
pub(crate) fn quoting_example(text: &str) -> String {
	if text.contains('\'') {
		String::new()
	} else {
		format!(", as in '{}'", abbreviate(text, 40))
	}
}

/**
The error for a token that is no number, instant, or duration. `unquoted_text` is the unquoted string that the token begins, for the suggestion to quote it.
*/
pub(crate) fn describe_bad_number(full_text: &str, unquoted_text: &str) -> String {
	let text = &full_text.as_bytes()[..full_text.len().min(MAX_DIAGNOSED_LENGTH)];
	let digit_run_end = |index: usize| {
		index
			+ text[index..]
				.iter()
				.take_while(|byte| byte.is_ascii_digit() || **byte == b'_')
				.count()
	};

	// The index after `-?\d[\d_]*`, when the text starts with it.
	let sign_length = usize::from(text.first() == Some(&b'-'));
	let leading_digit_run = text
		.get(sign_length)
		.is_some_and(u8::is_ascii_digit)
		.then(|| digit_run_end(sign_length + 1));

	if text.contains(&b'+') {
		return "A “+” sign is not allowed in a number, including in an exponent".to_owned();
	}

	let unsigned = text.strip_prefix(b"-").unwrap_or(text);

	if let [b'0', b'B' | b'O' | b'X', ..] = unsigned {
		return "A number prefix is lowercase: 0x, 0o, or 0b".to_owned();
	}

	// The digits after a radix prefix are checked in one pass each, so the whole token is diagnosed, however long.
	let full_bytes = full_text.as_bytes();

	if let [b'0', radix @ (b'b' | b'o' | b'x'), digits @ ..] =
		full_bytes.strip_prefix(b"-").unwrap_or(full_bytes)
	{
		let (name, article, is_digit): (&str, &str, fn(&u8) -> bool) = match radix {
			b'x' => ("hexadecimal", "A", is_hexadecimal_digit),
			b'o' => ("octal", "An", is_octal_digit),
			_ => ("binary", "A", is_binary_digit),
		};

		if text.first() == Some(&b'-') {
			return format!(
				"{article} {name} integer cannot have a sign, because it states a bit pattern rather than a quantity"
			);
		}

		if digits.is_empty() {
			return format!("Expected {name} digits after “0{}”", char::from(*radix));
		}

		if *radix == b'x'
			&& digits.iter().any(|byte| matches!(byte, b'a'..=b'f'))
			&& digits
				.iter()
				.all(|byte| byte.is_ascii_hexdigit() || *byte == b'_')
		{
			// The uppercase spelling is only suggested when it is valid, so a misplaced underscore is reported first, and a value outside the 64-bit range gets no example.
			if skip_digits(digits, 0, u8::is_ascii_hexdigit) != digits.len() {
				return "An underscore in a number must be between two digits".to_owned();
			}

			let uppercase = format!("0x{}", String::from_utf8_lossy(digits).to_ascii_uppercase());

			return if crate::parse::is_valid_value(&uppercase) {
				format!(
					"Hexadecimal digits are uppercase: {}",
					abbreviate(&uppercase, 40)
				)
			} else {
				"Hexadecimal digits are uppercase".to_owned()
			};
		}

		// A lowercase hexadecimal digit is a digit in the wrong case, so the one named is a character that is no digit in either case.
		let is_digit_in_any_case =
			|byte: u8| is_digit(&byte) || (*radix == b'x' && matches!(byte, b'a'..=b'f'));

		return match std::str::from_utf8(digits)
			.unwrap_or_default()
			.chars()
			.find(|character| {
				!(*character == '_' || u8::try_from(*character).is_ok_and(is_digit_in_any_case))
			}) {
			Some(character) => format!("Invalid {name} digit “{character}”"),
			None => "An underscore in a number must be between two digits".to_owned(),
		};
	}

	// `-?\d[\d_]*(\.[\d_]+)?E-?[\d_]*`, the whole text. Only when the “E” is where an exponent marker goes, with digits, a `-`, or nothing after it. In `5EUR` or `10EB`, it is part of a word, so lowercasing it would not help.
	if let Some(end) = leading_digit_run {
		let fraction_end = if text.get(end) == Some(&b'.') {
			digit_run_end(end + 1)
		} else {
			end
		};
		let marker = if fraction_end > end + 1 {
			fraction_end
		} else {
			end
		};

		if let [b'E', exponent @ ..] = &text[marker..]
			&& exponent
				.strip_prefix(b"-")
				.unwrap_or(exponent)
				.iter()
				.all(|byte| byte.is_ascii_digit() || *byte == b'_')
		{
			return "An exponent marker is a lowercase “e”".to_owned();
		}
	}

	// `-?[\d_]+\.` followed by the end or by something other than a digit or `_`.
	let integer_end = digit_run_end(sign_length);

	if integer_end > sign_length
		&& text.get(integer_end) == Some(&b'.')
		&& !text
			.get(integer_end + 1)
			.is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
	{
		return "A decimal point must be followed by a digit".to_owned();
	}

	if text.starts_with(b"-.") {
		return "A number cannot begin with “.”; write a digit before it, as in -0.5".to_owned();
	}

	// Digits with a `:` between them, such as a time of day.
	let parts: Vec<&[u8]> = text.split(|&byte| byte == b':').collect();

	if parts.len() > 1
		&& parts
			.iter()
			.all(|part| !part.is_empty() && part.iter().all(u8::is_ascii_digit))
	{
		let is_time_of_day = matches!(parts.len(), 2 | 3)
			&& matches!(parts[0].len(), 1 | 2)
			&& parts[1..].iter().all(|part| part.len() == 2);

		if is_time_of_day {
			return "A time of day is a string, so it must be quoted".to_owned();
		}

		let text = abbreviate(full_text, 40);
		return format!(
			"Invalid number “{text}”. A value that contains “:” must be quoted, as in '{text}'"
		);
	}

	// A value with a `:` that starts with a zero, such as the MAC address `00:1A:2B`, is not a number with a leading zero, because removing the zero would not make it valid.
	if let [b'0', b'0'..=b'9' | b'_', ..] = unsigned
		&& !text.contains(&b':')
	{
		// Removing the zero gives a valid number with another meaning, so the message says what the zero usually meant.
		let message = "Leading zeros are not allowed in a decimal number";

		if !text.iter().all(u8::is_ascii_digit) {
			return message.to_owned();
		}

		// A `'...'` string cannot hold a `'`, so then there is no example.
		let identifier = if unquoted_text.contains('\'') {
			"an identifier, such as a ZIP code, as a string".to_owned()
		} else {
			format!(
				"an identifier, such as a ZIP code, as a string: '{}'",
				abbreviate(unquoted_text, 40)
			)
		};

		// The octal suggestion is only for the number on its own, not for one that more text follows, as in `0412 345 678`, and only when it is in range. It is decided on the whole number, because a cut can end before a digit that is not octal or that changes the value.
		let octal = format!("0o{}", without_leading_zeros(full_text));

		if unquoted_text == full_text
			&& full_text.bytes().all(|byte| is_octal_digit(&byte))
			&& crate::parse::is_valid_value(&octal)
		{
			return format!(
				"{message}. Write an octal number, such as a file mode, as {}, and {identifier}",
				abbreviate(&octal, 40)
			);
		}

		return format!("{message}. Write {identifier}");
	}

	// An underscore at either end, or next to something other than a digit. Only in a number: an underscore next to a letter other than the exponent marker is part of a word, such as `4k_video`.
	let has_bad_underscore = text.iter().enumerate().any(|(index, byte)| {
		*byte == b'_'
			&& (index == 0
				|| index + 1 == text.len()
				|| !text[index - 1].is_ascii_digit()
				|| !text[index + 1].is_ascii_digit())
	});

	if has_bad_underscore
		&& text
			.iter()
			.all(|byte| byte.is_ascii_digit() || matches!(byte, b'-' | b'.' | b'_' | b'e'))
	{
		return "An underscore in a number must be between two digits".to_owned();
	}

	// `-?\d[\d_]*(\.[\d_]+)?e`, then the exponent.
	if let Some(end) = leading_digit_run {
		let mantissa_end = if text.get(end) == Some(&b'.') && digit_run_end(end + 1) > end + 1 {
			digit_run_end(end + 1)
		} else {
			end
		};

		if text.get(mantissa_end) == Some(&b'e') {
			let exponent = &text[mantissa_end + 1..];
			let exponent_digits = exponent.strip_prefix(b"-").unwrap_or(exponent);

			if let [b'0', b'_', b'0'..=b'9', ..] | [b'0', b'0'..=b'9', ..] = exponent_digits {
				return "Leading zeros are not allowed in an exponent".to_owned();
			}

			if exponent == b"-0" {
				return "“e-0” is not allowed, because an exponent of zero has one spelling: e0"
					.to_owned();
			}

			// Only a number ends with its exponent marker. A word that ends with an “e”, such as `1byte` or `-verbose`, has no exponent.
			if matches!(exponent, b"" | b"-") {
				return "Expected digits after the exponent marker “e”".to_owned();
			}
		}
	}

	if text.iter().filter(|&&byte| byte == b'.').count() > 1 {
		return format!(
			"Invalid number “{}”. A value with several dots, such as a version number, must be quoted",
			abbreviate(full_text, 40)
		);
	}

	if text.first() == Some(&b'-') && !text.get(1).is_some_and(u8::is_ascii_digit) {
		let rest = text[1..].to_ascii_lowercase();

		// The whole word, as for one without the “-”, so that a word such as `-nano` or `-info` is not taken for NaN or infinity.
		if rest == b"nan" {
			return "NaN is not representable. Use null for a missing value".to_owned();
		}

		if rest == b"inf" || rest == b"infinity" {
			return format!(
				"“{}” is not a value. Negative infinity is written -infinity",
				abbreviate(full_text, 40)
			);
		}

		return "Expected a digit or “infinity” after “-”".to_owned();
	}

	if text.iter().any(u8::is_ascii_alphabetic) {
		return format!(
			"Invalid number “{}”. A string value must be quoted{}",
			abbreviate(full_text, 40),
			quoting_example(unquoted_text)
		);
	}

	format!("Invalid number “{}”", abbreviate(full_text, 40))
}

/**
The error for a word where a value should be, such as `yes`, `NULL`, or an unquoted string. `text` is the unquoted string that `word` begins, which the suggestion quotes.
*/
pub(crate) fn describe_unknown_word(word: &str, text: &str) -> String {
	// A keyword hint is only for the word on its own. In `Yes please` or `Nan Goldin`, following it would leave text behind or change the value.
	let lowercase = if text == word {
		word.to_ascii_lowercase()
	} else {
		String::new()
	};

	match lowercase.as_str() {
		"true" | "false" | "yes" | "no" | "on" | "off" => {
			format!("“{word}” is not a value. Booleans are written true and false, in lowercase")
		}
		"null" | "nil" | "none" | "undefined" => {
			format!("“{word}” is not a value. Null is written null, in lowercase")
		}
		"nan" => "NaN is not representable. Use null for a missing value".to_owned(),
		"inf" | "infinity" => {
			format!("“{word}” is not a value. Infinity is written infinity, in lowercase")
		}
		_ => format!(
			"Unexpected “{}”. A string value must be quoted{}",
			abbreviate(word, 40),
			quoting_example(text)
		),
	}
}

/**
Describes a character for an error message, such as `“$”` or `U+00A0 (NO-BREAK SPACE; only space, tab, and line feed are whitespace)`.
*/
pub(crate) fn describe_character(character: char) -> String {
	let code = u32::from(character);

	if let Some(name) = invisible_character_name(character) {
		// U+200B and U+FEFF are not whitespace in Unicode, but they are zero-width spaces, so the note applies to them too.
		let note = if character.is_whitespace() || matches!(code, 0x200B | 0xFEFF) {
			"; only space, tab, and line feed are whitespace"
		} else {
			""
		};
		return format!("U+{code:04X} ({name}{note})");
	}

	if is_invisible(character) {
		return format!("U+{code:04X}");
	}

	format!("“{character}”")
}

/**
Whether a character is invisible or ambiguous on screen, so an error message shows it by code point: a control, format, private-use, unassigned, separator, or default-ignorable character other than the space. These are the characters of `[\p{Default_Ignorable_Code_Point}\p{Other}\p{Separator}]`, which the JS reference implementation uses.
*/
pub(crate) fn is_invisible(character: char) -> bool {
	// An odd count of boundaries at or below the character means that the last of them starts a range, so the character is in that range.
	INVISIBLE_RANGES.partition_point(|&boundary| boundary <= u32::from(character)) % 2 == 1
}

/*
The ranges of the characters that `is_invisible` matches, in Unicode 17.0, the version of Node.js 26: each range starts at a number at an even index and ends before the next number. The table includes the surrogates, which are not `char` values, so that fewer ranges are needed.
*/
const INVISIBLE_RANGES: &[u32] = &[
	0x0000, 0x0020, 0x007F, 0x00A1, 0x00AD, 0x00AE, 0x034F, 0x0350, 0x0378, 0x037A, 0x0380, 0x0384,
	0x038B, 0x038C, 0x038D, 0x038E, 0x03A2, 0x03A3, 0x0530, 0x0531, 0x0557, 0x0559, 0x058B, 0x058D,
	0x0590, 0x0591, 0x05C8, 0x05D0, 0x05EB, 0x05EF, 0x05F5, 0x0606, 0x061C, 0x061D, 0x06DD, 0x06DE,
	0x070E, 0x0710, 0x074B, 0x074D, 0x07B2, 0x07C0, 0x07FB, 0x07FD, 0x082E, 0x0830, 0x083F, 0x0840,
	0x085C, 0x085E, 0x085F, 0x0860, 0x086B, 0x0870, 0x0890, 0x0897, 0x08E2, 0x08E3, 0x0984, 0x0985,
	0x098D, 0x098F, 0x0991, 0x0993, 0x09A9, 0x09AA, 0x09B1, 0x09B2, 0x09B3, 0x09B6, 0x09BA, 0x09BC,
	0x09C5, 0x09C7, 0x09C9, 0x09CB, 0x09CF, 0x09D7, 0x09D8, 0x09DC, 0x09DE, 0x09DF, 0x09E4, 0x09E6,
	0x09FF, 0x0A01, 0x0A04, 0x0A05, 0x0A0B, 0x0A0F, 0x0A11, 0x0A13, 0x0A29, 0x0A2A, 0x0A31, 0x0A32,
	0x0A34, 0x0A35, 0x0A37, 0x0A38, 0x0A3A, 0x0A3C, 0x0A3D, 0x0A3E, 0x0A43, 0x0A47, 0x0A49, 0x0A4B,
	0x0A4E, 0x0A51, 0x0A52, 0x0A59, 0x0A5D, 0x0A5E, 0x0A5F, 0x0A66, 0x0A77, 0x0A81, 0x0A84, 0x0A85,
	0x0A8E, 0x0A8F, 0x0A92, 0x0A93, 0x0AA9, 0x0AAA, 0x0AB1, 0x0AB2, 0x0AB4, 0x0AB5, 0x0ABA, 0x0ABC,
	0x0AC6, 0x0AC7, 0x0ACA, 0x0ACB, 0x0ACE, 0x0AD0, 0x0AD1, 0x0AE0, 0x0AE4, 0x0AE6, 0x0AF2, 0x0AF9,
	0x0B00, 0x0B01, 0x0B04, 0x0B05, 0x0B0D, 0x0B0F, 0x0B11, 0x0B13, 0x0B29, 0x0B2A, 0x0B31, 0x0B32,
	0x0B34, 0x0B35, 0x0B3A, 0x0B3C, 0x0B45, 0x0B47, 0x0B49, 0x0B4B, 0x0B4E, 0x0B55, 0x0B58, 0x0B5C,
	0x0B5E, 0x0B5F, 0x0B64, 0x0B66, 0x0B78, 0x0B82, 0x0B84, 0x0B85, 0x0B8B, 0x0B8E, 0x0B91, 0x0B92,
	0x0B96, 0x0B99, 0x0B9B, 0x0B9C, 0x0B9D, 0x0B9E, 0x0BA0, 0x0BA3, 0x0BA5, 0x0BA8, 0x0BAB, 0x0BAE,
	0x0BBA, 0x0BBE, 0x0BC3, 0x0BC6, 0x0BC9, 0x0BCA, 0x0BCE, 0x0BD0, 0x0BD1, 0x0BD7, 0x0BD8, 0x0BE6,
	0x0BFB, 0x0C00, 0x0C0D, 0x0C0E, 0x0C11, 0x0C12, 0x0C29, 0x0C2A, 0x0C3A, 0x0C3C, 0x0C45, 0x0C46,
	0x0C49, 0x0C4A, 0x0C4E, 0x0C55, 0x0C57, 0x0C58, 0x0C5B, 0x0C5C, 0x0C5E, 0x0C60, 0x0C64, 0x0C66,
	0x0C70, 0x0C77, 0x0C8D, 0x0C8E, 0x0C91, 0x0C92, 0x0CA9, 0x0CAA, 0x0CB4, 0x0CB5, 0x0CBA, 0x0CBC,
	0x0CC5, 0x0CC6, 0x0CC9, 0x0CCA, 0x0CCE, 0x0CD5, 0x0CD7, 0x0CDC, 0x0CDF, 0x0CE0, 0x0CE4, 0x0CE6,
	0x0CF0, 0x0CF1, 0x0CF4, 0x0D00, 0x0D0D, 0x0D0E, 0x0D11, 0x0D12, 0x0D45, 0x0D46, 0x0D49, 0x0D4A,
	0x0D50, 0x0D54, 0x0D64, 0x0D66, 0x0D80, 0x0D81, 0x0D84, 0x0D85, 0x0D97, 0x0D9A, 0x0DB2, 0x0DB3,
	0x0DBC, 0x0DBD, 0x0DBE, 0x0DC0, 0x0DC7, 0x0DCA, 0x0DCB, 0x0DCF, 0x0DD5, 0x0DD6, 0x0DD7, 0x0DD8,
	0x0DE0, 0x0DE6, 0x0DF0, 0x0DF2, 0x0DF5, 0x0E01, 0x0E3B, 0x0E3F, 0x0E5C, 0x0E81, 0x0E83, 0x0E84,
	0x0E85, 0x0E86, 0x0E8B, 0x0E8C, 0x0EA4, 0x0EA5, 0x0EA6, 0x0EA7, 0x0EBE, 0x0EC0, 0x0EC5, 0x0EC6,
	0x0EC7, 0x0EC8, 0x0ECF, 0x0ED0, 0x0EDA, 0x0EDC, 0x0EE0, 0x0F00, 0x0F48, 0x0F49, 0x0F6D, 0x0F71,
	0x0F98, 0x0F99, 0x0FBD, 0x0FBE, 0x0FCD, 0x0FCE, 0x0FDB, 0x1000, 0x10C6, 0x10C7, 0x10C8, 0x10CD,
	0x10CE, 0x10D0, 0x115F, 0x1161, 0x1249, 0x124A, 0x124E, 0x1250, 0x1257, 0x1258, 0x1259, 0x125A,
	0x125E, 0x1260, 0x1289, 0x128A, 0x128E, 0x1290, 0x12B1, 0x12B2, 0x12B6, 0x12B8, 0x12BF, 0x12C0,
	0x12C1, 0x12C2, 0x12C6, 0x12C8, 0x12D7, 0x12D8, 0x1311, 0x1312, 0x1316, 0x1318, 0x135B, 0x135D,
	0x137D, 0x1380, 0x139A, 0x13A0, 0x13F6, 0x13F8, 0x13FE, 0x1400, 0x1680, 0x1681, 0x169D, 0x16A0,
	0x16F9, 0x1700, 0x1716, 0x171F, 0x1737, 0x1740, 0x1754, 0x1760, 0x176D, 0x176E, 0x1771, 0x1772,
	0x1774, 0x1780, 0x17B4, 0x17B6, 0x17DE, 0x17E0, 0x17EA, 0x17F0, 0x17FA, 0x1800, 0x180B, 0x1810,
	0x181A, 0x1820, 0x1879, 0x1880, 0x18AB, 0x18B0, 0x18F6, 0x1900, 0x191F, 0x1920, 0x192C, 0x1930,
	0x193C, 0x1940, 0x1941, 0x1944, 0x196E, 0x1970, 0x1975, 0x1980, 0x19AC, 0x19B0, 0x19CA, 0x19D0,
	0x19DB, 0x19DE, 0x1A1C, 0x1A1E, 0x1A5F, 0x1A60, 0x1A7D, 0x1A7F, 0x1A8A, 0x1A90, 0x1A9A, 0x1AA0,
	0x1AAE, 0x1AB0, 0x1ADE, 0x1AE0, 0x1AEC, 0x1B00, 0x1B4D, 0x1B4E, 0x1BF4, 0x1BFC, 0x1C38, 0x1C3B,
	0x1C4A, 0x1C4D, 0x1C8B, 0x1C90, 0x1CBB, 0x1CBD, 0x1CC8, 0x1CD0, 0x1CFB, 0x1D00, 0x1F16, 0x1F18,
	0x1F1E, 0x1F20, 0x1F46, 0x1F48, 0x1F4E, 0x1F50, 0x1F58, 0x1F59, 0x1F5A, 0x1F5B, 0x1F5C, 0x1F5D,
	0x1F5E, 0x1F5F, 0x1F7E, 0x1F80, 0x1FB5, 0x1FB6, 0x1FC5, 0x1FC6, 0x1FD4, 0x1FD6, 0x1FDC, 0x1FDD,
	0x1FF0, 0x1FF2, 0x1FF5, 0x1FF6, 0x1FFF, 0x2010, 0x2028, 0x2030, 0x205F, 0x2070, 0x2072, 0x2074,
	0x208F, 0x2090, 0x209D, 0x20A0, 0x20C2, 0x20D0, 0x20F1, 0x2100, 0x218C, 0x2190, 0x242A, 0x2440,
	0x244B, 0x2460, 0x2B74, 0x2B76, 0x2CF4, 0x2CF9, 0x2D26, 0x2D27, 0x2D28, 0x2D2D, 0x2D2E, 0x2D30,
	0x2D68, 0x2D6F, 0x2D71, 0x2D7F, 0x2D97, 0x2DA0, 0x2DA7, 0x2DA8, 0x2DAF, 0x2DB0, 0x2DB7, 0x2DB8,
	0x2DBF, 0x2DC0, 0x2DC7, 0x2DC8, 0x2DCF, 0x2DD0, 0x2DD7, 0x2DD8, 0x2DDF, 0x2DE0, 0x2E5E, 0x2E80,
	0x2E9A, 0x2E9B, 0x2EF4, 0x2F00, 0x2FD6, 0x2FF0, 0x3000, 0x3001, 0x3040, 0x3041, 0x3097, 0x3099,
	0x3100, 0x3105, 0x3130, 0x3131, 0x3164, 0x3165, 0x318F, 0x3190, 0x31E6, 0x31EF, 0x321F, 0x3220,
	0xA48D, 0xA490, 0xA4C7, 0xA4D0, 0xA62C, 0xA640, 0xA6F8, 0xA700, 0xA7DD, 0xA7F1, 0xA82D, 0xA830,
	0xA83A, 0xA840, 0xA878, 0xA880, 0xA8C6, 0xA8CE, 0xA8DA, 0xA8E0, 0xA954, 0xA95F, 0xA97D, 0xA980,
	0xA9CE, 0xA9CF, 0xA9DA, 0xA9DE, 0xA9FF, 0xAA00, 0xAA37, 0xAA40, 0xAA4E, 0xAA50, 0xAA5A, 0xAA5C,
	0xAAC3, 0xAADB, 0xAAF7, 0xAB01, 0xAB07, 0xAB09, 0xAB0F, 0xAB11, 0xAB17, 0xAB20, 0xAB27, 0xAB28,
	0xAB2F, 0xAB30, 0xAB6C, 0xAB70, 0xABEE, 0xABF0, 0xABFA, 0xAC00, 0xD7A4, 0xD7B0, 0xD7C7, 0xD7CB,
	0xD7FC, 0xF900, 0xFA6E, 0xFA70, 0xFADA, 0xFB00, 0xFB07, 0xFB13, 0xFB18, 0xFB1D, 0xFB37, 0xFB38,
	0xFB3D, 0xFB3E, 0xFB3F, 0xFB40, 0xFB42, 0xFB43, 0xFB45, 0xFB46, 0xFDD0, 0xFDF0, 0xFE00, 0xFE10,
	0xFE1A, 0xFE20, 0xFE53, 0xFE54, 0xFE67, 0xFE68, 0xFE6C, 0xFE70, 0xFE75, 0xFE76, 0xFEFD, 0xFF01,
	0xFFA0, 0xFFA1, 0xFFBF, 0xFFC2, 0xFFC8, 0xFFCA, 0xFFD0, 0xFFD2, 0xFFD8, 0xFFDA, 0xFFDD, 0xFFE0,
	0xFFE7, 0xFFE8, 0xFFEF, 0xFFFC, 0xFFFE, 0x10000, 0x1000C, 0x1000D, 0x10027, 0x10028, 0x1003B,
	0x1003C, 0x1003E, 0x1003F, 0x1004E, 0x10050, 0x1005E, 0x10080, 0x100FB, 0x10100, 0x10103,
	0x10107, 0x10134, 0x10137, 0x1018F, 0x10190, 0x1019D, 0x101A0, 0x101A1, 0x101D0, 0x101FE,
	0x10280, 0x1029D, 0x102A0, 0x102D1, 0x102E0, 0x102FC, 0x10300, 0x10324, 0x1032D, 0x1034B,
	0x10350, 0x1037B, 0x10380, 0x1039E, 0x1039F, 0x103C4, 0x103C8, 0x103D6, 0x10400, 0x1049E,
	0x104A0, 0x104AA, 0x104B0, 0x104D4, 0x104D8, 0x104FC, 0x10500, 0x10528, 0x10530, 0x10564,
	0x1056F, 0x1057B, 0x1057C, 0x1058B, 0x1058C, 0x10593, 0x10594, 0x10596, 0x10597, 0x105A2,
	0x105A3, 0x105B2, 0x105B3, 0x105BA, 0x105BB, 0x105BD, 0x105C0, 0x105F4, 0x10600, 0x10737,
	0x10740, 0x10756, 0x10760, 0x10768, 0x10780, 0x10786, 0x10787, 0x107B1, 0x107B2, 0x107BB,
	0x10800, 0x10806, 0x10808, 0x10809, 0x1080A, 0x10836, 0x10837, 0x10839, 0x1083C, 0x1083D,
	0x1083F, 0x10856, 0x10857, 0x1089F, 0x108A7, 0x108B0, 0x108E0, 0x108F3, 0x108F4, 0x108F6,
	0x108FB, 0x1091C, 0x1091F, 0x1093A, 0x1093F, 0x1095A, 0x10980, 0x109B8, 0x109BC, 0x109D0,
	0x109D2, 0x10A04, 0x10A05, 0x10A07, 0x10A0C, 0x10A14, 0x10A15, 0x10A18, 0x10A19, 0x10A36,
	0x10A38, 0x10A3B, 0x10A3F, 0x10A49, 0x10A50, 0x10A59, 0x10A60, 0x10AA0, 0x10AC0, 0x10AE7,
	0x10AEB, 0x10AF7, 0x10B00, 0x10B36, 0x10B39, 0x10B56, 0x10B58, 0x10B73, 0x10B78, 0x10B92,
	0x10B99, 0x10B9D, 0x10BA9, 0x10BB0, 0x10C00, 0x10C49, 0x10C80, 0x10CB3, 0x10CC0, 0x10CF3,
	0x10CFA, 0x10D28, 0x10D30, 0x10D3A, 0x10D40, 0x10D66, 0x10D69, 0x10D86, 0x10D8E, 0x10D90,
	0x10E60, 0x10E7F, 0x10E80, 0x10EAA, 0x10EAB, 0x10EAE, 0x10EB0, 0x10EB2, 0x10EC2, 0x10EC8,
	0x10ED0, 0x10ED9, 0x10EFA, 0x10F28, 0x10F30, 0x10F5A, 0x10F70, 0x10F8A, 0x10FB0, 0x10FCC,
	0x10FE0, 0x10FF7, 0x11000, 0x1104E, 0x11052, 0x11076, 0x1107F, 0x110BD, 0x110BE, 0x110C3,
	0x110D0, 0x110E9, 0x110F0, 0x110FA, 0x11100, 0x11135, 0x11136, 0x11148, 0x11150, 0x11177,
	0x11180, 0x111E0, 0x111E1, 0x111F5, 0x11200, 0x11212, 0x11213, 0x11242, 0x11280, 0x11287,
	0x11288, 0x11289, 0x1128A, 0x1128E, 0x1128F, 0x1129E, 0x1129F, 0x112AA, 0x112B0, 0x112EB,
	0x112F0, 0x112FA, 0x11300, 0x11304, 0x11305, 0x1130D, 0x1130F, 0x11311, 0x11313, 0x11329,
	0x1132A, 0x11331, 0x11332, 0x11334, 0x11335, 0x1133A, 0x1133B, 0x11345, 0x11347, 0x11349,
	0x1134B, 0x1134E, 0x11350, 0x11351, 0x11357, 0x11358, 0x1135D, 0x11364, 0x11366, 0x1136D,
	0x11370, 0x11375, 0x11380, 0x1138A, 0x1138B, 0x1138C, 0x1138E, 0x1138F, 0x11390, 0x113B6,
	0x113B7, 0x113C1, 0x113C2, 0x113C3, 0x113C5, 0x113C6, 0x113C7, 0x113CB, 0x113CC, 0x113D6,
	0x113D7, 0x113D9, 0x113E1, 0x113E3, 0x11400, 0x1145C, 0x1145D, 0x11462, 0x11480, 0x114C8,
	0x114D0, 0x114DA, 0x11580, 0x115B6, 0x115B8, 0x115DE, 0x11600, 0x11645, 0x11650, 0x1165A,
	0x11660, 0x1166D, 0x11680, 0x116BA, 0x116C0, 0x116CA, 0x116D0, 0x116E4, 0x11700, 0x1171B,
	0x1171D, 0x1172C, 0x11730, 0x11747, 0x11800, 0x1183C, 0x118A0, 0x118F3, 0x118FF, 0x11907,
	0x11909, 0x1190A, 0x1190C, 0x11914, 0x11915, 0x11917, 0x11918, 0x11936, 0x11937, 0x11939,
	0x1193B, 0x11947, 0x11950, 0x1195A, 0x119A0, 0x119A8, 0x119AA, 0x119D8, 0x119DA, 0x119E5,
	0x11A00, 0x11A48, 0x11A50, 0x11AA3, 0x11AB0, 0x11AF9, 0x11B00, 0x11B0A, 0x11B60, 0x11B68,
	0x11BC0, 0x11BE2, 0x11BF0, 0x11BFA, 0x11C00, 0x11C09, 0x11C0A, 0x11C37, 0x11C38, 0x11C46,
	0x11C50, 0x11C6D, 0x11C70, 0x11C90, 0x11C92, 0x11CA8, 0x11CA9, 0x11CB7, 0x11D00, 0x11D07,
	0x11D08, 0x11D0A, 0x11D0B, 0x11D37, 0x11D3A, 0x11D3B, 0x11D3C, 0x11D3E, 0x11D3F, 0x11D48,
	0x11D50, 0x11D5A, 0x11D60, 0x11D66, 0x11D67, 0x11D69, 0x11D6A, 0x11D8F, 0x11D90, 0x11D92,
	0x11D93, 0x11D99, 0x11DA0, 0x11DAA, 0x11DB0, 0x11DDC, 0x11DE0, 0x11DEA, 0x11EE0, 0x11EF9,
	0x11F00, 0x11F11, 0x11F12, 0x11F3B, 0x11F3E, 0x11F5B, 0x11FB0, 0x11FB1, 0x11FC0, 0x11FF2,
	0x11FFF, 0x1239A, 0x12400, 0x1246F, 0x12470, 0x12475, 0x12480, 0x12544, 0x12F90, 0x12FF3,
	0x13000, 0x13430, 0x13440, 0x13456, 0x13460, 0x143FB, 0x14400, 0x14647, 0x16100, 0x1613A,
	0x16800, 0x16A39, 0x16A40, 0x16A5F, 0x16A60, 0x16A6A, 0x16A6E, 0x16ABF, 0x16AC0, 0x16ACA,
	0x16AD0, 0x16AEE, 0x16AF0, 0x16AF6, 0x16B00, 0x16B46, 0x16B50, 0x16B5A, 0x16B5B, 0x16B62,
	0x16B63, 0x16B78, 0x16B7D, 0x16B90, 0x16D40, 0x16D7A, 0x16E40, 0x16E9B, 0x16EA0, 0x16EB9,
	0x16EBB, 0x16ED4, 0x16F00, 0x16F4B, 0x16F4F, 0x16F88, 0x16F8F, 0x16FA0, 0x16FE0, 0x16FE5,
	0x16FF0, 0x16FF7, 0x17000, 0x18CD6, 0x18CFF, 0x18D1F, 0x18D80, 0x18DF3, 0x1AFF0, 0x1AFF4,
	0x1AFF5, 0x1AFFC, 0x1AFFD, 0x1AFFF, 0x1B000, 0x1B123, 0x1B132, 0x1B133, 0x1B150, 0x1B153,
	0x1B155, 0x1B156, 0x1B164, 0x1B168, 0x1B170, 0x1B2FC, 0x1BC00, 0x1BC6B, 0x1BC70, 0x1BC7D,
	0x1BC80, 0x1BC89, 0x1BC90, 0x1BC9A, 0x1BC9C, 0x1BCA0, 0x1CC00, 0x1CCFD, 0x1CD00, 0x1CEB4,
	0x1CEBA, 0x1CED1, 0x1CEE0, 0x1CEF1, 0x1CF00, 0x1CF2E, 0x1CF30, 0x1CF47, 0x1CF50, 0x1CFC4,
	0x1D000, 0x1D0F6, 0x1D100, 0x1D127, 0x1D129, 0x1D173, 0x1D17B, 0x1D1EB, 0x1D200, 0x1D246,
	0x1D2C0, 0x1D2D4, 0x1D2E0, 0x1D2F4, 0x1D300, 0x1D357, 0x1D360, 0x1D379, 0x1D400, 0x1D455,
	0x1D456, 0x1D49D, 0x1D49E, 0x1D4A0, 0x1D4A2, 0x1D4A3, 0x1D4A5, 0x1D4A7, 0x1D4A9, 0x1D4AD,
	0x1D4AE, 0x1D4BA, 0x1D4BB, 0x1D4BC, 0x1D4BD, 0x1D4C4, 0x1D4C5, 0x1D506, 0x1D507, 0x1D50B,
	0x1D50D, 0x1D515, 0x1D516, 0x1D51D, 0x1D51E, 0x1D53A, 0x1D53B, 0x1D53F, 0x1D540, 0x1D545,
	0x1D546, 0x1D547, 0x1D54A, 0x1D551, 0x1D552, 0x1D6A6, 0x1D6A8, 0x1D7CC, 0x1D7CE, 0x1DA8C,
	0x1DA9B, 0x1DAA0, 0x1DAA1, 0x1DAB0, 0x1DF00, 0x1DF1F, 0x1DF25, 0x1DF2B, 0x1E000, 0x1E007,
	0x1E008, 0x1E019, 0x1E01B, 0x1E022, 0x1E023, 0x1E025, 0x1E026, 0x1E02B, 0x1E030, 0x1E06E,
	0x1E08F, 0x1E090, 0x1E100, 0x1E12D, 0x1E130, 0x1E13E, 0x1E140, 0x1E14A, 0x1E14E, 0x1E150,
	0x1E290, 0x1E2AF, 0x1E2C0, 0x1E2FA, 0x1E2FF, 0x1E300, 0x1E4D0, 0x1E4FA, 0x1E5D0, 0x1E5FB,
	0x1E5FF, 0x1E600, 0x1E6C0, 0x1E6DF, 0x1E6E0, 0x1E6F6, 0x1E6FE, 0x1E700, 0x1E7E0, 0x1E7E7,
	0x1E7E8, 0x1E7EC, 0x1E7ED, 0x1E7EF, 0x1E7F0, 0x1E7FF, 0x1E800, 0x1E8C5, 0x1E8C7, 0x1E8D7,
	0x1E900, 0x1E94C, 0x1E950, 0x1E95A, 0x1E95E, 0x1E960, 0x1EC71, 0x1ECB5, 0x1ED01, 0x1ED3E,
	0x1EE00, 0x1EE04, 0x1EE05, 0x1EE20, 0x1EE21, 0x1EE23, 0x1EE24, 0x1EE25, 0x1EE27, 0x1EE28,
	0x1EE29, 0x1EE33, 0x1EE34, 0x1EE38, 0x1EE39, 0x1EE3A, 0x1EE3B, 0x1EE3C, 0x1EE42, 0x1EE43,
	0x1EE47, 0x1EE48, 0x1EE49, 0x1EE4A, 0x1EE4B, 0x1EE4C, 0x1EE4D, 0x1EE50, 0x1EE51, 0x1EE53,
	0x1EE54, 0x1EE55, 0x1EE57, 0x1EE58, 0x1EE59, 0x1EE5A, 0x1EE5B, 0x1EE5C, 0x1EE5D, 0x1EE5E,
	0x1EE5F, 0x1EE60, 0x1EE61, 0x1EE63, 0x1EE64, 0x1EE65, 0x1EE67, 0x1EE6B, 0x1EE6C, 0x1EE73,
	0x1EE74, 0x1EE78, 0x1EE79, 0x1EE7D, 0x1EE7E, 0x1EE7F, 0x1EE80, 0x1EE8A, 0x1EE8B, 0x1EE9C,
	0x1EEA1, 0x1EEA4, 0x1EEA5, 0x1EEAA, 0x1EEAB, 0x1EEBC, 0x1EEF0, 0x1EEF2, 0x1F000, 0x1F02C,
	0x1F030, 0x1F094, 0x1F0A0, 0x1F0AF, 0x1F0B1, 0x1F0C0, 0x1F0C1, 0x1F0D0, 0x1F0D1, 0x1F0F6,
	0x1F100, 0x1F1AE, 0x1F1E6, 0x1F203, 0x1F210, 0x1F23C, 0x1F240, 0x1F249, 0x1F250, 0x1F252,
	0x1F260, 0x1F266, 0x1F300, 0x1F6D9, 0x1F6DC, 0x1F6ED, 0x1F6F0, 0x1F6FD, 0x1F700, 0x1F7DA,
	0x1F7E0, 0x1F7EC, 0x1F7F0, 0x1F7F1, 0x1F800, 0x1F80C, 0x1F810, 0x1F848, 0x1F850, 0x1F85A,
	0x1F860, 0x1F888, 0x1F890, 0x1F8AE, 0x1F8B0, 0x1F8BC, 0x1F8C0, 0x1F8C2, 0x1F8D0, 0x1F8D9,
	0x1F900, 0x1FA58, 0x1FA60, 0x1FA6E, 0x1FA70, 0x1FA7D, 0x1FA80, 0x1FA8B, 0x1FA8E, 0x1FAC7,
	0x1FAC8, 0x1FAC9, 0x1FACD, 0x1FADD, 0x1FADF, 0x1FAEB, 0x1FAEF, 0x1FAF9, 0x1FB00, 0x1FB93,
	0x1FB94, 0x1FBFB, 0x20000, 0x2A6E0, 0x2A700, 0x2B81E, 0x2B820, 0x2CEAE, 0x2CEB0, 0x2EBE1,
	0x2EBF0, 0x2EE5E, 0x2F800, 0x2FA1E, 0x30000, 0x3134B, 0x31350, 0x3347A, 0x110000,
];

/**
The names of the invisible and ambiguous characters a document most likely holds by mistake.
*/
fn invisible_character_name(character: char) -> Option<&'static str> {
	match u32::from(character) {
		0x061C => Some("ARABIC LETTER MARK"),
		0x85 => Some("NEXT LINE"),
		0xA0 => Some("NO-BREAK SPACE"),
		0xAD => Some("SOFT HYPHEN"),
		0x1680 => Some("OGHAM SPACE MARK"),
		0x2000 => Some("EN QUAD"),
		0x2001 => Some("EM QUAD"),
		0x2002 => Some("EN SPACE"),
		0x2003 => Some("EM SPACE"),
		0x2004 => Some("THREE-PER-EM SPACE"),
		0x2005 => Some("FOUR-PER-EM SPACE"),
		0x2006 => Some("SIX-PER-EM SPACE"),
		0x2007 => Some("FIGURE SPACE"),
		0x2008 => Some("PUNCTUATION SPACE"),
		0x2009 => Some("THIN SPACE"),
		0x200A => Some("HAIR SPACE"),
		0x200B => Some("ZERO WIDTH SPACE"),
		0x200C => Some("ZERO WIDTH NON-JOINER"),
		0x200D => Some("ZERO WIDTH JOINER"),
		0x200E => Some("LEFT-TO-RIGHT MARK"),
		0x200F => Some("RIGHT-TO-LEFT MARK"),
		0x2028 => Some("LINE SEPARATOR"),
		0x2029 => Some("PARAGRAPH SEPARATOR"),
		0x202A => Some("LEFT-TO-RIGHT EMBEDDING"),
		0x202B => Some("RIGHT-TO-LEFT EMBEDDING"),
		0x202C => Some("POP DIRECTIONAL FORMATTING"),
		0x202D => Some("LEFT-TO-RIGHT OVERRIDE"),
		0x202E => Some("RIGHT-TO-LEFT OVERRIDE"),
		0x202F => Some("NARROW NO-BREAK SPACE"),
		0x205F => Some("MEDIUM MATHEMATICAL SPACE"),
		0x2060 => Some("WORD JOINER"),
		0x2066 => Some("LEFT-TO-RIGHT ISOLATE"),
		0x2067 => Some("RIGHT-TO-LEFT ISOLATE"),
		0x2068 => Some("FIRST STRONG ISOLATE"),
		0x2069 => Some("POP DIRECTIONAL ISOLATE"),
		0x3000 => Some("IDEOGRAPHIC SPACE"),
		0xFEFF => Some("ZERO WIDTH NO-BREAK SPACE"),
		_ => None,
	}
}

/**
Whether a character may be meant as part of a key, although a bare key cannot hold it: a visible character with no meaning in the grammar, such as the `$` in `$schema`. A `=` is left out, because after a key, as in `name=foo`, it was most likely meant as the `:` of INI and TOML, so a key that holds a `=` gets no quoting hint.
*/
pub(crate) fn is_quotable_key_character(character: char) -> bool {
	character != ' ' && !is_invisible(character) && !"\"#'*,/:=[]{}".contains(character)
}

/**
Whether a block string starts at `start`: a run of three or more of the same quote.
*/
pub(crate) fn is_block_string(bytes: &[u8], start: usize) -> bool {
	bytes.get(start + 1) == Some(&bytes[start]) && bytes.get(start + 2) == Some(&bytes[start])
}

/**
Reads any string that starts at `start`, which is a quote: a block string, `'...'`, or `"..."`. Returns its value, borrowed from the source when it can be, and the index after it.
*/
pub(crate) fn string(source: &str, start: usize) -> Result<(Cow<'_, str>, usize), ScalarError> {
	if is_block_string(source.as_bytes(), start) {
		let (value, end) = block_string(source, start)?;
		return Ok((Cow::Owned(value), end));
	}

	if source.as_bytes()[start] == b'\'' {
		let (value, end) = literal_string(source, start)?;
		return Ok((Cow::Borrowed(value), end));
	}

	escaped_string(source, start)
}

/**
The index of the first byte that is one of `targets`. It checks 8 bytes at a time in one machine word, as `serde_json` and `memchr` do, and the lowest marked byte of a word is the first match.
*/
#[inline]
pub(crate) fn find_any<const N: usize>(bytes: &[u8], targets: [u8; N]) -> Option<usize> {
	// All bytes of these masks are the same, so the byte order does not matter. The word itself must be little-endian, so that its first byte is the lowest.
	const ONES: u64 = u64::from_ne_bytes([0x01; 8]);
	const HIGHS: u64 = u64::from_ne_bytes([0x80; 8]);

	let (words, remainder) = bytes.as_chunks::<8>();

	for (word_index, word) in words.iter().enumerate() {
		let word = u64::from_le_bytes(*word);
		let mut found = 0;

		// A byte of `difference` is zero where the word has the target, which sets the high bit of that byte. A borrow can mark a byte above a match too, but never one below it.
		for target in targets {
			let difference = word ^ (ONES * u64::from(target));
			found |= difference.wrapping_sub(ONES) & !difference & HIGHS;
		}

		if found != 0 {
			return Some(word_index * 8 + found.trailing_zeros() as usize / 8);
		}
	}

	let remainder_start = bytes.len() - remainder.len();

	remainder
		.iter()
		.position(|byte| targets.contains(byte))
		.map(|position| remainder_start + position)
}

/**
Reads a `'...'` string that starts at `start`. Returns its value, borrowed from the source, and the index after its closing quote.
*/
pub(crate) fn literal_string(source: &str, start: usize) -> Result<(&str, usize), ScalarError> {
	let bytes = source.as_bytes();

	// Stops at the first quote or line break, so that a one-line document with many strings stays linear.
	match find_any(&bytes[start + 1..], *b"'\n") {
		Some(length) if bytes[start + 1 + length] == b'\'' => {
			Ok((&source[start + 1..start + 1 + length], start + 2 + length))
		}
		_ => Err(ScalarError::new(
			"Unterminated string. A '...' string must end on the line it starts on; use a block string (''') for multiple lines",
			start,
		)),
	}
}

/**
Reads a `"..."` string that starts at `start`. Returns its value, borrowed from the source when it has no escapes, and the index after its closing quote.
*/
pub(crate) fn escaped_string(
	source: &str,
	start: usize,
) -> Result<(Cow<'_, str>, usize), ScalarError> {
	let bytes = source.as_bytes();
	let mut chunk_start = start + 1;
	let mut value: Option<String> = None;

	loop {
		let Some(length) = find_any(&bytes[chunk_start..], *b"\"\\\n") else {
			return Err(unterminated_escaped_string(start));
		};

		let index = chunk_start + length;
		let chunk = &source[chunk_start..index];

		match bytes[index] {
			b'"' => {
				let value = match value {
					Some(mut value) => {
						value.push_str(chunk);
						Cow::Owned(value)
					}
					None => Cow::Borrowed(chunk),
				};

				return Ok((value, index + 1));
			}
			b'\\' => {
				let value = value.get_or_insert_with(String::new);
				value.push_str(chunk);
				chunk_start = escape(source, index, value)?;
			}
			_ => return Err(unterminated_escaped_string(start)),
		}
	}
}

#[cold]
fn unterminated_escaped_string(start: usize) -> ScalarError {
	ScalarError::new(
		"Unterminated string. A \"...\" string must end on the line it starts on; use a block string (\"\"\") for multiple lines",
		start,
	)
}

/**
Decodes the escape at `offset`, which is a backslash, into `output`. Returns the index after it.
*/
fn escape(source: &str, offset: usize, output: &mut String) -> Result<usize, ScalarError> {
	let bytes = source.as_bytes();

	match bytes.get(offset + 1) {
		Some(b'\\') => output.push('\\'),
		Some(b'"') => output.push('"'),
		Some(b'n') => output.push('\n'),
		Some(b't') => output.push('\t'),
		Some(b'u') => return unicode_escape(source, offset, output),
		Some(b'r') => {
			return Err(ScalarError::new(
				"There is no \\r escape, because a carriage return cannot be represented",
				offset,
			));
		}
		Some(b'\'') => {
			return Err(ScalarError::new(
				"A ' needs no escape inside \"...\"",
				offset,
			));
		}
		None | Some(b'\n') => {
			return Err(ScalarError::new(
				"A backslash must be followed by an escape character. Use \\\\ for a literal backslash, or a '...' string",
				offset,
			));
		}
		Some(_) => {
			let character = source[offset + 1..].chars().next().unwrap_or_default();
			return Err(ScalarError::new(
				format!(
					"Unknown escape “\\{character}”. The escapes are \\\\, \\\", \\n, \\t, and \\u{{…}}; use a '...' string for literal backslashes"
				),
				offset,
			));
		}
	}

	Ok(offset + 2)
}

/**
Decodes a `\u{…}` escape at `offset`, which is the backslash. Returns the index after it.
*/
fn unicode_escape(source: &str, offset: usize, output: &mut String) -> Result<usize, ScalarError> {
	let bytes = source.as_bytes();
	let digits_start = offset + 3;
	let length = bytes
		.get(digits_start..)
		.unwrap_or_default()
		.iter()
		// Uppercase digits too, so that the value of such an escape is checked before its spelling.
		.take_while(|byte| byte.is_ascii_hexdigit())
		// One more than the six allowed, so that too many digits are found without a scan of all of them.
		.take(7)
		.count();

	if bytes.get(offset + 2) != Some(&b'{')
		|| length == 0
		|| length > 6
		|| bytes.get(digits_start + length) != Some(&b'}')
	{
		return Err(ScalarError::new(
			describe_bad_unicode_escape(source, offset + 1),
			offset,
		));
	}

	let hex = &source[digits_start..digits_start + length];
	let code = u32::from_str_radix(hex, 16).expect("the digits are hexadecimal");

	// The value is checked before the spelling, so that the uppercase and leading zeros errors never point to an escape that is not allowed either.
	if code == 0x0D {
		return Err(ScalarError::new(
			format!(
				"A carriage return (U+000D) cannot be represented, so \\u{{{hex}}} is not allowed"
			),
			offset,
		));
	}

	if (0xD800..=0xDFFF).contains(&code) {
		return Err(ScalarError::new(
			format!("\\u{{{hex}}} is a surrogate, which is not a Unicode scalar value"),
			offset,
		));
	}

	let Some(character) = char::from_u32(code) else {
		return Err(ScalarError::new(
			format!("\\u{{{hex}}} is above U+10FFFF, the largest Unicode scalar value"),
			offset,
		));
	};

	if hex.bytes().any(|byte| byte.is_ascii_uppercase()) {
		return Err(ScalarError::new(
			"A Unicode escape uses lowercase hexadecimal digits",
			offset,
		));
	}

	if length > 1 && hex.starts_with('0') {
		return Err(ScalarError::new(
			format!(
				"A Unicode escape may not have leading zeros; write \\u{{{}}}",
				without_leading_zeros(hex)
			),
			offset,
		));
	}

	output.push(character);
	Ok(digits_start + length + 1)
}

/**
Describes a malformed `\u` escape. `offset` is the `u`.
*/
fn describe_bad_unicode_escape(source: &str, offset: usize) -> String {
	// Long enough for the longest form that is read, a JSON surrogate pair such as `ud83d\ude00`. The slice is of bytes, so it may cut a character.
	let rest = &source.as_bytes()[offset..source.len().min(offset + 12)];

	if rest.len() >= 5 && rest[1..5].iter().all(u8::is_ascii_hexdigit) {
		return describe_four_digit_escape(rest);
	}

	let braced_digits = rest.strip_prefix(b"u{").map(|digits| {
		&digits[..digits
			.iter()
			.take_while(|byte| byte.is_ascii_hexdigit())
			.count()]
	});

	if let Some(digits) = braced_digits {
		if digits.iter().any(u8::is_ascii_uppercase) {
			return "A Unicode escape uses lowercase hexadecimal digits".to_owned();
		}

		if rest.starts_with(b"u{}") {
			return "A Unicode escape needs one to six hexadecimal digits".to_owned();
		}

		if digits.len() >= 7 {
			return "A Unicode escape has at most six hexadecimal digits".to_owned();
		}
	}

	"A Unicode escape is written \\u{…} with one to six lowercase hexadecimal digits".to_owned()
}

/**
The JSON form `\uXXXX`, from its `u`, with the escape to write instead. JSON writes a character above U+FFFF as two of them, a surrogate pair, which is one `\u{…}` escape here. Here, a lone surrogate and a carriage return have no escape.
*/
fn describe_four_digit_escape(rest: &[u8]) -> String {
	let four_digits = |digits: &[u8]| {
		std::str::from_utf8(digits)
			.ok()
			.and_then(|digits| u32::from_str_radix(digits, 16).ok())
	};
	let code = four_digits(&rest[1..5]).expect("the digits are hexadecimal");
	let form = String::from_utf8_lossy(&rest[..5]);
	let low = rest
		.get(5..11)
		.filter(|escape| {
			escape.starts_with(b"\\u") && escape[2..].iter().all(u8::is_ascii_hexdigit)
		})
		.map(|escape| &escape[2..]);

	if let Some(low) = low
		&& let Some(low_code) = four_digits(low)
		&& (0xD800..=0xDBFF).contains(&code)
		&& (0xDC00..=0xDFFF).contains(&low_code)
	{
		let code_point = 0x10000 + (code - 0xD800) * 0x400 + (low_code - 0xDC00);
		return format!(
			"The four-digit \\{form}\\u{} form is not an escape. Write \\u{{{code_point:x}}}",
			String::from_utf8_lossy(low)
		);
	}

	if (0xD800..=0xDFFF).contains(&code) {
		return format!(
			"The four-digit \\{form} form is not an escape, and a lone surrogate is not a Unicode scalar value. Write the character it is half of as one \\u{{…}} escape"
		);
	}

	if code == 0x0D {
		return format!(
			"The four-digit \\{form} form is not an escape, and a carriage return (U+000D) cannot be represented"
		);
	}

	format!("The four-digit \\{form} form is not an escape. Write \\u{{{code:x}}}")
}

/**
Reads a block string, whose opening run of quotes starts at `start`. Returns its value and the index after its closing delimiter.

The block closes at the first line whose first non-whitespace content is a run of exactly as many quotes as the opening delimiter. The closing line's indentation is removed from every content line, except a blank one, which holds only spaces and tabs and becomes an empty line. Blank lines directly after the opening delimiter and directly before the closing one are not content.
*/
pub(crate) fn block_string(source: &str, start: usize) -> Result<(String, usize), ScalarError> {
	let bytes = source.as_bytes();
	let quote = bytes[start];
	let delimiter_length = bytes[start..]
		.iter()
		.take_while(|&&byte| byte == quote)
		.count();
	let mut index = start + delimiter_length;

	if index >= bytes.len() {
		return Err(ScalarError::new("Unterminated block string", start));
	}

	// As in Swift, the opening delimiter is followed directly by a line break, not even by trailing whitespace.
	if bytes[index] != b'\n' {
		return Err(ScalarError::new(
			"A block string's opening delimiter must be followed directly by a line break, and its content starts on the next line",
			index,
		));
	}

	index += 1;
	let content_start = index;

	// Each content line, as a range of the source.
	let mut lines = Vec::new();

	let indentation = loop {
		if index >= bytes.len() {
			return Err(ScalarError::new(
				format!(
					"Unterminated block string{}",
					describe_inline_closing_delimiter(
						source,
						content_start,
						quote,
						delimiter_length
					)
				),
				start,
			));
		}

		let line_start = index;
		let line_end = line_end(bytes, line_start);
		let content_start = skip_spaces(bytes, line_start);
		let run_length = bytes[content_start..]
			.iter()
			.take_while(|&&byte| byte == quote)
			.count();

		if run_length == delimiter_length {
			index = content_start + delimiter_length;
			break &source[line_start..content_start];
		}

		lines.push(line_start..line_end);
		index = line_end + 1;
	};

	// Each line's content and its offset, with blank lines empty.
	let mut contents = Vec::with_capacity(lines.len());

	for range in lines {
		let line = &source[range.clone()];

		// A blank line, which is empty or holds only spaces and tabs, may leave out the indentation, and it becomes an empty line. Swift is stricter here: there only a completely empty line may leave it out.
		if line.bytes().all(|byte| byte == b' ' || byte == b'\t') {
			contents.push(("", range.start));
		} else if let Some(content) = line.strip_prefix(indentation) {
			contents.push((content, range.start + indentation.len()));
		} else {
			return Err(ScalarError::new(
				"This line does not start with the indentation of its block string's closing delimiter. Every line except a blank one must start with exactly the same spaces and tabs",
				range.start,
			));
		}
	}

	let first = contents
		.iter()
		.position(|(text, _)| !text.is_empty())
		.unwrap_or(contents.len());
	let last = contents
		.iter()
		.rposition(|(text, _)| !text.is_empty())
		.map_or(first, |last| last + 1);
	let mut value = String::new();

	for (position, (text, offset)) in contents[first..last].iter().enumerate() {
		if position > 0 {
			value.push('\n');
		}

		if quote == b'\'' {
			value.push_str(text);
		} else {
			unescape_line(source, text, *offset, &mut value)?;
		}
	}

	Ok((value, index))
}

/**
The hint for a content line of an unterminated block string that ends with the delimiter, as TOML allows. Adding a closing line after it would keep the delimiter as content.
*/
fn describe_inline_closing_delimiter(
	source: &str,
	content_start: usize,
	quote: u8,
	delimiter_length: usize,
) -> String {
	let delimiter = char::from(quote).to_string().repeat(delimiter_length);
	let first_line = source[..content_start].matches('\n').count() + 1;

	for (number, line) in source[content_start..].split('\n').enumerate() {
		let text = line.trim_matches([' ', '\t']);

		// A line of only quotes, longer than the delimiter, is content.
		if text.ends_with(&delimiter) && !text.bytes().all(|byte| byte == quote) {
			return format!(
				". Its closing delimiter must start a line, so move the {delimiter} at the end of line {} to a new line",
				first_line + number
			);
		}
	}

	String::new()
}

/**
Decodes the escapes in one line of a `"""` block, which starts at `offset` in the source.
*/
fn unescape_line(
	source: &str,
	text: &str,
	offset: usize,
	output: &mut String,
) -> Result<(), ScalarError> {
	let mut chunk_start = 0;

	while let Some(length) = text[chunk_start..].find('\\') {
		let index = chunk_start + length;
		output.push_str(&text[chunk_start..index]);
		// `escape` takes a source offset, so that its errors point into the source. The index it returns is still inside `text`, because no escape continues past a line feed.
		chunk_start = escape(source, offset + index, output)? - offset;
	}

	output.push_str(&text[chunk_start..]);
	Ok(())
}
