use std::fmt::{self, Display, Write};

/**
An error from reading or writing SOML.

A document that is not valid SOML, a document that does not fit the type it is read into, and a value that SOML cannot represent all give an `Error`. [`kind`](Self::kind) tells them apart. When the error is about a place in a document, it has a line and a column, and [`code_frame`](Self::code_frame) shows that place.

```rust
let error = soml::from_str::<soml::Value>("a: 1\na: 2").unwrap_err();

assert_eq!(error.kind(), soml::ErrorKind::Syntax);
assert_eq!(error.message(), "Duplicate key a");
assert_eq!(error.position(), Some(soml::LineColumn { line: 2, column: 1 }));
assert_eq!(error.to_string(), "Duplicate key a at line 2, column 1");
```
*/
pub struct Error {
	// Boxed, so that a `Result` that holds an `Error` stays small. serde_json measured a larger error type to be substantially slower.
	inner: Box<Inner>,
}

struct Inner {
	kind: ErrorKind,
	message: String,
	position: Option<Position>,
	source: Option<std::io::Error>,
}

/**
What kind of problem an [`Error`] is about.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
	/**
	The text is not valid SOML: a document, or an instant or a duration read from text.
	*/
	Syntax,
	/**
	Valid SOML that does not fit what it is read into, such as a string for a number field, a value that is out of range for a conversion, or a path that does not fit a document.
	*/
	Data,
	/**
	A value that cannot be written as SOML, such as NaN or a carriage return, or a [`Document::set`](crate::Document::set) that would make the document invalid.
	*/
	Write,
	/**
	Reading or writing failed, in [`from_reader`](crate::from_reader) or [`to_writer`](crate::to_writer). The `io::Error` is the error's [`source`](std::error::Error::source).
	*/
	Io,
}

/**
A place in a document's text, as a person counts it: a 1-based line, and a 1-based column in Unicode scalar values.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LineColumn {
	/**
	The 1-based line.
	*/
	pub line: usize,
	/**
	The 1-based column, counted in Unicode scalar values.
	*/
	pub column: usize,
}

#[derive(Clone, Copy)]
struct Position {
	line_column: LineColumn,
	offset: usize,
}

