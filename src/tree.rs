/*!
A lossless syntax tree, for tools that read and change SOML documents.

A [`Document`](crate::Document) holds the tree. An unchanged tree prints the exact text it was parsed from, so comments, blank lines, indentation, member order, and the author's spelling of every value survive. A changed tree keeps all of that outside the change.

Whitespace and comments between tokens are [`Decor`], and every piece of it belongs to exactly one node:

- The text between an item and the next token (after the item's comma, if it has one) is split at its first line feed outside a block comment. The part up to and including that line feed is the item's trailing decor: usually a comment at the end of its line. The rest is the next item's leading decor: the comment lines above it, blank lines, and its indentation. When there is no line feed, as in `{a: 1, b: 2}`, all of it is the next item's leading decor. The one exception is the end of a document without a final line feed, where the rest of the last line, such as a comment, is the last member's trailing decor.
- The text after the last item, before `}` or `]`, is the container's closing decor. The text before and after a braced top-level collection belongs to the document.
- In a braced container, each item records whether it has a comma.

# Formatting

[`Document::format`](crate::Document::format) is the formatter in the spec's "Formatting" section. It keeps comments where they are (a comment on its own line stays on its own line, and a comment at the end of a line stays at the end of that line), member order, the spelling of every key, string, number, instant, and duration, block strings, and a top level with or without braces. It changes only layout: one tab of indentation per level, every member and item on its own line, no commas (a line break separates the members and items), no trailing spaces or tabs, at most one blank line in a row, and one line feed at the end. An object or an array whose brackets are on one line stays on one line, as in `ports: [80, 443]`, with a comma and a space between its members or items. To give a one-line container one member or item per line, put a line break anywhere inside it, also inside a comment or a block string.

In detail, as the spec states:

- No blank line directly after an opening bracket, directly before a closing one, or at the start or end of the document.
- An empty `{}` or `[]` is written on one line. A container that holds only comments is not empty: on one line it stays on one line, as in `[/* note */]`, and otherwise its closing bracket begins a line of its own.
- In a container on one line, no space is directly inside its brackets or before a comma, the comma after the last member or item goes, and block comments stay where they are among the members or items and commas, so `[1, /* end */]` becomes `[1 /* end */]`.
- Where a comma goes, a comment between the value and the comma stays at the end of the value's line. Every other comment keeps its line.
- A value on the line after its `key:` moves up to that line, unless it is a block string or a comment comes between them. Then each comment keeps its line, and the value goes on a new line, one level deeper.
- In a container that spans lines, a comment between two items on one line, as in `1, /* c */ 2`, goes before the second item, on its line. At the end of a container or of the document, a comment with no line break before it stays at the end of the line before.
- A block string begins on the line after its key, one level deeper, after any comments on the key's line. Its opening delimiter, its content, and its closing delimiter have one indentation: that of the line it begins on, which for an array item is the item's own. Its content keeps the indentation it has past the closing delimiter's, so its value does not change. A line of only spaces and tabs in a block string stays as written, as the spec's layout rules do not apply inside a block string.
- The lines of a block comment keep their indentation, and only the line it starts on is indented. As on every line outside a block string, trailing spaces and tabs go, and runs of blank lines collapse to one. A block comment that spans lines is part of the line it ends on.
*/

use crate::scalar::{self, Scalar as Token, is_bare_key_byte};
use crate::{Value, write};
use std::fmt::Write;
use std::ops::Range;

pub(crate) mod format;

/**
The whitespace and comments around a member or an array item, or around a top-level collection written with braces.
*/
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Decor {
	/**
	The text before the item: the comment lines and blank lines above it, and its indentation. For a top-level collection, the text before it.
	*/
	pub leading: String,
	/**
	The text after the item and its comma, up to and including the line feed that ends its line, or for a top-level collection, the text after it: usually empty, a line feed, or a comment at the end of the line and a line feed. At the end of a document without a final line feed, it is the rest of the last line, such as a comment, without a line feed.
	*/
	pub trailing: String,
}

/**
A node in the tree: an object, an array, or a scalar.
*/
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
	/**
	An object, with or without braces.
	*/
	Object(Object),
	/**
	An array.
	*/
	Array(Array),
	/**
	A string, number, bool, null, instant, or duration.
	*/
	Scalar(Scalar),
}

/**
An object, with its members in the order they are written.
*/
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
	members: Vec<Member>,
	is_braced: bool,
	closing: String,
	span: Option<Range<usize>>,
}

/**
A member of an object: a key and a value.
*/
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
	key: Key,
	/**
	The text between the `:` and the value, usually one space.
	*/
	pub(crate) after_colon: String,
	value: Node,
	separator: Separator,
	span: Option<Range<usize>>,
}

/**
An empty object, written with braces, so it can be a member's value.
*/
impl Default for Object {
	fn default() -> Self {
		Self {
			members: Vec::new(),
			is_braced: true,
			closing: String::new(),
			span: None,
		}
	}
}

/**
An array.
*/
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Array {
	items: Vec<Item>,
	closing: String,
	span: Option<Range<usize>>,
}

/**
An item of an array.
*/
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
	value: Node,
	separator: Separator,
}

/**
The decor of a member or an item, and its comma.
*/
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Separator {
	pub(crate) decor: Decor,
	/**
	Whether the item has a comma after it. An item on the same line as the next one always gets one when printed. In a top-level object without braces, commas are never printed.
	*/
	pub(crate) has_comma: bool,
	/**
	The text between the value and its comma, usually empty.
	*/
	pub(crate) before_comma: String,
}

/**
A member of an object or an item of an array, which both have decor and may have a comma, so that inserting, removing, and printing them is one piece of code. A member of a top-level object without braces has decor too, and its comma is never printed.
*/
pub(crate) trait Entry {
	fn separator(&self) -> &Separator;
	fn separator_mut(&mut self) -> &mut Separator;
	fn node_mut(&mut self) -> &mut Node;
	/**
	Writes the entry without its separator's decor: a member's key, the text after its `:`, and its value, or an item's value.
	*/
	fn print(&self, printer: &mut Printer);
}

/**
The entries of a container and its closing decor, to insert and remove entries. `is_braced` is false only for a top-level object without braces, where every entry starts a line.
*/
pub(crate) struct Entries<'a, T> {
	pub list: &'a mut Vec<T>,
	pub closing: &'a mut String,
	pub is_braced: bool,
}

impl Entry for Member {
	fn separator(&self) -> &Separator {
		&self.separator
	}

	fn separator_mut(&mut self) -> &mut Separator {
		&mut self.separator
	}

	fn node_mut(&mut self) -> &mut Node {
		&mut self.value
	}

	fn print(&self, printer: &mut Printer) {
		printer.member(self);
	}
}

impl Entry for Item {
	fn separator(&self) -> &Separator {
		&self.separator
	}

	fn separator_mut(&mut self) -> &mut Separator {
		&mut self.separator
	}

	fn node_mut(&mut self) -> &mut Node {
		&mut self.value
	}

	fn print(&self, printer: &mut Printer) {
		printer.node(&self.value);
	}
}

/**
A key of a member.

Like the other nodes, a key compares equal to another only when it is also written the same way and is at the same place, so a parsed key `'port'` is not equal to `Key::new("port")`. Compare [`value`](Self::value) for the key itself.
*/
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
	value: String,
	raw: Option<String>,
	span: Option<Range<usize>>,
}

/**
A scalar: its value, and the text the author wrote for it.
*/
#[derive(Debug, Clone, PartialEq)]
pub struct Scalar {
	value: Value,
	raw: Option<String>,
	span: Option<Range<usize>>,
}

/**
How a string or a key is written.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StringStyle {
	/**
	A bare key, without quotes. Only a key can be bare.
	*/
	Bare,
	/**
	`'...'`, with no escapes.
	*/
	Literal,
	/**
	`"..."`, with escapes.
	*/
	Escaped,
	/**
	`'''...'''`, a literal block string.
	*/
	LiteralBlock,
	/**
	`"""..."""`, a block string with escapes.
	*/
	EscapedBlock,
}

impl StringStyle {
	fn of(raw: &str) -> Self {
		match raw.as_bytes() {
			[b'\'', b'\'', b'\'', ..] => Self::LiteralBlock,
			[b'"', b'"', b'"', ..] => Self::EscapedBlock,
			[b'\'', ..] => Self::Literal,
			[b'"', ..] => Self::Escaped,
			_ => Self::Bare,
		}
	}
}

/**
The two kinds of comment.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentKind {
	/**
	`# ...`, to the end of its line.
	*/
	Line,
	/**
	`/* ... */`, which may span lines.
	*/
	Block,
}

/**
The radix an int is written in. Its discriminant is the radix, so `radix as u32` is 2, 8, 10, or 16.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Radix {
	/**
	`0b`, as in `0b1010`.
	*/
	Binary = 2,
	/**
	`0o`, as in `0o644`.
	*/
	Octal = 8,
	/**
	No prefix, as in `-42`.
	*/
	Decimal = 10,
	/**
	`0x`, as in `0xFF`.
	*/
	Hexadecimal = 16,
}

/**
A comment in a document.
*/
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
	/**
	The text of the comment, without its delimiters.
	*/
	pub text: String,
	/**
	Whether it is a `#` line comment or a `/* */` block comment.
	*/
	pub kind: CommentKind,
	/**
	The byte range of the comment, with its delimiters, in the document's current text.
	*/
	pub span: Range<usize>,
}

