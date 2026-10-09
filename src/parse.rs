/*!
The parser. It checks every rule in the spec and builds a tree in which every value and key knows its offset in the source, so that a deserialization error can say where it is.
*/

use crate::instant::describe_malformed_instant;
use crate::scalar::{
	self, MAX_DIAGNOSED_LENGTH, Scalar, ScalarError, describe_bad_number, describe_character,
	describe_unknown_word, has_date_prefix, is_bare_key, is_bare_key_byte,
	is_quotable_key_character, quoting_example,
};
use crate::tree::Key;
use crate::{Duration, Error, Instant, abbreviate};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::num::NonZeroUsize;

/**
The deepest nesting a document may have, which the spec fixes exactly: every reader accepts every document up to it, and rejects every document deeper. Every object and array counts, including a top-level object without braces.
*/
pub(crate) const MAX_DEPTH: usize = 100;

const KEY_QUOTING_HINT: &str =
	". A key that contains characters other than letters, digits, “_”, and “-” must be quoted";

/**
A value in a parsed document.
*/
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Node<'de> {
	pub kind: Kind<'de>,
	/**
	The byte offset of the value in the source. `None` for a value that did not come from a document.
	*/
	pub offset: Offset,
	/**
	The byte offset after the value, for a value that came from a document.
	*/
	pub end: usize,
}

impl<'de> From<Kind<'de>> for Node<'de> {
	/**
	A node that did not come from a document.
	*/
	fn from(kind: Kind<'de>) -> Self {
		Self {
			kind,
			offset: Offset::NONE,
			end: 0,
		}
	}
}

/**
A byte offset in the source, or none for a value that did not come from a document. It takes one word, not the two of an `Option<usize>`, which keeps the nodes of a large document small.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Offset(Option<NonZeroUsize>);

impl Offset {
	pub(crate) const NONE: Self = Self(None);

	pub(crate) fn at(offset: usize) -> Self {
		Self(NonZeroUsize::new(offset + 1))
	}

	pub(crate) fn get(self) -> Option<usize> {
		self.0.map(|offset| offset.get() - 1)
	}
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind<'de> {
	Null,
	Bool(bool),
	Int(i64),
	Float(f64),
	String(Cow<'de, str>),
	Instant(Instant),
	Duration(Duration),
	Array(Vec<Node<'de>>),
	Object(Object<'de>),
}

impl Kind<'_> {
	/**
	The type, with an article, for error messages: `an int`, `an object`.
	*/
	pub(crate) const fn describe(&self) -> &'static str {
		match self {
			Self::Null => "null",
			Self::Bool(_) => "a bool",
			Self::Int(_) => "an int",
			Self::Float(_) => "a float",
			Self::String(_) => "a string",
			Self::Instant(_) => "an instant",
			Self::Duration(_) => "a duration",
			Self::Array(_) => "an array",
			Self::Object(_) => "an object",
		}
	}
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Object<'de> {
	/**
	Members in the order they are written.
	*/
	pub members: Vec<(Cow<'de, str>, Member<'de>)>,
	/**
	The position of each member by key, built once an object is too large to search one member at a time. Boxed, so the many small objects stay small.
	*/
	#[allow(
		clippy::box_collection,
		reason = "The box keeps every object one word smaller, and most objects have no index."
	)]
	index: Option<Box<HashMap<Cow<'de, str>, usize>>>,
}

/**
The number of members up to which an object is searched one member at a time, which is faster than hashing for the small objects that config files are made of.
*/
const MAX_UNINDEXED_MEMBERS: usize = 16;

impl<'de> Object<'de> {
	pub(crate) fn new(members: Vec<(Cow<'de, str>, Member<'de>)>) -> Self {
		Self {
			members,
			index: None,
		}
	}

	pub(crate) fn position(&self, key: &str) -> Option<usize> {
		match &self.index {
			Some(index) => index.get(key).copied(),
			None => self
				.members
				.iter()
				.position(|(member_key, _)| member_key == key),
		}
	}

	pub(crate) fn push(&mut self, key: Cow<'de, str>, member: Member<'de>) -> usize {
		let position = self.members.len();

		if let Some(index) = &mut self.index {
			index.insert(key.clone(), position);
		} else if position == MAX_UNINDEXED_MEMBERS {
			let mut index: HashMap<Cow<'de, str>, usize> = self
				.members
				.iter()
				.enumerate()
				.map(|(position, (key, _))| (key.clone(), position))
				.collect();
			index.insert(key.clone(), position);
			self.index = Some(Box::new(index));
		}

		self.members.push((key, member));
		position
	}
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Member<'de> {
	/**
	The byte offset of the key. `None` for a member that did not come from a document.
	*/
	pub key_offset: Offset,
	pub value: Node<'de>,
}

/**
Parses a document.
*/
pub(crate) fn parse(source: &str) -> Result<Node<'_>, Error> {
	check_characters(source)?;

	let mut parser = Parser {
		source,
		bytes: source.as_bytes(),
		index: 0,
		document_start: 0,
	};

	parser
		.parse_document()
		.map_err(|error| Error::at(error.message, source, error.offset))
}

/**
Whether `text` is one valid value, so that an error message only suggests a fix that works.
*/
pub(crate) fn is_valid_value(text: &str) -> bool {
	parse(&format!("[{text}]"))
		.is_ok_and(|node| matches!(node.kind, Kind::Array(items) if items.len() == 1))
}

/**
The text of a document given as bytes, which must be UTF-8. The error gives the position of the first byte that is not.
*/
pub(crate) fn decode(bytes: &[u8]) -> Result<&str, Error> {
	std::str::from_utf8(bytes).map_err(|error| {
		let offset = error.valid_up_to();
		let valid = std::str::from_utf8(&bytes[..offset]).unwrap_or_default();

		match error.error_len() {
			None => Error::at(
				"Incomplete UTF-8 sequence at the end of the input",
				valid,
				offset,
			),
			Some(_) => Error::at(
				format!("Invalid UTF-8 byte 0x{:02X}", bytes[offset]),
				valid,
				offset,
			),
		}
	})
}