impl Error {
	#[cold]
	pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
		Self {
			inner: Box::new(Inner {
				kind,
				message: message.into(),
				position: None,
				source: None,
			}),
		}
	}

	#[cold]
	pub(crate) fn syntax(message: impl Into<String>) -> Self {
		Self::new(ErrorKind::Syntax, message)
	}

	/**
	A data error. The message can quote the document, such as an unknown key in serde's messages, so its control characters are hidden, as in a syntax error. Line feeds stay, because a message of your own can have more than one line.
	*/
	#[cold]
	pub(crate) fn data(message: impl Into<String>) -> Self {
		let message = message
			.into()
			.chars()
			.map(|character| {
				if character == '\n' {
					character
				} else {
					sanitize(character)
				}
			})
			.collect::<String>();

		Self::new(ErrorKind::Data, message)
	}

	#[cold]
	pub(crate) fn write(message: impl Into<String>) -> Self {
		Self::new(ErrorKind::Write, message)
	}

	#[cold]
	pub(crate) fn io(error: std::io::Error) -> Self {
		let mut io = Self::new(ErrorKind::Io, error.to_string());
		io.inner.source = Some(error);
		io
	}

	/**
	A syntax error at a byte offset in `source`. The message can quote the document, so its control characters are hidden, as in a code frame.
	*/
	#[cold]
	pub(crate) fn at(message: impl Into<String>, source: &str, offset: usize) -> Self {
		let mut error = Self::syntax(message.into().chars().map(sanitize).collect::<String>());
		error.inner.position = Some(Position::locate(source, offset));
		error
	}

	/**
	An error of your own about a place in a document, such as a value that is valid SOML but not valid for your app. Use the byte offset of a [`Spanned`](crate::Spanned) value or of a syntax tree node. An offset past the end of `text` counts as its end. The error's kind is [`ErrorKind::Data`].

	```rust
	#[derive(serde::Deserialize)]
	struct Server {
		port: soml::Spanned<u16>,
	}

	let text = "# The server.\nport: 0";
	let server: Server = soml::from_str(text)?;

	if *server.port == 0 {
		let start = server.port.span().unwrap().start;
		let error = soml::Error::with_position("Port 0 is reserved", text, start);
		assert_eq!(error.to_string(), "Port 0 is reserved at line 2, column 7");
	}
	# Ok::<(), soml::Error>(())
	```
	*/
	#[must_use]
	pub fn with_position(message: impl Into<String>, text: &str, offset: usize) -> Self {
		let mut error = Self::data(message);
		error.inner.position = Some(Position::locate(text, offset));
		error
	}

	/**
	Gives the error a position, unless it already has one. The innermost position is the most precise one, because it is the value or key the error is about.
	*/
	#[cold]
	pub(crate) fn or_at(mut self, source: Option<&str>, offset: Option<usize>) -> Self {
		if self.inner.position.is_none()
			&& let Some(source) = source
			&& let Some(offset) = offset
		{
			self.inner.position = Some(Position::locate(source, offset));
		}

		self
	}

	/**
	What kind of problem the error is about.
	*/
	#[must_use]
	pub fn kind(&self) -> ErrorKind {
		self.inner.kind
	}

	/**
	What is wrong, without the position.
	*/
	#[must_use]
	pub fn message(&self) -> &str {
		&self.inner.message
	}

	/**
	The line and column of the error, if it is about a place in a document.
	*/
	#[must_use]
	pub fn position(&self) -> Option<LineColumn> {
		self.inner.position.map(|position| position.line_column)
	}

	/**
	The 0-based byte offset of the error in the document, if it is about a place in a document.
	*/
	#[must_use]
	pub fn offset(&self) -> Option<usize> {
		self.inner.position.map(|position| position.offset)
	}

	/**
	Up to three lines of `text` that end at the error, with a caret under its position, for a terminal. Give it the text the error came from. `None` when the error is not about a place in a document.

	A long line is cut to a part around the position, and control characters, bidirectional controls, and other characters a terminal could act on are shown as U+FFFD. Tabs are kept, so the caret lines up whatever the tab width is. The layout is the same as the `codeFrame` of the JS reference implementation, except that a long line is cut by characters rather than by UTF-16 code units.

	```rust
	let text = "name: 'api'\nport: 80\nhost: localhost";
	let error = soml::from_str::<soml::Value>(text).unwrap_err();

	assert_eq!(error.code_frame(text).unwrap(), "  1 | name: 'api'\n  2 | port: 80\n> 3 | host: localhost\n    |       ^");
	```

	For an error from [`from_slice`](crate::from_slice) about bytes that are not UTF-8, use `String::from_utf8_lossy` for the text: it matches the bytes up to the error.
	*/
	#[must_use]
	pub fn code_frame(&self, text: &str) -> Option<String> {
		const CONTEXT_LINES: usize = 2;
		let position = self.inner.position?;

		// The text may not be the one the error came from, so the offset is kept inside it.
		let offset = character_start(text, position.offset);

		let line = LineColumn::locate(text, offset).line;
		let error_line_start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
		let mut line_starts = vec![error_line_start];

		while line_starts.len() <= CONTEXT_LINES && line_starts[0] > 0 {
			let previous_end = line_starts[0] - 1;
			line_starts.insert(
				0,
				text[..previous_end]
					.rfind('\n')
					.map_or(0, |index| index + 1),
			);
		}

		let first_line = line + 1 - line_starts.len();
		let gutter_width = line.to_string().len();
		let mut frame = String::new();

		for (index, &line_start) in line_starts.iter().enumerate() {
			let number = first_line + index;
			let is_error_line = number == line;
			let line_end = text[line_start..]
				.find('\n')
				.map_or(text.len(), |length| line_start + length);
			let pointer = if is_error_line {
				text[line_start..offset].chars().count()
			} else {
				0
			};
			let (characters, pointer) = clip(&text[line_start..line_end], pointer);
			let marker = if is_error_line { '>' } else { ' ' };

			if index > 0 {
				frame.push('\n');
			}

			write!(frame, "{marker} {number:>gutter_width$} |")
				.expect("writing to a String does not fail");

			if !characters.is_empty() {
				frame.push(' ');
				frame.extend(characters.iter());
			}

			if is_error_line {
				let padding: String = characters[..pointer]
					.iter()
					.map(|&character| if character == '\t' { '\t' } else { ' ' })
					.collect();
				write!(frame, "\n  {} | {padding}^", " ".repeat(gutter_width))
					.expect("writing to a String does not fail");
			}
		}

		Some(frame)
	}
}