impl Node {
	/**
	A node for a value, printed in canonical form, with no decor. Use it to replace a value through [`Member::value_mut`] or [`Item::value_mut`], which keeps the decor around the value.

	```rust
	let mut document: soml::Document = "port: 1 # The default.".parse()?;
	let root = document.root_mut().as_object_mut().unwrap();
	*root.members_mut()[0].value_mut() = soml::tree::Node::new(8080)?;
	assert_eq!(document.to_string(), "port: 8080 # The default.");
	# Ok::<(), soml::Error>(())
	```

	# Errors

	Returns an error when the value holds something SOML cannot represent: NaN, a carriage return, or nesting of 100 levels or more, because a node is always inside a collection.
	*/
	pub fn new(value: impl Into<Value>) -> Result<Self, crate::Error> {
		Ok(Self::from_value(checked(value.into())?, ""))
	}

	/**
	A node for a value, with its members and items on their own lines, indented by `indentation` and one tab more, as in canonical form.
	*/
	pub(crate) fn from_value(value: Value, indentation: &str) -> Self {
		let inner = format!("{indentation}\t");
		// The closing bracket of a container with items goes on its own line.
		let closing = |count: usize| {
			if count == 0 {
				String::new()
			} else {
				indentation.to_owned()
			}
		};

		match value {
			Value::Object(object) => {
				let count = object.len();
				let members = object
					.into_iter()
					.enumerate()
					.map(|(index, (key, value))| {
						Member::laid_out(
							Key::new(key),
							Self::from_value(value, &inner),
							&inner,
							index == 0,
						)
					})
					.collect();

				Self::Object(Object {
					members,
					is_braced: true,
					closing: closing(count),
					span: None,
				})
			}
			Value::Array(items) => {
				let count = items.len();
				let items = items
					.into_iter()
					.enumerate()
					.map(|(index, value)| Item {
						value: Self::from_value(value, &inner),
						separator: Separator::laid_out(&inner, index == 0),
					})
					.collect();

				Self::Array(Array {
					items,
					closing: closing(count),
					span: None,
				})
			}
			value => Self::Scalar(Scalar {
				value,
				raw: None,
				span: None,
			}),
		}
	}