/**
Characters that are errors wherever they appear, so they are checked once, up front, as in the JS reference.
*/
fn check_characters(source: &str) -> Result<(), Error> {
	if source.starts_with('\u{FEFF}') {
		return Err(Error::at(
			"A byte order mark (BOM) is not allowed",
			source,
			0,
		));
	}

	let Some(offset) = find_control(source.as_bytes()) else {
		return Ok(());
	};

	let byte = source.as_bytes()[offset];

	if byte == b'\r' {
		return Err(Error::at(
			"A carriage return (U+000D) is not allowed anywhere. Use LF line endings",
			source,
			offset,
		));
	}

	Err(Error::at(
		format!(
			"A raw control character (U+{byte:04X}) is not allowed anywhere, including in strings and comments. In a string, write it as the escape \\u{{{byte:x}}} inside \"...\""
		),
		source,
		offset,
	))
}

/**
Whether a byte is a raw control character that SOML refuses: U+0000 to U+001F except tab and line feed, and U+007F.
*/
const fn is_control(byte: u8) -> bool {
	(byte < 0x20 && byte != b'\t' && byte != b'\n') || byte == 0x7F
}

/**
The offset of the first raw control character. Each block is checked without stopping early, which lets the compiler check many bytes at once, and only a block with a control character is searched byte by byte.
*/
fn find_control(bytes: &[u8]) -> Option<usize> {
	const BLOCK: usize = 32;

	for (block_index, block) in bytes.chunks(BLOCK).enumerate() {
		if block
			.iter()
			.fold(false, |found, &byte| found | is_control(byte))
		{
			return block
				.iter()
				.position(|&byte| is_control(byte))
				.map(|position| block_index * BLOCK + position);
		}
	}

	None
}

type Result<T, E = ScalarError> = std::result::Result<T, E>;

struct Parser<'de> {
	source: &'de str,
	bytes: &'de [u8],
	index: usize,
	/**
	Where the document's collection starts, after the comments and whitespace before it.
	*/
	document_start: usize,
}

impl<'de> Parser<'de> {
	#[cold]
	fn fail<T>(&self, message: impl Into<String>, offset: usize) -> Result<T> {
		Err(ScalarError::new(message, offset))
	}

	#[cold]
	fn fail_too_deep<T>(&self, offset: usize) -> Result<T> {
		self.fail(
			format!("The document is nested more than {MAX_DEPTH} levels deep"),
			offset,
		)
	}

	fn peek(&self) -> Option<u8> {
		self.bytes.get(self.index).copied()
	}

	fn is_at_end(&self) -> bool {
		self.index >= self.bytes.len()
	}

	fn parse_document(&mut self) -> Result<Node<'de>> {
		self.skip_trivia()?;
		self.document_start = self.index;

		if self.is_at_end() {
			return self.fail(
				"A document must contain an object or an array, but this one is empty",
				self.index,
			);
		}

		let value = match self.peek() {
			Some(b'{') => self.parse_object(1)?,
			Some(b'[') => self.parse_array(1)?,
			_ => self.parse_bare_object()?,
		};

		self.skip_trivia()?;

		if !self.is_at_end() {
			return self.fail(
				format!(
					"Unexpected {} after the end of the document",
					self.describe_here()
				),
				self.index,
			);
		}

