/*!
Writes a value as text, by the rules of canonical form: every member and item is on its own line, so a line break separates them, and no commas are written. Canonical form, the one text the spec defines for a value, so that equal values give equal bytes, also sorts the members of every object by key, which `Node::sort_members` does before the value is written. Without it, members keep the order they were given, which is the order a person reading the file expects, and every other rule still holds.
*/

use crate::Error;
use crate::parse::{Kind, Node};
use crate::scalar::is_bare_key;
use std::fmt::{self, Write};

/**
Writes a document. The value must be an array or an object, and the serializer must have accepted it, which checks everything else that SOML cannot represent.
*/
pub(crate) fn document(value: &Node<'_>) -> Result<String, Error> {
	let mut output = String::new();

	match &value.kind {
		Kind::Object(object) if !object.members.is_empty() => {
			// A non-empty top-level object is written without braces, with its members at column 0.
			for (key, member) in &object.members {
				write_member(&mut output, key, &member.value, 0);
				output.push('\n');
			}
		}
		// An empty object keeps its braces, because an empty document is not valid.
		Kind::Object(_) | Kind::Array(_) => {
			write_value(&mut output, value, 0);
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

fn write_member(output: &mut String, key: &str, value: &Node<'_>, indentation: usize) {
	write_key(output, key).expect("writing to a String does not fail");
	output.push_str(": ");
	write_value(output, value, indentation);
}

/**
Writes a value as it appears after a key or as an array item, with `indentation` tabs for the lines inside it.
*/
pub(crate) fn write_value(output: &mut String, value: &Node<'_>, indentation: usize) {
	match &value.kind {
		Kind::Null => output.push_str("null"),
		Kind::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
		Kind::Int(value) => write_int(output, *value),
		Kind::Float(value) => write_float(output, *value),
		Kind::String(value) => {
			write_string(output, value).expect("writing to a String does not fail");
		}
		Kind::Instant(value) => write_display(output, value),
		Kind::Duration(value) => write_display(output, value),
		Kind::Array(items) => {
			if items.is_empty() {
				output.push_str("[]");
				return;
			}

			output.push('[');

			for item in items {
				push_line(output, indentation + 1);
				write_value(output, item, indentation + 1);
			}

			push_line(output, indentation);
			output.push(']');
		}
		Kind::Object(object) => {
			if object.members.is_empty() {
				output.push_str("{}");
				return;
			}

			output.push('{');

			for (key, member) in &object.members {
				push_line(output, indentation + 1);
				write_member(output, key, &member.value, indentation + 1);
			}

			push_line(output, indentation);
			output.push('}');
		}
	}
}

/**
Writes a line feed and `count` tabs, the start of a new line at that indentation.
*/
pub(crate) fn push_line(output: &mut String, count: usize) {
	// Most lines are indented a few tabs, and single pushes, which are inlined, are faster than a copy then.
	if count <= 4 {
		output.push('\n');

		for _ in 0..count {
			output.push('\t');
		}

		return;
	}

	// Enough for all but the deepest nesting, so indentation is nearly always one copy.
	const LINE: &str = "\n\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t";
	const TAB_COUNT: usize = LINE.len() - 1;
	output.push_str(&LINE[..=count.min(TAB_COUNT)]);

	if count > TAB_COUNT {
		output.extend(std::iter::repeat_n('\t', count - TAB_COUNT));
	}
}

/**
Writes an int in decimal, without the formatting machinery, which is several times slower for this.
*/
pub(crate) fn write_int(output: &mut String, value: i64) {
	if value < 0 {
		output.push('-');
	}

	output.push_str(decimal(value.unsigned_abs(), &mut [0; 20]));
}

/**
The decimal digits of `value`, put at the end of `digits`, which holds the 20 digits of the largest `u64`.
*/
pub(crate) fn decimal(mut value: u64, digits: &mut [u8; 20]) -> &str {
	let mut start = digits.len();

	loop {
		start -= 1;
		digits[start] = b'0' + (value % 10) as u8;
		value /= 10;

		if value == 0 {
			break;
		}
	}

	std::str::from_utf8(&digits[start..]).expect("the digits are ASCII")
}

pub(crate) fn write_display(output: &mut String, value: &(impl fmt::Display + ?Sized)) {
	write!(output, "{value}").expect("writing to a String does not fail");
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
	const ONES: u64 = u64::from_ne_bytes([0x01; 8]);
	const HIGHS: u64 = u64::from_ne_bytes([0x80; 8]);

	// Whether a byte of the word is zero. A borrow can only start at a zero byte, so it never makes a word without one look like it has one.
	let has_zero = |word: u64| word.wrapping_sub(ONES) & !word & HIGHS != 0;
	let needs = |byte: u8| byte == b'\'' || byte < 0x20 || byte == 0x7F;
	let (words, remainder) = text.as_bytes().as_chunks::<8>();

	// It checks 8 bytes at a time, as `scalar::find_any` does. A byte below 0x20 is the one that the subtraction of 0x20 takes below zero.
	words.iter().any(|word| {
		let word = u64::from_ne_bytes(*word);
		word.wrapping_sub(ONES * 0x20) & !word & HIGHS != 0
			|| has_zero(word ^ (ONES * u64::from(b'\'')))
			|| has_zero(word ^ (ONES * 0x7F))
	}) || remainder.iter().any(|&byte| needs(byte))
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

	write_escaped_string(output, text)
}

/**
Writes a string that `needs_escapes` accepts as `'...'`.
*/
pub(crate) fn write_literal_string(output: &mut String, text: &str) {
	output.reserve(text.len() + 2);
	output.push('\'');
	output.push_str(text);
	output.push('\'');
}

/**
Writes a string as `"..."` with escapes.
*/
pub(crate) fn write_escaped_string(output: &mut impl Write, text: &str) -> fmt::Result {
	output.write_char('"')?;
	let mut chunk_start = 0;

	// Every byte that is escaped is ASCII, so `index` is always at a character boundary.
	for (index, byte) in text.bytes().enumerate() {
		let escape = match byte {
			b'\\' => "\\\\",
			b'"' => "\\\"",
			b'\n' => "\\n",
			b'\t' => "\\t",
			// No short escape, so it is written as `\u{…}` below.
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

The digits come from `zmij`, because when two shortest digit strings are equally close to the value, the spec requires the even one, and `std` picks the higher one. The layout is written here, because it is part of the spec, and a dependency's own layout can change. zmij's own text is used only where it has no exponent, and a test checks that it is the same layout there.
*/
pub(crate) fn write_float(output: &mut String, value: f64) {
	debug_assert!(!value.is_nan(), "the serializer rejects NaN");

	if value.is_infinite() {
		output.push_str(if value > 0.0 { "infinity" } else { "-infinity" });
		return;
	}

	// Zero has one value whatever its sign.
	if value == 0.0 {
		output.push_str("0.0");
		return;
	}

	if value < 0.0 {
		output.push('-');
	}

	let mut buffer = zmij::Buffer::new();
	let text = buffer.format_finite(value.abs());

	// zmij writes no exponent only from about 1e-5 to 1e16, where canonical form has the same text, such as `61.171`, `0.5`, or `180.0`. An exponent is at most `e-324`, so its `e` is in the last 5 bytes, and a loop over them is faster than a search of short text.
	if !text.bytes().rev().take(5).any(|byte| byte == b'e') {
		output.push_str(text);
		return;
	}

	write_layout(output, text);
}

/**
Writes the digits of zmij's `text` for a positive float in the layout of canonical form.
*/
fn write_layout(output: &mut String, text: &str) {
	let mut digit_buffer = [0; 32];
	let (digits, exponent) = shortest_digits(text, &mut digit_buffer);
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn needs_escapes_finds_each_byte_at_each_place_in_a_word() {
		// Every ASCII byte at every place of a string of up to 2 words, between plain letters or between bytes of `é`, which are 0x80 and above.
		for filler in ["a", "é"] {
			for length in 1..=17 {
				for place in 0..length {
					for byte in 0..=0x7F_u8 {
						let mut text: Vec<u8> = filler.repeat(length).into_bytes();
						let place = place * filler.len();
						text[place] = byte;

						// Replacing a byte of `é` leaves invalid UTF-8, so the other byte becomes a letter too.
						if filler.len() == 2 {
							text[place ^ 1] = b'a';
						}

						let text = String::from_utf8(text).expect("valid UTF-8");
						let expected = byte == b'\'' || byte < 0x20 || byte == 0x7F;
						assert_eq!(needs_escapes(&text), expected, "{text:?}");
					}
				}
			}
		}
	}

	#[test]
	fn the_text_of_zmij_without_an_exponent_is_in_canonical_layout() {
		// Every power of ten and the floats next to it cover the edges of zmij's range. A simple generator adds short decimals, such as `61.171`, and floats with all their digits, at every power of ten in between.
		let powers = (-30..=30).flat_map(|exponent| {
			let value = 10_f64.powi(exponent);
			[value.next_down(), value, value.next_up()]
		});
		let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
		let others = std::iter::repeat_with(move || {
			state ^= state << 13;
			state ^= state >> 7;
			state ^= state << 17;
			let power = 10_f64.powi((state % 30) as i32 - 10);

			if state & (1 << 40) == 0 {
				((state >> 20) % 1_000_000) as f64 * power
			} else {
				(state >> 11) as f64 / (1_u64 << 53) as f64 * power
			}
		});
		let mut compared = 0;

		for value in powers
			.chain(others.take(1_000_000))
			.filter(|value| *value > 0.0)
		{
			let mut buffer = zmij::Buffer::new();
			let text = buffer.format_finite(value);

			if text.contains('e') {
				continue;
			}

			let mut expected = String::new();
			write_layout(&mut expected, text);
			assert_eq!(text, expected, "{value:e}");
			compared += 1;
		}

		assert!(compared > 500_000, "{compared}");
	}
}