	/**
	A node for a value on one line, as in `{a: 1, b: [2, 3]}`, with a comma and a space between its members or items, for a value in a container that stays on one line.
	*/
	pub(crate) fn from_value_on_one_line(value: Value) -> Self {
		let mut node = Self::from_value(value, "");
		node.put_on_one_line();
		node
	}

	fn put_on_one_line(&mut self) {
		match self {
			Self::Object(object) => put_on_one_line(&mut object.members, &mut object.closing),
			Self::Array(array) => put_on_one_line(&mut array.items, &mut array.closing),
			Self::Scalar(_) => {}
		}
	}

	/**
	The object, if this node is one.
	*/
	#[must_use]
	pub const fn as_object(&self) -> Option<&Object> {
		match self {
			Self::Object(object) => Some(object),
			_ => None,
		}
	}

	/**
	The object, if this node is one, to change.
	*/
	pub const fn as_object_mut(&mut self) -> Option<&mut Object> {
		match self {
			Self::Object(object) => Some(object),
			_ => None,
		}
	}

	/**
	The array, if this node is one.
	*/
	#[must_use]
	pub const fn as_array(&self) -> Option<&Array> {
		match self {
			Self::Array(array) => Some(array),
			_ => None,
		}
	}

	/**
	The array, if this node is one, to change.
	*/
	pub const fn as_array_mut(&mut self) -> Option<&mut Array> {
		match self {
			Self::Array(array) => Some(array),
			_ => None,
		}
	}

	/**
	The scalar, if this node is one.
	*/
	#[must_use]
	pub const fn as_scalar(&self) -> Option<&Scalar> {
		match self {
			Self::Scalar(scalar) => Some(scalar),
			_ => None,
		}
	}

	/**
	The byte range of the node in the text the document was parsed from. `None` for a node that was added or replaced.
	*/
	#[must_use]
	pub fn span(&self) -> Option<Range<usize>> {
		match self {
			Self::Object(object) => object.span.clone(),
			Self::Array(array) => array.span.clone(),
			Self::Scalar(scalar) => scalar.span.clone(),
		}
	}

	/**
	Whether the node is printed on one line, with no line break anywhere in it, also not in a comment or a block string. A line comment in changed decor counts as a line break, because the printer ends its line.
	*/
	pub(crate) fn is_on_one_line(&self) -> bool {
		let mut printer = Printer::new(false);
		printer.node(self);
		!printer.output.contains('\n')
	}
}

impl Object {
	/**
	The members, in the order they are written.
	*/
	#[must_use]
	pub fn members(&self) -> &[Member] {
		&self.members
	}

	/**
	The members, to add, remove, reorder, or change. A change here can make a document that is not valid, such as one with two members with the same key; [`Document::to_value`](crate::Document::to_value) finds that. [`Document::set`](crate::Document::set) and [`Document::remove`](crate::Document::remove) keep the document valid and lay out what they add.
	*/
	pub const fn members_mut(&mut self) -> &mut Vec<Member> {
		&mut self.members
	}

	/**
	Whether the object is written with braces. Only a top-level object can be written without them.
	*/
	#[must_use]
	pub const fn is_braced(&self) -> bool {
		self.is_braced
	}

	/**
	The text after the last member: before the closing `}`, or at the end of a top-level object without braces.
	*/
	#[must_use]
	pub fn closing_decor(&self) -> &str {
		&self.closing
	}