		Ok(value)
	}

	/**
	`bare-object = entry ( entry-sep entry )*`, where a separator is one or more line breaks.
	*/
	fn parse_bare_object(&mut self) -> Result<Node<'de>> {
		let start = self.index;
		let mut object = Object::default();

		if let Err(error) = self.parse_entry(&mut object, 1) {
			return Err(self.diagnose_bare_value(start).unwrap_or(error));
		}

		// From the first member to the end of the last one's value. The text around them is not part of the object.
		let mut end = self.index;

		loop {
			let has_line_break = self.skip_trivia()?;

			if self.is_at_end() {
				return Ok(Node {
					kind: Kind::Object(object),
					offset: Offset::at(start),
					end,
				});
			}

			if self.peek() == Some(b',') {
				return self.fail(
					"Top-level entries are separated by line breaks, not commas. Use braces for a one-line object",
					self.index,
				);
			}

			if !has_line_break {
				return self.fail(
					format!(
						"Expected a line break before the next entry, but found {}{}",
						self.describe_here(),
						self.slash_comment_hint()
					),
					self.index,
				);
			}

			self.parse_entry(&mut object, 1)?;
			end = self.index;
		}
	}

	/**
	A whole document that is one scalar, such as `5` or an instant, fails as an entry. That failure is reported as what it is.
	*/
	#[cold]
	fn diagnose_bare_value(&mut self, start: usize) -> Option<ScalarError> {
		self.index = start;
		let is_bare_value =
			self.parse_value(1).is_ok() && self.skip_trivia().is_ok() && self.is_at_end();

		is_bare_value.then(|| ScalarError::new("A bare value is not a document. A document is an object or an array, so write it as `key: value` or `[value]`", start))
	}

	/**
	Skips spaces, tabs, line breaks, and comments, and returns whether it crossed a line break. A line break inside a block comment does not count.
	*/
	fn skip_trivia(&mut self) -> Result<bool> {
		let mut has_line_break = false;

		loop {
			match self.peek() {
				Some(b' ' | b'\t') => self.index += 1,
				Some(b'\n') => {
					has_line_break = true;
					self.index += 1;
				}
				Some(b'#') => {
					self.index = scalar::line_end(self.bytes, self.index);
				}
				Some(b'/') if self.bytes.get(self.index + 1) == Some(&b'*') => {
					self.skip_block_comment()?
				}
				_ => return Ok(has_line_break),
			}
		}
	}

	fn skip_block_comment(&mut self) -> Result<()> {
		let start = self.index;
		let end = block_comment_end(self.bytes, start);
		let Some(end) = end else {
			return self.fail("Unterminated block comment", start);
		};

		// The body may not contain `/*`. An opening that overlaps the closing `*/`, as in `/*/`, is not inside the body.
		if let Some(nested) = find(&self.bytes[start + 2..end], b"/*") {
			return self.fail(
				"Block comments cannot be nested, and their body may not contain “/*”",
				start + 2 + nested,
			);
		}

		self.index = end + 2;
		Ok(())
	}

	/**
	`entry = key ":" ws value`.
	*/
	fn parse_entry(&mut self, object: &mut Object<'de>, depth: usize) -> Result<()> {
		let key_start = self.index;
		let key = self.parse_key()?;

		if self.peek() == Some(b'.') {
			return self.fail_dot_in_key(key_start);
		}

		if self.peek() != Some(b':') {
			return self.fail_missing_colon(key_start);
		}

		let colon = self.index;
		self.index += 1;
		self.skip_trivia()?;
		let value = match self.parse_value(depth + 1) {
			Ok(value) => value,
			Err(error) => {
				self.diagnose_bad_value(&error, &key, key_start, colon)?;
				return Err(error);
			}
		};

		if object.position(&key).is_some() {
			return self.fail(format!("Duplicate key {}", describe_key(&key)), key_start);
		}

		object.push(
			key,
			Member {
				key_offset: Offset::at(key_start),
				value,
			},
		);

		Ok(())
	}

	/**
	Reports a value that failed to parse as what the entry was meant to be, when that is clear. `key` is the entry's key.
	*/
	#[cold]
	fn diagnose_bad_value(
		&mut self,
		error: &ScalarError,
		key: &str,
		key_start: usize,
		colon: usize,
	) -> Result<()> {
		// A key that contains a `:`, as in `12:30: 'lunch'`, ends at the first `:`, and the rest is read as the value.
		if self
			.bytes
			.get(colon + 1)
			.is_some_and(|&byte| is_bare_key_byte(byte))
		{
			self.diagnose_key_with_colon(key_start)?;
		}

		self.index = colon + 1;
		let is_value_on_next_line = self.skip_trivia()?;
		let value_start = self.index;
		let comment_hint = self.describe_comment_as_value(colon);

		if is_value_on_next_line {
			self.diagnose_missing_value(value_start, key, key_start, &comment_hint)?;
		}

		// A value that was left out at the end of the document or of an object.
		if !comment_hint.is_empty()
			&& error.offset == value_start
			&& matches!(self.bytes.get(value_start), None | Some(b'}'))
		{
			return self.fail(format!("{}{comment_hint}", error.message), value_start);
		}

		Ok(())
	}

	/**
	A bare key followed directly by `:` and more of a key, up to a `:` that ends the key, as in `a:b: 1`.
	*/
	fn diagnose_key_with_colon(&self, key_start: usize) -> Result<()> {
		for index in key_start..key_start + MAX_DIAGNOSED_LENGTH {
			match self.bytes.get(index) {
				Some(b':') => {
					if matches!(self.bytes.get(index + 1), None | Some(b'\n' | b' ' | b'\t')) {
						return self.fail(
							format!(
								"A key that contains “:” must be quoted, as in '{}'",
								abbreviate(&self.source[key_start..index], 40)
							),
							key_start,
						);
					}
				}
				Some(&byte) if is_bare_key_byte(byte) => {}
				_ => return Ok(()),
			}
		}

		Ok(())
	}

	/**
	The hint for a `#` directly after a `:`, as in `color: #FFF`, which starts a comment rather than a value.
	*/
	fn describe_comment_as_value(&self, colon: usize) -> String {
		let hash = skip_spaces(self.bytes, colon + 1);

		if self.bytes.get(hash) != Some(&b'#') {
			return String::new();
		}

		// A `#` followed by a space, or by another `#`, starts an ordinary comment. A separator or a closing bracket after the value is not part of it. Only the start of a long comment is read, which is cut short in the message anyway.
		let is_value_byte = |byte: &u8| !matches!(byte, b'\t' | b'\n' | b' ' | b',' | b']' | b'}');
		let rest = &self.bytes[hash + 1..utf16_limit(self.source, hash, MAX_DIAGNOSED_LENGTH)];

		if !rest
			.first()
			.is_some_and(|byte| *byte != b'#' && is_value_byte(byte))
		{
			return String::new();
		}

		let mut end = hash + 1 + rest.iter().take_while(|byte| is_value_byte(byte)).count();

		// The limit may cut a character.
		while !self.source.is_char_boundary(end) {
			end -= 1;
		}

		format!(
			". “#” starts a comment, so a value that starts with “#” must be quoted{}",
			quoting_example(&self.source[hash..end])
		)
	}

	/**
	An entry whose value was left out, as in `a:` followed by `'b': 1` on the next line, reads the next key as the value. That failure is reported as what it is. `parent` and `key_start` are the entry's key.
	*/
	#[cold]
	fn diagnose_missing_value(
		&mut self,
		start: usize,
		parent: &str,
		key_start: usize,
		comment_hint: &str,
	) -> Result<()> {
		// A YAML block sequence.
		if self.bytes.get(start) == Some(&b'-')
			&& matches!(self.bytes.get(start + 1), Some(b' ' | b'\t'))
		{
			return self.fail(
				"Expected a value, but found a “-” list. An array is written in brackets, as in [80, 443]",
				start,
			);
		}

		self.index = start;

		let Ok(key) = self.parse_key() else {
			// Not a key either, so the original error stands.
			return Ok(());
		};

		if self.peek() != Some(b':') {
			return Ok(());
		}

		// Whitespace or the end follows the `:` of a bare key. A digit follows the `:` in an instant such as `2026-09-19T25:00:00Z`, whose own error is more precise. A quoted key cannot be part of a value, so anything may follow its `:`.
		let is_quoted = matches!(self.bytes[start], b'\'' | b'"');

		if is_quoted
			|| matches!(
				self.bytes.get(self.index + 1),
				None | Some(b' ' | b'\t' | b'\n')
			) {
			let hint = if comment_hint.is_empty() {
				self.describe_indented_key(parent, &key, key_start, start)
			} else {
				comment_hint.to_owned()
			};

			return self.fail(
				format!(
					"Expected a value, but found the key {}{hint}",
					abbreviate(&describe_key(&key), 200)
				),
				start,
			);
		}

		Ok(())
	}

	/**
	The hint for a key at `start` that is indented under the entry whose value is missing, as YAML nests an object. `parent` and `key_start` are that entry's key, and `key` is the one found.
	*/
	fn describe_indented_key(
		&self,
		parent: &str,
		key: &str,
		key_start: usize,
		start: usize,
	) -> String {
		let (Some(key_indentation), Some(indentation)) =
			(self.indentation(key_start), self.indentation(start))
		else {
			return String::new();
		};

		if indentation <= key_indentation {
			return String::new();
		}

		// The keys are written as in a document, so that the suggestion is valid, rather than as JSON strings like the rest of the message, whose escapes, such as `\b`, are not all valid.
		let parent = Key::new(parent).to_string();
		let parent = abbreviate(&parent, 200);
		let child = Key::new(key).to_string();
		let child = abbreviate(&child, 200);
		format!(". Indentation does not nest objects, so write {parent}: {{{child}: …}}")
	}

	/**
	The number of spaces and tabs before `offset` on its line, or `None` when something else comes before it.
	*/
	fn indentation(&self, offset: usize) -> Option<usize> {
		let line_start = line_start(self.bytes, offset);
		(skip_spaces(self.bytes, line_start) == offset).then_some(offset - line_start)
	}

	#[cold]
	fn fail_missing_colon<T>(&mut self, key_start: usize) -> Result<T> {
		let mut next = self.index;

		while matches!(self.bytes.get(next), Some(b' ' | b'\t')) {
			next += 1;
		}

		let next_byte = self.bytes.get(next).copied();

		if next > self.index && next_byte == Some(b':') {
			return self.fail(
				"Whitespace is not allowed between a key and its “:”",
				self.index,
			);
		}

		// A block comment that the “:” follows was meant to come before it.
		if self.bytes[next..].starts_with(b"/*")
			&& let Some(comment_end) = block_comment_end(self.bytes, next)
			&& self.bytes[comment_end + 2..]
				.iter()
				.find(|&&byte| byte != b' ' && byte != b'\t')
				== Some(&b':')
		{
			return self.fail("A comment is not allowed between a key and its “:”", next);
		}

		let line_end = scalar::line_end(self.bytes, next);
		let colon = self.bytes[next..line_end]
			.iter()
			.position(|&byte| byte == b':')
			.map(|length| next + length);

		// A key with a space in it, such as `the name: 1`. The hint is only given for plain words, because quoting a quoted key would change what it means, and for a `:` that whitespace or the end follows, because quoting the words before the `:` in `server localhost:8080` would give a valid document with another meaning. So `the name:1` gets no hint.
		if next > self.index
			&& let Some(colon) = colon
			&& next_byte.is_some_and(is_bare_key_byte)
			&& matches!(self.bytes.get(colon + 1), None | Some(b' ' | b'\t' | b'\n'))
		{
			let key = self.source[key_start..colon].trim_end_matches([' ', '\t']);

			if key
				.bytes()
				.all(|byte| byte == b' ' || byte == b'\t' || is_bare_key_byte(byte))
			{
				return self.fail(
					format!(
						"A bare key cannot contain spaces. Quote it, as in '{}'",
						abbreviate(key, 40)
					),
					key_start,
				);
			}
		}

		if matches!(self.peek(), None | Some(b'\n')) {
			return self.fail("Expected “:” after the key", self.index);
		}

		// A visible character directly after a bare key is most likely meant to be part of the key.
		let is_quotable = next == self.index
			&& !matches!(self.bytes[next - 1], b'\'' | b'"')
			&& self.source[next..]
				.chars()
				.next()
				.is_some_and(is_quotable_key_character);
		let hint = if is_quotable { KEY_QUOTING_HINT } else { "" };
		self.index = next;
		self.fail(
			format!(
				"Expected “:” after the key, but found {}{hint}",
				self.describe_here()
			),
			next,
		)
	}

	fn parse_key(&mut self) -> Result<Cow<'de, str>> {
		let start = self.index;

		match self.peek() {
			Some(b'\'' | b'"') => {
				if scalar::is_block_string(self.bytes, start) {
					return self.fail("A block string cannot be a key", start);
				}

				// A key is a string, so it may hold anything a string can, including a line feed written as an escape.
				let (value, end) = scalar::string(self.source, start)?;
				self.index = end;
				Ok(value)
			}
			_ => {
				let end = start
					+ self.bytes[start..]
						.iter()
						.take_while(|&&byte| is_bare_key_byte(byte))
						.count();

				if end == start {
					if self.peek() == Some(b'[') {
						self.diagnose_table_header(false)?;
					}

					if self.is_at_end() {
						return self.fail("Expected a key", start);
					}

					return self.fail(
						format!(
							"Expected a key, but found {}{}{}",
							self.describe_here(),
							self.key_quoting_hint(),
							self.slash_comment_hint()
						),
						start,
					);
				}

				self.index = end;
				Ok(Cow::Borrowed(&self.source[start..end]))
			}
		}
	}

	/**
	The hint for a key that starts here with a character a bare key cannot hold, such as the `$` in `$schema`, with the key quoted when its “:” follows it. Empty for a character with a meaning of its own, such as `}`.
	*/
	fn key_quoting_hint(&self) -> String {
		let rest = &self.source[self.index..];

		if !rest.chars().next().is_some_and(is_quotable_key_character) {
			return String::new();
		}

		let length = rest
			.find(|character| !is_quotable_key_character(character))
			.unwrap_or(rest.len());

		// A longer key gets the hint without the example. The key and its `:` must fit in the length that is diagnosed, counted in UTF-16 code units, as in the JS reference implementation.
		let is_diagnosed = rest[..length].encode_utf16().count() < MAX_DIAGNOSED_LENGTH;

		if is_diagnosed && rest[length..].starts_with(':') {
			format!(
				"{KEY_QUOTING_HINT}, as in '{}'",
				abbreviate(&rest[..length], 40)
			)
		} else {
			KEY_QUOTING_HINT.to_owned()
		}
	}

	/**
	A `.` after a key, as in `example.com: 1`. A key is never a path, so the `.` is meant either as part of the key or as nesting.
	*/
	#[cold]
	fn fail_dot_in_key<T>(&self, key_start: usize) -> Result<T> {
		let end = self.index
			+ self.bytes[self.index..]
				.iter()
				.take(MAX_DIAGNOSED_LENGTH.saturating_sub(self.index - key_start))
				.take_while(|&&byte| is_bare_key_byte(byte) || byte == b'.')
				.count();
		let key = &self.source[key_start..end];

		// The suggestions are only given for a bare key whose dots each have a word on both sides, so that both are valid.
		if self.bytes.get(end) != Some(&b':')
			|| !is_bare_key_byte(self.bytes[key_start])
			|| key.split('.').any(str::is_empty)
		{
			return self.fail(
				"A key cannot contain “.” unless it is quoted. Quote the whole key, or use braces to nest, as in a: {b: …}",
				self.index,
			);
		}

		let (parents, last) = key.rsplit_once('.').expect("the key has a “.”");
		let mut nested = format!("{last}: …");

		for parent in parents.rsplit('.') {
			nested = format!("{parent}: {{{nested}}}");
		}

		self.fail(
			format!(
				"A bare key cannot contain “.”. Quote it, as in '{}', or use braces to nest, as in {}",
				abbreviate(key, 40),
				abbreviate(&nested, 40)
			),
			self.index,
		)
	}

	fn parse_object(&mut self, depth: usize) -> Result<Node<'de>> {
		let mut object = Object::default();
		let start = self.parse_braced(depth, Collection::Object, |parser| {
			parser.parse_entry(&mut object, depth)
		})?;

		Ok(Node {
			kind: Kind::Object(object),
			offset: Offset::at(start),
			end: self.index,
		})
	}

	fn parse_array(&mut self, depth: usize) -> Result<Node<'de>> {
		let mut items = Vec::new();

		let start = self.parse_braced(depth, Collection::Array, |parser| {
			items.push(parser.parse_value(depth + 1)?);
			Ok(())
		})?;

		Ok(Node {
			kind: Kind::Array(items),
			offset: Offset::at(start),
			end: self.index,
		})
	}

	/**
	Reads a braced container that opens here, with `item` reading one item. The items are separated by a comma, a line break, or both, and a trailing comma is allowed. A comma must be on the line of the item before it. Returns the offset of the opening bracket.
	*/
	fn parse_braced(
		&mut self,
		depth: usize,
		collection: Collection,
		mut item: impl FnMut(&mut Self) -> Result<()>,
	) -> Result<usize> {
		let start = self.index;
		let closing = collection.closing();
		let closing_text = char::from(closing);
		let (container, item_name) = collection.names();

		if depth > MAX_DEPTH {
			return self.fail_too_deep(start);
		}

		self.index += 1;
		self.skip_trivia()?;

		loop {
			if self.peek() == Some(closing) {
				self.index += 1;
				return Ok(start);
			}

			if self.is_at_end() {
				return self.fail(
					format!("Unterminated {container}: expected “{closing_text}”"),
					start,
				);
			}

			item(self)?;
			let item_end = self.index;
			let has_line_break = self.skip_trivia()?;

			// Items are separated by a comma, a line break, or both. A line break inside a block comment does not count.
			match self.peek() {
				Some(b',') => {
					if has_line_break {
						return self.fail(
							"A comma must be on the same line as the item before it. The line break already separates the items, so remove the comma",
							self.index,
						);
					}

					self.index += 1;
					self.skip_trivia()?;
				}
				Some(byte) if byte == closing || has_line_break => {}
				None => {
					return self.fail(
						format!("Unterminated {container}: expected “{closing_text}”"),
						start,
					);
				}
				Some(_) => {
					// Without a line break that separates, a line break in the gap is inside a block comment.
					let hint = if self.bytes[item_end..self.index].contains(&b'\n') {
						". A line break inside a block comment does not separate items"
					} else {
						self.slash_comment_hint()
					};

					return self.fail(
						format!(
							"Expected “,”, a line break, or “{closing_text}” after an {container} {item_name}, but found {}{hint}",
							self.describe_here()
						),
						self.index,
					);
				}
			}
		}
	}

	fn parse_value(&mut self, depth: usize) -> Result<Node<'de>> {
		let start = self.index;
		// An int or a float written with digits.
		let mut is_number = false;

		let kind = match self.peek() {
			Some(b'{') => return self.parse_object(depth),
			Some(b'[') => return self.parse_array(depth),
			Some(b'\'' | b'"') => {
				let (value, end) = scalar::string(self.source, start)?;
				self.index = end;
				Kind::String(value)
			}
			Some(b't') if self.eat_keyword("true") => Kind::Bool(true),
			Some(b'f') if self.eat_keyword("false") => Kind::Bool(false),
			Some(b'n') if self.eat_keyword("null") => Kind::Null,
			Some(b'i') if self.eat_keyword("infinity") => Kind::Float(f64::INFINITY),
			Some(b'-') if self.eat_keyword("-infinity") => Kind::Float(f64::NEG_INFINITY),
			Some(b'0'..=b'9' | b'-') => {
				if let Some((kind, end)) = self.plain_number(start) {
					self.index = end;

					if self.is_word_after_space() {
						self.diagnose_unit_after_space(start)?;
					}

					return Ok(Node {
						kind,
						offset: Offset::at(start),
						end,
					});
				}

				let end = scalar::number_end(self.bytes, start);
				self.index = end;

				match scalar::token_value(&self.source[start..end]) {
					Some(Ok(Scalar::Int(value))) => {
						is_number = true;
						Kind::Int(value)
					}
					Some(Ok(Scalar::Float(value))) => {
						is_number = true;
						Kind::Float(value)
					}
					Some(Ok(Scalar::Instant(value))) => Kind::Instant(value),
					Some(Ok(Scalar::Duration(value))) => Kind::Duration(value),
					Some(Err(message)) => {
						let message = self
							.describe_malformed_instant(start, end)
							.unwrap_or(message);
						return self.fail(message, start);
					}
					None => return self.fail(self.describe_bad_number(start, end), start),
				}
			}
			_ => return self.fail_unexpected_value(),
		};

		// What may directly follow a scalar: whitespace, a separator, a closing bracket, a comment, or the end.
		if self.peek().is_some_and(|next| !can_follow_value(next)) {
			return self.fail_after_value(start, is_number);
		}

		if is_number && self.is_word_after_space() {
			self.diagnose_unit_after_space(start)?;
		}

		Ok(Node {
			kind,
			offset: Offset::at(start),
			end: self.index,
		})
	}

	/**
	A character that cannot follow the value that starts at `start`.
	*/
	#[cold]
	fn fail_after_value<T>(&self, start: usize, is_number: bool) -> Result<T> {
		let rest = &self.source[self.index..];

		// Go writes microseconds as `µs`, with the micro sign or the Greek letter mu. The duration is only suggested when it is valid, which a number with an exponent or a radix prefix, for example, is not.
		if is_number && (rest.starts_with("\u{B5}s") || rest.starts_with("\u{3BC}s")) {
			let number = &self.source[start..self.index];
			let example = if is_valid_value(&format!("{number}us")) {
				format!(", as in {}us", abbreviate(number, 40))
			} else {
				String::new()
			};

			return self.fail(
				format!("The unit for microseconds is written us{example}"),
				self.index,
			);
		}

		// A `''` inside a '...' string, as SQL and YAML escape a quote, ends the string.
		if self.peek() == Some(b'\'') && self.bytes[start] == b'\'' {
			return self.fail(
				"There is no '' escape in a '...' string. Write a string that contains ' as \"...\", as in \"it's\"",
				self.index,
			);
		}

		self.fail(
			format!("Unexpected {} after a value", self.describe_here()),
			self.index,
		)
	}

	/**
	Whether spaces or tabs and then a letter follow, as in `512 MiB`. Checked before `diagnose_unit_after_space`, which this keeps off the path of a valid document.
	*/
	fn is_word_after_space(&self) -> bool {
		matches!(self.peek(), Some(b' ' | b'\t'))
			&& self
				.bytes
				.get(skip_spaces(self.bytes, self.index))
				.is_some_and(u8::is_ascii_alphabetic)
	}

	/**
	A number followed by a space and a word, such as `512 MiB` or `10 seconds`, which is an error anyway. A word followed by more than spaces, a separator, or a comment is left to the general errors, because it may be a key, as in `a: 1 b: 2`, or a sentence.
	*/
	#[cold]
	fn diagnose_unit_after_space(&self, start: usize) -> Result<()> {
		let unit_start = skip_spaces(self.bytes, self.index);
		let unit_end = unit_start
			+ self.bytes[unit_start..]
				.iter()
				.take_while(|byte| byte.is_ascii_alphabetic())
				.count();
		let unit = &self.source[unit_start..unit_end];

		// A keyword after a number is a missing comma.
		if unit.is_empty()
			|| self
				.bytes
				.get(skip_spaces(self.bytes, unit_end))
				.is_some_and(|&byte| !can_follow_value(byte))
			|| matches!(unit, "true" | "false" | "null" | "infinity")
		{
			return Ok(());
		}

		let number = &self.source[start..self.index];

		// The duration is only suggested when it is valid, which a number with an exponent or a radix prefix, a fraction of a nanosecond, a negative zero, or a value outside the 64-bit range is not. The string is always offered, because a unit such as `m` may mean meters rather than minutes.
		let duration = if matches!(unit, "h" | "m" | "s" | "ms" | "us" | "ns")
			&& is_valid_value(&format!("{number}{unit}"))
		{
			format!("{}{unit}", abbreviate(number, 40))
		} else {
			"10s".to_owned()
		};

		self.fail(
			format!(
				"A unit cannot follow a number after a space. Write a duration without the space, as in {duration}, and anything else, such as a size, as a string, as in '{}'",
				abbreviate(&format!("{number} {unit}"), 40)
			),
			unit_start,
		)
	}

	/**
	The reason the token from `start` to `end` is not an instant, when it starts like a date but does not have the form of one. The reason depends on the text after it: a space and a time, as TOML and Python write a date and time, or more that may be part of the instant.
	*/
	#[cold]
	fn describe_malformed_instant(&self, start: usize, end: usize) -> Option<String> {
		let text = &self.source[start..end];

		if !has_date_prefix(text.as_bytes()) {
			return None;
		}

		let time_start = end + 1;
		let time_end = if self.bytes.get(end) == Some(&b' ')
			&& self.bytes.get(time_start).is_some_and(u8::is_ascii_digit)
		{
			scalar::number_end(self.bytes, time_start)
		} else {
			end
		};
		let time = if time_end > end {
			&self.source[time_start..time_end]
		} else {
			""
		};

		// More of the instant may follow, as in `14:00:00,5Z` or `14:00:00[Europe/Oslo]`, so it may have an offset.
		let next = self.bytes.get(time_end).copied();
		let is_whole_value = next.is_none_or(can_follow_value)
			&& !(next == Some(b',')
				&& self.bytes.get(time_end + 1).is_some_and(u8::is_ascii_digit));

		describe_malformed_instant(text, time, is_whole_value)
	}

	/**
	The error for the token from `start` to `end`, which is no number, instant, or duration.
	*/
	#[cold]
	fn describe_bad_number(&self, start: usize, end: usize) -> String {
		let text = &self.source[start..end];

		self.describe_thousands_separator(text, start)
			.unwrap_or_else(|| describe_bad_number(text, self.unquoted_text(start)))
	}

	/**
	A comma used as a thousands separator, as in `[1,000]`, makes the group after it a separate item, which fails only when it has a leading zero. Writing that item in octal, as the leading zero message suggests, would parse to the wrong items.
	*/
	fn describe_thousands_separator(&self, text: &str, start: usize) -> Option<String> {
		let bytes = self.bytes;

		if !(text.len() == 3
			&& text.starts_with('0')
			&& text.bytes().all(|byte| byte.is_ascii_digit()))
			|| start == 0
			|| bytes[start - 1] != b','
		{
			return None;
		}

		let group_start = |group_end: usize| {
			group_end
				- bytes[..group_end]
					.iter()
					.rev()
					.take_while(|byte| byte.is_ascii_digit())
					.count()
		};
		let mut group_end = start - 1;
		let mut number_start = group_start(group_end);

		// Earlier groups of three digits, as in `12,345,000`.
		while group_end - number_start == 3
			&& number_start >= 2
			&& bytes[number_start - 1] == b','
			&& bytes[number_start - 2].is_ascii_digit()
		{
			group_end = number_start - 1;
			number_start = group_start(group_end);
		}

		let before = number_start.checked_sub(1).map(|index| bytes[index]);

		// The first group has one to three digits, which are not the end of a float, of a number with underscores, or of a number with a radix prefix.
		if number_start == group_end
			|| matches!(before, Some(b'.' | b'_'))
			|| number_start + 3 < group_end
			|| before.is_some_and(|byte| byte.is_ascii_alphabetic())
		{
			return None;
		}

		// The sign belongs to the number, as in `-1,000`.
		if before == Some(b'-') {
			number_start -= 1;
		}

		// The groups of three digits after it, as in `,000` in `1,000,000`.
		let limit = bytes.len().min(start + text.len() + MAX_DIAGNOSED_LENGTH);
		let mut groups_end = start + text.len();

		while groups_end + 4 <= limit
			&& bytes[groups_end] == b','
			&& bytes[groups_end + 1..groups_end + 4]
				.iter()
				.all(u8::is_ascii_digit)
			&& !(groups_end + 4 < limit && bytes[groups_end + 4].is_ascii_digit())
		{
			groups_end += 4;
		}

		let number = &self.source[number_start..groups_end];
		let reason = format!(
			"Leading zeros are not allowed in a decimal number. A comma separates items, so {} is not one number",
			abbreviate(number, 40)
		);
		// Both spellings are the same int, so they are suggested only when it is in range.
		let with_underscores = number.replace(',', "_");

		Some(if is_valid_value(&with_underscores) {
			format!(
				"{reason}. Write it as {} or {}",
				abbreviate(&with_underscores, 40),
				abbreviate(&number.replace(',', ""), 40)
			)
		} else {
			reason
		})
	}

	/**
	The common case of a short decimal int or float, such as `8080`, `-3`, or `30.5`, without the general path's scan and checks. Anything else, including every error, returns `None` and is left to the general path.
	*/
	fn plain_number(&self, start: usize) -> Option<(Kind<'de>, usize)> {
		let bytes = self.bytes;
		let integer_start = start + usize::from(bytes[start] == b'-');
		let integer_end = integer_start
			+ bytes[integer_start..]
				.iter()
				.take_while(|byte| byte.is_ascii_digit())
				.count();
		let integer_length = integer_end - integer_start;

		// No digits, or a leading zero, which is either `0` alone or an error.
		if integer_length == 0 || (integer_length > 1 && bytes[integer_start] == b'0') {
			return None;
		}

		let mut end = integer_end;

		if bytes.get(end) == Some(&b'.') {
			let fraction_length = bytes[end + 1..]
				.iter()
				.take_while(|byte| byte.is_ascii_digit())
				.count();

			if fraction_length == 0 {
				return None;
			}

			end += 1 + fraction_length;
		}

		// At most 18 digits, so an int cannot overflow. A character that may continue a number, such as `e`, `_`, or `:`, needs the general path.
		if end - integer_start > 18 || bytes.get(end).is_some_and(|&byte| !can_follow_value(byte)) {
			return None;
		}

		let text = &self.source[start..end];

		if end > integer_end {
			let value: f64 = text.parse().ok()?;

			// Negative zero is the same value as zero.
			return Some((Kind::Float(if value == 0.0 { 0.0 } else { value }), end));
		}

		// `-0` is an error, which the general path reports.
		if text == "-0" {
			return None;
		}

		Some((Kind::Int(text.parse().ok()?), end))
	}

	/**
	Whether the keyword is here, as a whole word, and if so, moves past it. A longer word, such as `nullable`, is not the keyword.
	*/
	fn eat_keyword(&mut self, word: &str) -> bool {
		let end = self.index + word.len();

		if !self.bytes[self.index..].starts_with(word.as_bytes())
			|| self.bytes.get(end).copied().is_some_and(is_bare_key_byte)
		{
			return false;
		}

		self.index = end;
		true
	}

	#[cold]
	fn fail_unexpected_value<T>(&self) -> Result<T> {
		let start = self.index;

		match self.peek() {
			None => {
				return self.fail(
					"Expected a value, but reached the end of the document",
					start,
				);
			}
			Some(b'+') => {
				return self.fail(
					"A “+” sign is not allowed. A number without a sign is positive",
					start,
				);
			}
			// A `.` that no digit follows begins a string, such as `.env` or `./foo`, rather than a number.
			Some(b'.') => {
				if self.bytes.get(start + 1).is_some_and(u8::is_ascii_digit) {
					return self.fail(
						"A number cannot begin with “.”; write a digit before it, as in 0.5",
						start,
					);
				}

				return self.fail(describe_unknown_word(".", self.unquoted_text(start)), start);
			}
			_ => {}
		}

		let word_end = start
			+ self.bytes[start..]
				.iter()
				.take_while(|&&byte| is_bare_key_byte(byte))
				.count();

		if word_end > start {
			let word = &self.source[start..word_end];

			// A key where a value should be, as when the value of an entry is left out and the next line has a key, or as in `[a: 1]`, is reported as a key. A word after the `:` of a member and a space, as in `msg: Error: file not found` or `url: https://example.com`, is that member's value instead, an unquoted string. Without the space, as in `a:b: 1`, the first `:` was most likely meant as part of the key.
			let spaces_start = skip_spaces_back(self.bytes, start);
			let is_member_value =
				spaces_start < start && spaces_start > 0 && self.bytes[spaces_start - 1] == b':';

			if !is_member_value && self.bytes.get(word_end) == Some(&b':') {
				return self.fail(
					format!(
						"Expected a value, but found the key {}",
						abbreviate(word, 40)
					),
					start,
				);
			}

			self.diagnose_table_header(true)?;
			return self.fail(
				describe_unknown_word(word, self.unquoted_text(start)),
				start,
			);
		}

		let hint = if self.is_block_scalar_indicator() {
			". Write a multiline string as a block string, between \'\'\' lines"
		} else {
			self.slash_comment_hint()
		};

		self.fail(
			format!("Expected a value, but found {}{hint}", self.describe_here()),
			start,
		)
	}

	/**
	The text from `start` that was most likely meant as one unquoted string, such as `John Smith`: up to the end of the line, a comma, a closing bracket, or a comment after a space or a tab, without the spaces and tabs at its end.
	*/
	fn unquoted_text(&self, start: usize) -> &'de str {
		let bytes = self.bytes;
		let limit = utf16_limit(self.source, start, MAX_DIAGNOSED_LENGTH);
		let is_end = |index: usize| {
			let is_comment = bytes[index] == b'#'
				|| (bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*'));
			matches!(bytes[index], b'\n' | b',' | b']' | b'}')
				|| (is_comment && index > 0 && matches!(bytes[index - 1], b' ' | b'\t'))
		};
		let mut end = start;

		while end < limit && !is_end(end) {
			end += 1;
		}

		// The limit may cut a character.
		while !self.source.is_char_boundary(end) {
			end -= 1;
		}

		self.source[start..end].trim_end_matches([' ', '\t'])
	}

	/**
	A TOML table header, as in `[server]` or `[[servers]]` on a line of its own, reads as an array that holds a word, or as a key that starts with “[”. Where a value is expected, `is_value`, it is only a table header when its bracket opens the document, because a table cannot be written as an item of an array inside it.
	*/
	fn diagnose_table_header(&self, is_value: bool) -> Result<()> {
		let bytes = self.bytes;
		let line_start = line_start(bytes, self.index);

		if is_value && skip_spaces(bytes, line_start) != self.document_start {
			return Ok(());
		}

		let line_end = scalar::line_end(bytes, self.index).min(line_start + MAX_DIAGNOSED_LENGTH);
		let line = bytes[line_start..line_end].trim_ascii_end();
		let line = &line[skip_spaces(line, 0)..];
		let Some(inner) = line.strip_prefix(b"[") else {
			return Ok(());
		};
		let inner = inner.strip_prefix(b"[").unwrap_or(inner);
		let name_length = inner
			.iter()
			.take_while(|&&byte| is_bare_key_byte(byte))
			.count();
		let (name, closing) = inner.split_at(name_length);

		// The name is a valid key, so that the suggestion is valid.
		let is_name = name
			.first()
			.is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_');

		if !is_name || !matches!(closing, b"]" | b"]]") {
			return Ok(());
		}

		let name = abbreviate(std::str::from_utf8(name).expect("the name is ASCII"), 40);
		self.fail(
			format!(
				"There are no table headers. Write the table as an object, as in {name}: {{…}}"
			),
			self.index,
		)
	}

	/**
	A YAML literal block scalar indicator, as in `key: |` or `key: |-`, at the end of its line.
	*/
	fn is_block_scalar_indicator(&self) -> bool {
		// A folded block scalar, `>`, joins its lines, which a block string does not, so only `|` gets the hint.
		if self.peek() != Some(b'|') {
			return false;
		}

		let chomping = usize::from(matches!(self.bytes.get(self.index + 1), Some(b'-' | b'+')));
		let end = skip_spaces(self.bytes, self.index + 1 + chomping);
		matches!(self.bytes.get(end), None | Some(b'\n'))
	}

	/**
	The hint for a `//` comment, as in JavaScript.
	*/
	fn slash_comment_hint(&self) -> &'static str {
		if self.bytes[self.index..].starts_with(b"//") {
			". A comment starts with “#”"
		} else {
			""
		}
	}

	fn describe_here(&self) -> String {
		match self.source[self.index..].chars().next() {
			None => "the end of the document".to_owned(),
			Some('\n') => "a line break".to_owned(),
			Some(character) => describe_character(character),
		}
	}
}

