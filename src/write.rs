/*!
Writes a value as text, by the rules of canonical form: every member and item is on its own line, so a line break separates them, and no commas are written. Canonical form, the one text the spec defines for a value, so that equal values give equal bytes, also sorts the members of every object by key. Without it, members keep the order they were given, which is the order a person reading the file expects, and every other rule still holds.
*/

use crate::Error;
use crate::parse::{Kind, MAX_DEPTH, Member, Node, Object};
use crate::scalar::is_bare_key;
use std::borrow::Cow;
use std::fmt::{self, Write};

/**
Writes a document, in canonical form when `canonical` is true. The value must be an array or an object.
*/
pub(crate) fn document(value: &Node<'_>, canonical: bool) -> Result<String, Error> {
	let mut output = String::new();

	match &value.kind {
		Kind::Object(object) if !object.members.is_empty() => {
			// A non-empty top-level object is written without braces, with its members at column 0.
			for (key, member) in members(object, canonical) {
				write_member(&mut output, key, &member.value, 0, 1, canonical)?;
				output.push('\n');
			}
		}
		Kind::Object(_) | Kind::Array(_) => {
			write_value(&mut output, value, 0, 0, canonical)?;
			output.push('\n');
		}
		kind => {
			return Err(Error::write(format!(
				"A document must be an object or an array, not {}",
				kind.describe()
			)));
		}
	}

	Ok(output)
}

/**
The members of an object in the order they were given, or sorted by key for canonical form.
*/
fn members<'a, 'de>(
	object: &'a Object<'de>,
	canonical: bool,
) -> Vec<&'a (Cow<'de, str>, Member<'de>)> {
	let mut members: Vec<_> = object.members.iter().collect();

	if canonical {
		// A `str` compares byte by byte, which for UTF-8 is the order of Unicode scalar values, as canonical form requires. Keys are unique, so the sort does not need to be stable.
		members.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
	}

	members
}

fn write_member(
	output: &mut String,
	key: &str,
	value: &Node<'_>,
	indentation: usize,
	depth: usize,
	canonical: bool,
) -> Result<(), Error> {
	check_representable(key, "key")?;
	write_key(output, key).expect("writing to a String does not fail");
	output.push_str(": ");
	write_value(output, value, indentation, depth, canonical)
}

/**
Writes a value as it appears after a key or as an array item, with `indentation` tabs for the lines inside it. `depth` is the number of collections around it.
*/
pub(crate) fn write_value(
	output: &mut String,
	value: &Node<'_>,
	indentation: usize,
	depth: usize,
	canonical: bool,
) -> Result<(), Error> {
	match &value.kind {
		Kind::Null => output.push_str("null"),
		Kind::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
		Kind::Int(value) => write!(output, "{value}").expect("writing to a String does not fail"),
		Kind::Float(value) => write_float(output, *value)?,
		Kind::String(value) => {
			check_representable(value, "string")?;
			write_string(output, value).expect("writing to a String does not fail");
		}
		Kind::Instant(value) => {
			write!(output, "{value}").expect("writing to a String does not fail")
		}
		Kind::Duration(value) => {
			write!(output, "{value}").expect("writing to a String does not fail")
		}
		Kind::Array(items) => {
			check_depth(depth + 1)?;

			if items.is_empty() {
				output.push_str("[]");
				return Ok(());
			}

			output.push_str("[\n");

			for item in items {
				push_tabs(output, indentation + 1);
				write_value(output, item, indentation + 1, depth + 1, canonical)?;
				output.push('\n');
			}

			push_tabs(output, indentation);
			output.push(']');
		}
		Kind::Object(object) => {
			check_depth(depth + 1)?;

			if object.members.is_empty() {
				output.push_str("{}");
				return Ok(());
			}

			output.push_str("{\n");

			for (key, member) in members(object, canonical) {
				push_tabs(output, indentation + 1);
				write_member(
					output,
					key,
					&member.value,
					indentation + 1,
					depth + 1,
					canonical,
				)?;
				output.push('\n');
			}

			push_tabs(output, indentation);
			output.push('}');
		}
	}

	Ok(())
}

fn push_tabs(output: &mut String, count: usize) {
	// Enough for all but the deepest nesting, so indentation is nearly always one copy.
	const TABS: &str = "\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t";
	output.push_str(&TABS[..count.min(TABS.len())]);
	output.extend(std::iter::repeat_n('\t', count.saturating_sub(TABS.len())));
}

fn check_depth(depth: usize) -> Result<(), Error> {
	if depth > MAX_DEPTH {
		return Err(too_deep());
	}

	Ok(())
}

#[cold]
pub(crate) fn too_deep() -> Error {
	Error::write(format!(
		"The value is nested more than {MAX_DEPTH} levels deep, so no reader would accept the document"
	))
}

/**
A carriage return is the one character SOML cannot represent, not even as an escape.
*/
pub(crate) fn check_representable(text: &str, what: &str) -> Result<(), Error> {
	if text.contains('\r') {
		return Err(Error::write(format!(
			"A {what} cannot contain a carriage return (U+000D), because SOML cannot represent one"
		)));
	}

	Ok(())
}