	/**
	The text after the last member, to change.
	*/
	pub const fn closing_decor_mut(&mut self) -> &mut String {
		&mut self.closing
	}

	/**
	The position of the member with a key.
	*/
	pub(crate) fn position(&self, key: &str) -> Option<usize> {
		self.members
			.iter()
			.position(|member| member.key.value == key)
	}

	pub(crate) fn entries_mut(&mut self) -> Entries<'_, Member> {
		Entries {
			list: &mut self.members,
			closing: &mut self.closing,
			is_braced: self.is_braced,
		}
	}
}

impl Array {
	/**
	The items, in order.
	*/
	#[must_use]
	pub fn items(&self) -> &[Item] {
		&self.items
	}

	/**
	The items, to add, remove, reorder, or change.
	*/
	pub const fn items_mut(&mut self) -> &mut Vec<Item> {
		&mut self.items
	}

	/**
	The text after the last item, before the closing `]`.
	*/
	#[must_use]
	pub fn closing_decor(&self) -> &str {
		&self.closing
	}

	/**
	The text after the last item, to change.
	*/
	pub const fn closing_decor_mut(&mut self) -> &mut String {
		&mut self.closing
	}

	pub(crate) fn entries_mut(&mut self) -> Entries<'_, Item> {
		Entries {
			list: &mut self.items,
			closing: &mut self.closing,
			is_braced: true,
		}
	}
}

impl Member {
	/**
	Creates a member, with no decor.

	# Errors

	Returns an error when the value holds something SOML cannot represent: NaN, a carriage return, or nesting of 100 levels or more, because a member is always inside an object, or when the key holds a carriage return.
	*/
	pub fn new(key: Key, value: impl Into<Value>) -> Result<Self, crate::Error> {
		let value = checked(value.into())?;

		write::check_representable(key.value(), "key")?;

		Ok(Self {
			key,
			after_colon: " ".to_owned(),
			value: Node::from_value(value, ""),
			separator: Separator::default(),
			span: None,
		})
	}

	fn laid_out(key: Key, value: Node, indentation: &str, is_first: bool) -> Self {
		Self {
			key,
			after_colon: " ".to_owned(),
			value,
			separator: Separator::laid_out(indentation, is_first),
			span: None,
		}
	}

	/**
	The key.
	*/
	#[must_use]
	pub const fn key(&self) -> &Key {
		&self.key
	}

	/**
	The key, to change.
	*/
	pub const fn key_mut(&mut self) -> &mut Key {
		&mut self.key
	}

	/**
	The value.
	*/
	#[must_use]
	pub const fn value(&self) -> &Node {
		&self.value
	}

	/**
	The value, to change.
	*/
	pub const fn value_mut(&mut self) -> &mut Node {
		&mut self.value
	}

	/**
	The whitespace and comments around the member.
	*/
	#[must_use]
	pub const fn decor(&self) -> &Decor {
		&self.separator.decor
	}

	/**
	The whitespace and comments around the member, to change.
	*/
	pub const fn decor_mut(&mut self) -> &mut Decor {
		&mut self.separator.decor
	}

	/**
	The byte range of the member, from its key to the end of its value, in the text the document was parsed from. `None` for a member that was added.
	*/
	#[must_use]
	pub fn span(&self) -> Option<Range<usize>> {
		self.span.clone()
	}
}

impl Item {
	/**
	Creates an array item, with no decor.

	# Errors

	Returns an error when the value holds something SOML cannot represent: NaN, a carriage return, or nesting of 100 levels or more, because a node is always inside a collection.
	*/
	pub fn new(value: impl Into<Value>) -> Result<Self, crate::Error> {
		let value = checked(value.into())?;

		Ok(Self {
			value: Node::from_value(value, ""),
			separator: Separator::default(),
		})
	}

	/**
	The value.
	*/
	#[must_use]
	pub const fn value(&self) -> &Node {
		&self.value
	}

	/**
	The value, to change.
	*/
	pub const fn value_mut(&mut self) -> &mut Node {
		&mut self.value
	}

	/**
	The whitespace and comments around the item.
	*/
	#[must_use]
	pub const fn decor(&self) -> &Decor {
		&self.separator.decor
	}

	/**
	The whitespace and comments around the item, to change.
	*/
	pub const fn decor_mut(&mut self) -> &mut Decor {
		&mut self.separator.decor
	}
}

impl Separator {
	/**
	The decor of an item on its own line, as `Node::from_value` lays it out: the first item's line feed is in its leading decor, because it follows the opening bracket, and every item's trailing decor ends its line.
	*/
	fn laid_out(indentation: &str, is_first: bool) -> Self {
		Self {
			decor: Decor {
				leading: if is_first {
					format!("\n{indentation}")
				} else {
					indentation.to_owned()
				},
				trailing: "\n".to_owned(),
			},
			has_comma: false,
			before_comma: String::new(),
		}
	}
}