/**
A character that a terminal could act on rather than show, as U+FFFD: a control character other than tab, and a bidirectional control.
*/
pub(crate) fn sanitize(character: char) -> char {
	let is_bidirectional_control = matches!(character, '\u{61C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');

	if (character.is_control() && character != '\t') || is_bidirectional_control {
		'\u{FFFD}'
	} else {
		character
	}
}

/**
The characters of a line for a code frame, as [`sanitize`] shows them, cut to a window around the pointer when the line is long, with `…` where it was cut. Returns the characters and the pointer's new position. Only the window is collected, so a long line costs no more memory than a short one.
*/
fn clip(line: &str, pointer: usize) -> (Vec<char>, usize) {
	const MAX_LINE_WIDTH: usize = 100;

	let length = line.chars().count();

	if length <= MAX_LINE_WIDTH {
		return (line.chars().map(sanitize).collect(), pointer);
	}

	let start = pointer
		.saturating_sub(MAX_LINE_WIDTH / 2)
		.min(length - MAX_LINE_WIDTH);
	let end = start + MAX_LINE_WIDTH;
	let mut clipped = Vec::with_capacity(MAX_LINE_WIDTH + 2);

	if start > 0 {
		clipped.push('…');
	}

	clipped.extend(line.chars().skip(start).take(MAX_LINE_WIDTH).map(sanitize));

	if end < length {
		clipped.push('…');
	}

	let pointer = pointer - start + usize::from(start > 0);
	(clipped, pointer)
}

impl Position {
	/**
	The position of a byte offset. Only called when an error is created, so a document that reads without error never pays for it.
	*/
	fn locate(source: &str, offset: usize) -> Self {
		let offset = character_start(source, offset);

		Self {
			line_column: LineColumn::locate(source, offset),
			offset,
		}
	}
}

/**
A byte offset kept inside `text`, at the start of a character: an offset past the end is the end, and an offset inside a character, which an error of your own can have, is the start of that character.
*/
fn character_start(text: &str, offset: usize) -> usize {
	let mut offset = offset.min(text.len());

	while !text.is_char_boundary(offset) {
		offset -= 1;
	}

	offset
}

impl LineColumn {
	/**
	The line and column of a byte offset in `source`. An offset past the end counts as the end, and an offset inside a character points at that character.
	*/
	pub(crate) fn locate(source: &str, offset: usize) -> Self {
		let offset = character_start(source, offset);

		let before = &source.as_bytes()[..offset];
		let line_start = before
			.iter()
			.rposition(|&byte| byte == b'\n')
			.map_or(0, |index| index + 1);
		let line = before.iter().filter(|&&byte| byte == b'\n').count() + 1;

		// A UTF-8 continuation byte does not start a scalar value, so counting the other bytes counts the scalar values.
		let column = before[line_start..]
			.iter()
			.filter(|&&byte| byte & 0xC0 != 0x80)
			.count() + 1;

		Self { line, column }
	}
}

/**
Writes `line 2, column 7`.
*/
impl Display for LineColumn {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "line {}, column {}", self.line, self.column)
	}
}

impl Display for Error {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.inner.position {
			Some(position) => write!(
				formatter,
				"{} at {}",
				self.inner.message, position.line_column
			),
			None => formatter.write_str(&self.inner.message),
		}
	}
}

impl fmt::Debug for Error {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		let mut debug = formatter.debug_struct("Error");
		debug.field("kind", &self.inner.kind);
		debug.field("message", &self.inner.message);

		if let Some(position) = self.inner.position {
			debug.field("line", &position.line_column.line);
			debug.field("column", &position.line_column.column);
		}

		debug.finish()
	}
}

impl std::error::Error for Error {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		self.inner
			.source
			.as_ref()
			.map(|error| error as &(dyn std::error::Error + 'static))
	}
}

impl serde_core::de::Error for Error {
	#[cold]
	fn custom<T: Display>(message: T) -> Self {
		Self::data(message.to_string())
	}
}

impl serde_core::ser::Error for Error {
	#[cold]
	fn custom<T: Display>(message: T) -> Self {
		Self::write(message.to_string())
	}
}