/**
Writes a key: bare when it is non-empty and every character is a letter, a digit, `_`, or `-`, and otherwise as a string.
*/
pub(crate) fn write_key(output: &mut impl Write, key: &str) -> fmt::Result {
	if is_bare_key(key) {
		output.write_str(key)
	} else {
		write_string(output, key)
	}
}

/**
Whether a string needs `"..."`: it has a `'`, a tab, a line feed, another C0 control, or U+007F.
*/
pub(crate) fn needs_escapes(text: &str) -> bool {
	text.bytes()
		.any(|byte| byte == b'\'' || byte < 0x20 || byte == 0x7F)
}

/**
Writes a string: `'...'` unless the content has a `'`, a tab, a line feed, another C0 control, or U+007F, and then `"..."` with escapes. A `\` or a `"` alone does not need `"..."`, because a literal string holds both as they are.
*/
pub(crate) fn write_string(output: &mut impl Write, text: &str) -> fmt::Result {
	if !needs_escapes(text) {
		output.write_char('\'')?;
		output.write_str(text)?;
		return output.write_char('\'');
	}

	output.write_char('"')?;
	let mut chunk_start = 0;

	for (index, byte) in text.bytes().enumerate() {
		let escape = match byte {
			b'\\' => "\\\\",
			b'"' => "\\\"",
			b'\n' => "\\n",
			b'\t' => "\\t",
			0x00..0x20 | 0x7F => "",
			_ => continue,
		};

		output.write_str(&text[chunk_start..index])?;

		if escape.is_empty() {
			write!(output, "\\u{{{byte:x}}}")?;
		} else {
			output.write_str(escape)?;
		}

		chunk_start = index + 1;
	}

	output.write_str(&text[chunk_start..])?;
	output.write_char('"')
}

/**
Writes a float in canonical form (spec rule 11): the shortest digits that read back as the same value, laid out like ECMAScript's `Number::toString`, with `.0` when it would otherwise read as an int.

The digits come from `zmij`, because when two shortest digit strings are equally close to the value, the spec requires the even one, and `std` picks the higher one. The layout is written here, because it is part of the spec, and a dependency's own layout can change.
*/
pub(crate) fn write_float(output: &mut String, value: f64) -> Result<(), Error> {
	if value.is_nan() {
		return Err(Error::write("NaN is not a SOML value"));
	}

	if value.is_infinite() {
		output.push_str(if value > 0.0 { "infinity" } else { "-infinity" });
		return Ok(());
	}

	// Zero has one value whatever its sign.
	if value == 0.0 {
		output.push_str("0.0");
		return Ok(());
	}

	if value < 0.0 {
		output.push('-');
	}

	let mut buffer = zmij::Buffer::new();
	let mut digit_buffer = [0; 32];
	let (digits, exponent) = shortest_digits(buffer.format_finite(value.abs()), &mut digit_buffer);
	// The value is 0.d₁d₂…dₖ × 10ⁿ.
	let count = digits.len() as i32;
	let point = exponent + count;

	if count <= point && point <= 21 {
		output.push_str(digits);
		output.extend(std::iter::repeat_n('0', (point - count) as usize));
		output.push_str(".0");
	} else if 0 < point && point < count {
		output.push_str(&digits[..point as usize]);
		output.push('.');
		output.push_str(&digits[point as usize..]);
	} else if -6 < point && point <= 0 {
		output.push_str("0.");
		output.extend(std::iter::repeat_n('0', (-point) as usize));
		output.push_str(digits);
	} else {
		output.push_str(&digits[..1]);

		if count > 1 {
			output.push('.');
			output.push_str(&digits[1..]);
		}

		write!(output, "e{}", point - 1).expect("writing to a String does not fail");
	}

	Ok(())
}

/**
The significant digits of a positive decimal number, without leading or trailing zeros, and the power of ten to multiply them by. Accepts any spelling `zmij` writes, such as `1.5e+300`, `1e-7`, or `123.45`.
*/
fn shortest_digits<'a>(text: &str, buffer: &'a mut [u8; 32]) -> (&'a str, i32) {
	let (significand, exponent) = match text.split_once(['e', 'E']) {
		Some((significand, exponent)) => (
			significand,
			exponent
				.trim_start_matches('+')
				.parse::<i32>()
				.expect("zmij writes a valid exponent"),
		),
		None => (text, 0),
	};

	let (whole, fraction) = significand.split_once('.').unwrap_or((significand, ""));
	let mut length = 0;

	// Without leading zeros, which change nothing.
	for byte in whole
		.bytes()
		.chain(fraction.bytes())
		.skip_while(|&byte| byte == b'0')
	{
		buffer[length] = byte;
		length += 1;
	}

	// Without trailing zeros, which move into the exponent.
	let significant = buffer[..length]
		.iter()
		.rposition(|&byte| byte != b'0')
		.map_or(0, |last| last + 1);
	let exponent = exponent - fraction.len() as i32 + (length - significant) as i32;
	(
		std::str::from_utf8(&buffer[..significant]).expect("the digits are ASCII"),
		exponent,
	)
}