/**
Puts entries that `Node::from_value` laid out on one line, with a comma and a space between them.
*/
fn put_on_one_line<T: Entry>(entries: &mut [T], closing: &mut String) {
	let count = entries.len();

	for (index, entry) in entries.iter_mut().enumerate() {
		let separator = entry.separator_mut();
		separator.decor = Decor {
			leading: if index == 0 {
				String::new()
			} else {
				" ".to_owned()
			},
			trailing: String::new(),
		};
		separator.has_comma = index + 1 < count;
		entry.node_mut().put_on_one_line();
	}

	closing.clear();
}

/**
Checks that a value can be written, so that a tree never holds something that cannot be printed.
*/
pub(crate) fn checked(value: Value) -> Result<Value, crate::Error> {
	let node = crate::parse::Node::from(value);
	// As an item of a top-level array: one collection around it.
	write::write_value(&mut String::new(), &node, 1, 1, false)?;
	Ok(node.into_value())
}

impl Key {
	/**
	A key, which is written bare when it can be and quoted otherwise. A key that holds a `.` is quoted.

	```rust
	let key = soml::tree::Key::new("the name");
	assert_eq!(key.to_string(), "'the name'");

	let key = soml::tree::Key::new("example.com");
	assert_eq!(key.to_string(), "'example.com'");
	```
	*/
	#[must_use]
	pub fn new(value: impl Into<String>) -> Self {
		Self {
			value: value.into(),
			raw: None,
			span: None,
		}
	}

	/**
	The key, decoded, which is what makes two keys the same key.

	```rust
	let document: soml::Document = "'server': 1".parse()?;
	let key = document.root().as_object().unwrap().members()[0].key();

	assert_eq!(key.value(), "server");
	# Ok::<(), soml::Error>(())
	```
	*/
	#[must_use]
	pub fn value(&self) -> &str {
		&self.value
	}

	/**
	The text the author wrote for the key. `None` for a key that was added, which is written bare when it can be and quoted otherwise.
	*/
	#[must_use]
	pub fn raw(&self) -> Option<&str> {
		self.raw.as_deref()
	}

	/**
	How the key is written: bare, `'...'`, or `"..."`.
	*/
	#[must_use]
	pub fn style(&self) -> StringStyle {
		match &self.raw {
			Some(raw) => StringStyle::of(raw),
			None if scalar::is_bare_key(&self.value) => StringStyle::Bare,
			None if write::needs_escapes(&self.value) => StringStyle::Escaped,
			None => StringStyle::Literal,
		}
	}

	/**
	The byte range of the key in the text the document was parsed from. `None` for a key that was added.
	*/
	#[must_use]
	pub fn span(&self) -> Option<Range<usize>> {
		self.span.clone()
	}
}

impl std::fmt::Display for Key {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match &self.raw {
			Some(raw) => formatter.write_str(raw),
			None => write::write_key(formatter, &self.value),
		}
	}
}

impl Scalar {
	/**
	Writes the text the author wrote, or the canonical form of a value that was added or replaced.
	*/
	fn write_to(&self, output: &mut String) {
		match &self.raw {
			Some(raw) => output.push_str(raw),
			None => write::write_value(
				output,
				&crate::parse::Node::from(self.value.clone()),
				0,
				0,
				false,
			)
			.expect("a tree holds only scalars that can be written"),
		}
	}

	/**
	The value. Never an array or an object.
	*/
	#[must_use]
	pub const fn value(&self) -> &Value {
		&self.value
	}

	/**
	The text the author wrote for the value, such as `0xFF`, `'text'`, or `90m`. `None` for a value that was added or replaced, which is printed in canonical form.
	*/
	#[must_use]
	pub fn raw(&self) -> Option<&str> {
		self.raw.as_deref()
	}

	/**
	How a string is written. `None` for a value that is not a string.
	*/
	#[must_use]
	pub fn string_style(&self) -> Option<StringStyle> {
		let Value::String(text) = &self.value else {
			return None;
		};

		Some(match &self.raw {
			Some(raw) => StringStyle::of(raw),
			None if write::needs_escapes(text) => StringStyle::Escaped,
			None => StringStyle::Literal,
		})
	}

	/**
	Whether the value is a string written as a block string.
	*/
	pub(crate) fn is_block_string(&self) -> bool {
		matches!(
			self.string_style(),
			Some(StringStyle::LiteralBlock | StringStyle::EscapedBlock)
		)
	}

	/**
	The radix an int is written in. `None` for a value that is not an int.
	*/
	#[must_use]
	pub fn radix(&self) -> Option<Radix> {
		let Value::Int(_) = self.value else {
			return None;
		};

		Some(match self.raw.as_deref().map(str::as_bytes) {
			Some([b'0', b'x', ..]) => Radix::Hexadecimal,
			Some([b'0', b'o', ..]) => Radix::Octal,
			Some([b'0', b'b', ..]) => Radix::Binary,
			_ => Radix::Decimal,
		})
	}

	/**
	The byte range of the value in the text the document was parsed from. `None` for a value that was added or replaced.
	*/
	#[must_use]
	pub fn span(&self) -> Option<Range<usize>> {
		self.span.clone()
	}
}