/**
A collection written with brackets.
*/
#[derive(Clone, Copy)]
enum Collection {
	Object,
	Array,
}

impl Collection {
	/**
	The byte that closes it.
	*/
	const fn closing(self) -> u8 {
		match self {
			Self::Object => b'}',
			Self::Array => b']',
		}
	}

	/**
	The names of the collection and of one of its items, for error messages.
	*/
	const fn names(self) -> (&'static str, &'static str) {
		match self {
			Self::Object => ("object", "member"),
			Self::Array => ("array", "item"),
		}
	}
}

/**
Whether a byte may directly follow a scalar: whitespace, a separator, a closing bracket, a comment, or the end.
*/
const fn can_follow_value(byte: u8) -> bool {
	matches!(
		byte,
		b' ' | b'\t' | b'\n' | b',' | b']' | b'}' | b'#' | b'/'
	)
}

/**
The index of the closing delimiter of the block comment that starts at `start`.
*/
pub(crate) fn block_comment_end(bytes: &[u8], start: usize) -> Option<usize> {
	find(&bytes[start + 2..], b"*/").map(|length| start + 2 + length)
}

fn find(haystack: &[u8], needle: &[u8; 2]) -> Option<usize> {
	haystack.windows(2).position(|window| window == needle)
}

/**
The index after the spaces and tabs from `index`.
*/
fn skip_spaces(bytes: &[u8], index: usize) -> usize {
	index
		+ bytes
			.get(index..)
			.unwrap_or_default()
			.iter()
			.take_while(|byte| matches!(byte, b' ' | b'\t'))
			.count()
}