/**
The parts of a parsed document: the decor before the top-level collection, the collection, and the decor after it.
*/
pub(crate) struct Parts {
	pub outer: Decor,
	pub root: Node,
}

/**
Builds the tree of a document that the parser has already accepted, so it trusts its input.
*/
pub(crate) fn build(source: &str) -> Parts {
	let builder = Builder {
		source,
		bytes: source.as_bytes(),
	};

	let start = builder.skip_trivia(0);

	if matches!(builder.bytes.get(start), Some(b'{' | b'[')) {
		let (root, end) = builder.value(start);

		return Parts {
			outer: Decor {
				leading: source[..start].to_owned(),
				trailing: source[end..].to_owned(),
			},
			root,
		};
	}

	Parts {
		outer: Decor::default(),
		root: Node::Object(builder.bare_object()),
	}
}

struct Builder<'a> {
	source: &'a str,
	bytes: &'a [u8],
}

impl Builder<'_> {
	/**
	The index after the spaces, tabs, line feeds, and comments at `index`.
	*/
	fn skip_trivia(&self, mut index: usize) -> usize {
		loop {
			match self.bytes.get(index) {
				Some(b' ' | b'\t' | b'\n') => index += 1,
				Some(b'#') => {
					index = scalar::line_end(self.bytes, index);
				}
				Some(b'/') if self.bytes.get(index + 1) == Some(&b'*') => {
					index = crate::parse::block_comment_end(self.bytes, index)
						.expect("the parser checked the comment")
						+ 2
				}
				_ => return index,
			}
		}
	}

	/**
	Splits the decor between `start` and `end` after its first line feed outside a block comment. Returns the index of the split, which is `start` when there is no line feed.
	*/
	fn split_after_line_feed(&self, start: usize, end: usize) -> usize {
		decor_parts(&self.source[start..end])
			.find(|(part, _)| *part == DecorPart::LineFeed)
			.map_or(start, |(_, range)| start + range.end)
	}

	fn bare_object(&self) -> Object {
		let mut members = Vec::new();
		let mut leading_start = 0;

		loop {
			let key_start = self.skip_trivia(leading_start);
			let (mut member, value_end) = self.member(key_start);
			member.separator.decor.leading = self.source[leading_start..key_start].to_owned();
			let next = self.skip_trivia(value_end);
			let mut split = self.split_after_line_feed(value_end, next);

			// The last line of a document without a final line feed ends at the end of the text, so a comment there is the last member's.
			if next >= self.bytes.len() && split == value_end {
				split = self.bytes.len();
			}

			member.separator.decor.trailing = self.source[value_end..split].to_owned();
			members.push(member);

			if next >= self.bytes.len() {
				return Object {
					// From the first member to the end of the last, as in the JS reference. The text around them is decor.
					span: Some(
						members[0]
							.span
							.clone()
							.expect("a parsed member has a span")
							.start..value_end,
					),
					members,
					is_braced: false,
					closing: self.source[split..].to_owned(),
				};
			}

			leading_start = split;
		}
	}

	/**
	A member, without its decor, and the index after its value.
	*/
	fn member(&self, start: usize) -> (Member, usize) {
		let (key, index) = self.key(start);

		// Past the `:`.
		let value_start = self.skip_trivia(index + 1);
		let (value, value_end) = self.value(value_start);

		let member = Member {
			key,
			after_colon: self.source[index + 1..value_start].to_owned(),
			value,
			separator: Separator::default(),
			span: Some(start..value_end),
		};

		(member, value_end)
	}

	/**
	A key, and the index after it.
	*/
	fn key(&self, start: usize) -> (Key, usize) {
		let (value, end) = match self.bytes[start] {
			b'\'' | b'"' => {
				let (value, end) =
					scalar::string(self.source, start).expect("the parser checked the key");
				(value.into_owned(), end)
			}
			_ => {
				let end = start
					+ self.bytes[start..]
						.iter()
						.take_while(|&&byte| is_bare_key_byte(byte))
						.count();
				(self.source[start..end].to_owned(), end)
			}
		};

		let key = Key {
			value,
			raw: Some(self.source[start..end].to_owned()),
			span: Some(start..end),
		};

		(key, end)
	}

	/**
	A value, and the index after it.
	*/
	fn value(&self, start: usize) -> (Node, usize) {
		match self.bytes[start] {
			b'{' => self.braced_object(start),
			b'[' => self.array(start),
			b'\'' | b'"' => {
				let (value, end) =
					scalar::string(self.source, start).expect("the parser checked the string");
				(
					self.scalar(Value::String(value.into_owned()), start, end),
					end,
				)
			}
			_ => {
				let end = scalar::number_end(self.bytes, start);
				let text = &self.source[start..end];

				let value = match text {
					"true" => Value::Bool(true),
					"false" => Value::Bool(false),
					"null" => Value::Null,
					"infinity" => Value::Float(f64::INFINITY),
					"-infinity" => Value::Float(f64::NEG_INFINITY),
					_ => match scalar::token_value(text)
						.and_then(Result::ok)
						.expect("the parser checked the value")
					{
						Token::Int(value) => Value::Int(value),
						Token::Float(value) => Value::Float(value),
						Token::Instant(value) => Value::Instant(value),
						Token::Duration(value) => Value::Duration(value),
					},
				};

				(self.scalar(value, start, end), end)
			}
		}
	}

	fn scalar(&self, value: Value, start: usize, end: usize) -> Node {
		Node::Scalar(Scalar {
			value,
			raw: Some(self.source[start..end].to_owned()),
			span: Some(start..end),
		})
	}

	fn braced_object(&self, start: usize) -> (Node, usize) {
		let mut members = Vec::new();

		let (separators, closing, end) = self.items(start, b'}', |index| {
			let (member, end) = self.member(index);
			members.push(member);
			end
		});

		for (member, separator) in members.iter_mut().zip(separators) {
			member.separator = separator;
		}

		let object = Object {
			members,
			is_braced: true,
			closing,
			span: Some(start..end),
		};

		(Node::Object(object), end)
	}

	fn array(&self, start: usize) -> (Node, usize) {
		let mut values = Vec::new();

		let (separators, closing, end) = self.items(start, b']', |index| {
			let (value, end) = self.value(index);
			values.push(value);
			end
		});

		let items = values
			.into_iter()
			.zip(separators)
			.map(|(value, separator)| Item { value, separator })
			.collect();

		let array = Array {
			items,
			closing,
			span: Some(start..end),
		};

		(Node::Array(array), end)
	}

	/**
	Reads the items of a braced container that opens at `start`, with `item` reading one item and returning the index after it. Returns each item's separator, the closing decor, and the index after the closing bracket.
	*/
	fn items(
		&self,
		start: usize,
		closing_bracket: u8,
		mut item: impl FnMut(usize) -> usize,
	) -> (Vec<Separator>, String, usize) {
		let mut separators = Vec::new();
		let mut leading_start = start + 1;

		loop {
			let item_start = self.skip_trivia(leading_start);

			if self.bytes[item_start] == closing_bracket {
				return (
					separators,
					self.source[leading_start..item_start].to_owned(),
					item_start + 1,
				);
			}

			let value_end = item(item_start);
			separators.push(Separator {
				decor: Decor {
					leading: self.source[leading_start..item_start].to_owned(),
					trailing: String::new(),
				},
				..Separator::default()
			});

			let separator = separators.last_mut().expect("a separator was pushed");
			let after_value = self.skip_trivia(value_end);
			let mut trailing_start = value_end;

			if self.bytes[after_value] == b',' {
				separator.has_comma = true;
				separator.before_comma = self.source[value_end..after_value].to_owned();
				trailing_start = after_value + 1;
			}

			let next = self.skip_trivia(trailing_start);
			let split = self.split_after_line_feed(trailing_start, next);
			separator.decor.trailing = self.source[trailing_start..split].to_owned();
			leading_start = split;
		}
	}
}

/**
Writes a tree, and finds the comments in it on the way.
*/
pub(crate) struct Printer {
	pub output: String,
	pub comments: Option<Vec<Comment>>,
	/**
	Whether changed decor ended inside a line comment, so a line feed must come before anything else is printed. It is dropped at the end of the document, so a document that ends in a comment prints back unchanged.
	*/
	pub needs_line_feed: bool,
}

impl Printer {
	/**
	A printer with no output, which also collects the comments when `collects_comments` is true.
	*/
	pub(crate) fn new(collects_comments: bool) -> Self {
		Self {
			output: String::new(),
			comments: collects_comments.then(Vec::new),
			needs_line_feed: false,
		}
	}

	/**
	Writes text that is not decor.
	*/
	fn write(&mut self, text: &str) {
		self.end_line_comment();
		self.output.push_str(text);
	}

	fn end_line_comment(&mut self) {
		if self.needs_line_feed {
			self.output.push('\n');
			self.needs_line_feed = false;
		}
	}

	pub(crate) fn document(&mut self, outer: &Decor, root: &Node) {
		self.decor(&outer.leading);
		self.node(root);
		self.decor(&outer.trailing);
	}

	fn node(&mut self, node: &Node) {
		match node {
			Node::Object(object) => self.object(object),
			Node::Array(array) => self.array(array),
			Node::Scalar(scalar) => self.scalar(scalar),
		}
	}

	fn object(&mut self, object: &Object) {
		if !object.is_braced {
			// A top-level object without braces needs at least one member.
			if object.members.is_empty() {
				self.write("{}");
			}

			for (index, member) in object.members.iter().enumerate() {
				let decor = &member.separator.decor;

				// Members are separated by a line break, which an added member may not have in its decor.
				if index > 0
					&& !has_line_feed(&object.members[index - 1].separator.decor.trailing)
					&& !has_line_feed(&decor.leading)
				{
					// The next write ends the line once, also after a line comment, which needs a line feed of its own.
					self.needs_line_feed = true;
				}

				self.decor(&decor.leading);
				self.member(member);
				self.decor(&decor.trailing);
			}

			self.decor(&object.closing);
			return;
		}

		self.write("{");
		self.entries(&object.members);
		self.decor(&object.closing);
		self.write("}");
	}