/**
The index after the last byte before `index` that is not a space or a tab.
*/
fn skip_spaces_back(bytes: &[u8], index: usize) -> usize {
	index
		- bytes[..index]
			.iter()
			.rev()
			.take_while(|byte| matches!(byte, b' ' | b'\t'))
			.count()
}

/**
The index where the line that `index` is on starts.
*/
fn line_start(bytes: &[u8], index: usize) -> usize {
	bytes[..index]
		.iter()
		.rposition(|&byte| byte == b'\n')
		.map_or(0, |position| position + 1)
}

/**
The byte offset in `text` that is `length` UTF-16 code units after `start`, or the end of `text`, so that a diagnosed length counts as in the JS reference implementation. A character that would cross the limit is left out.
*/
fn utf16_limit(text: &str, start: usize, length: usize) -> usize {
	let mut units = 0;

	for (index, character) in text[start..].char_indices() {
		units += character.len_utf16();

		if units > length {
			return start + index;
		}
	}

	text.len()
}

/**
A key for an error message, such as `port` or `"the name"`: bare when it can be, and otherwise as a JSON string, as the JS reference implementation writes it.
*/
fn describe_key(key: &str) -> String {
	let mut text = String::new();
	write_described_key(&mut text, key);
	text
}

/**
Writes a key for an error message, as the JS reference implementation writes it: cut to 40 characters, and bare when it can be, and otherwise as a JSON string.
*/
pub(crate) fn write_described_key(output: &mut String, key: &str) {
	// Whether the key needs quotes depends on the whole key, not on its shortened text, which ends in `…`.
	let short = abbreviate(key, 40);

	if is_bare_key(key) {
		output.push_str(&short);
	} else {
		write_json_string(output, &short);
	}
}

/**
Writes `text` as `JSON.stringify()` does: `"` and `\` are escaped, as are the control characters below U+0020, with a short escape where JSON has one and `\u00xx` otherwise.
*/
fn write_json_string(output: &mut String, text: &str) {
	output.push('"');

	for character in text.chars() {
		match character {
			'"' => output.push_str("\\\""),
			'\\' => output.push_str("\\\\"),
			'\u{8}' => output.push_str("\\b"),
			'\t' => output.push_str("\\t"),
			'\n' => output.push_str("\\n"),
			'\u{C}' => output.push_str("\\f"),
			'\r' => output.push_str("\\r"),
			'\0'..='\u{1F}' => {
				write!(output, "\\u{:04x}", u32::from(character))
					.expect("writing to a String does not fail");
			}
			_ => output.push(character),
		}
	}

	output.push('"');
}