	fn member(&mut self, member: &Member) {
		self.end_line_comment();
		write!(self.output, "{}", member.key).expect("writing to a String does not fail");
		self.output.push(':');
		self.decor(&member.after_colon);
		self.node(&member.value);
	}

	fn array(&mut self, array: &Array) {
		self.write("[");
		self.entries(&array.items);
		self.decor(&array.closing);
		self.write("]");
	}

	/**
	The members or items of a braced container, each with its comma when it has one. An item that is on the same line as the next one gets a comma too, because a line break or a comma must separate them, so a change through the node API cannot join two items.
	*/
	fn entries<T: Entry>(&mut self, entries: &[T]) {
		for (index, entry) in entries.iter().enumerate() {
			let separator = entry.separator();
			self.decor(&separator.decor.leading);
			entry.print(self);
			// Printed also when the comma was dropped, so a comment before it is not lost.
			self.decor(&separator.before_comma);

			if separator.has_comma || shares_line_with_next(entries, index) {
				self.write(",");
			}

			self.decor(&separator.decor.trailing);
		}
	}

	fn scalar(&mut self, scalar: &Scalar) {
		self.end_line_comment();
		scalar.write_to(&mut self.output);
	}

	/**
	Writes decor, and records the comments in it. Decor holds only whitespace and comments, so a `#` or the opening of a block comment in it always starts a comment.
	*/
	fn decor(&mut self, decor: &str) {
		if decor.starts_with('\n') {
			self.needs_line_feed = false;
		} else if !decor.is_empty() {
			self.end_line_comment();
		}

		let start = self.output.len();
		self.output.push_str(decor);

		for (part, range) in decor_parts(decor) {
			// A line comment at the end of changed decor would otherwise swallow whatever is printed next.
			self.needs_line_feed = part == DecorPart::LineComment && range.end == decor.len();

			if let Some(comments) = &mut self.comments
				&& part != DecorPart::LineFeed
			{
				let delimited = &decor[range.clone()];
				// An unterminated block comment, which only changed decor can have, runs to the end.
				let (kind, text) = match delimited.strip_prefix("/*") {
					Some(body) => (CommentKind::Block, body.strip_suffix("*/").unwrap_or(body)),
					None => (CommentKind::Line, &delimited[1..]),
				};

				comments.push(Comment {
					text: text.to_owned(),
					kind,
					span: start + range.start..start + range.end,
				});
			}
		}
	}
}

/**
What a part of decor is.
*/
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecorPart {
	/**
	A line feed outside a comment.
	*/
	LineFeed,
	LineComment,
	BlockComment,
}

/**
The line feeds and comments in decor, with their ranges. Decor holds only whitespace and comments, so a `#` or the opening of a block comment in it always starts a comment. A block comment without its end, which only changed decor can have, runs to the end.
*/
pub(crate) fn decor_parts(decor: &str) -> impl Iterator<Item = (DecorPart, Range<usize>)> + '_ {
	let bytes = decor.as_bytes();
	let mut index = 0;

	std::iter::from_fn(move || {
		while index < bytes.len() {
			let start = index;

			let part = match bytes[index] {
				b'\n' => {
					index += 1;
					DecorPart::LineFeed
				}
				b'#' => {
					index = scalar::line_end(bytes, index);
					DecorPart::LineComment
				}
				b'/' if bytes.get(index + 1) == Some(&b'*') => {
					index = crate::parse::block_comment_end(bytes, index)
						.map_or(bytes.len(), |end| end + 2);
					DecorPart::BlockComment
				}
				_ => {
					index += 1;
					continue;
				}
			};

			return Some((part, start..index));
		}

		None
	})
}

/**
The spaces and tabs at the start of a line.
*/
pub(crate) fn indentation_of(line: &str) -> &str {
	&line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/**
Whether the entry at `index` is on the same line as the next one, so a comma must separate them.
*/
pub(crate) fn shares_line_with_next<T: Entry>(entries: &[T], index: usize) -> bool {
	let separator = entries[index].separator();

	entries.get(index + 1).is_some_and(|next| {
		!has_line_feed(&separator.decor.trailing) && !has_line_feed(&next.separator().decor.leading)
	})
}

/**
Whether decor ends in a line comment, without the line feed that ends it, as changed decor and the last line of a document can.
*/
pub(crate) fn ends_in_line_comment(decor: &str) -> bool {
	decor_parts(decor)
		.last()
		.is_some_and(|(part, range)| part == DecorPart::LineComment && range.end == decor.len())
}

/**
Whether decor has a line feed outside its block comments.
*/
pub(crate) fn has_line_feed(decor: &str) -> bool {
	decor_parts(decor).any(|(part, _)| part == DecorPart::LineFeed)
}
