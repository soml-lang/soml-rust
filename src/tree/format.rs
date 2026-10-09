/*!
The formatter in the spec's "Formatting" section. Its rules and the decisions the spec leaves open are in the docs of the `tree` module.
*/

use std::borrow::Cow;

use super::{
	Decor, DecorPart, Entry, Member, Node, Object, Scalar, decor_parts, ends_in_line_comment,
	has_line_feed, indentation_of, shares_line_with_next,
};

/**
The comments and blank lines in the decor between two tokens, by where they go.
*/
#[derive(Default)]
struct Gap<'a> {
	/**
	Comments on the line of the token before the gap.
	*/
	end_of_line: Vec<Cow<'a, str>>,
	/**
	Comment lines, each with its comments, and blank lines as `None`, a run of them as one.
	*/
	lines: Vec<Option<Vec<Cow<'a, str>>>>,
	/**
	Comments on the line of the token after the gap, before it. Only these comments are carried to the following token; all comments can borrow temporary joined decor while they are written.
	*/
	inline: Vec<Cow<'a, str>>,
}

impl<'a> Gap<'a> {
	/**
	Reads the gap from decor after a token.
	*/
	fn read(decor: &'a str) -> Self {
		Self::scan(decor, false, false)
	}

	/**
	Reads the gap from decor at the start of a document, where nothing is on the line of a token before it.
	*/
	fn read_document_start(decor: &'a str) -> Self {
		Self::scan(decor, true, false)
	}

	fn scan(decor: &'a str, starts_line: bool, is_end: bool) -> Self {
		let mut gap = Self::default();
		let mut current = Vec::new();
		let mut has_line_break = starts_line;

		for (part, range) in decor_parts(decor) {
			match part {
				DecorPart::LineComment => {
					// A line comment runs up to the line feed, so it includes the spaces and tabs at the end of its line. These are removed.
					current.push(decor[range].trim_end_matches([' ', '\t']).into())
				}
				DecorPart::BlockComment => {
					// A block comment that spans lines is part of the line it ends on, so the comments before it on the line of the token before stay on that line, as in the JavaScript reference.
					if !has_line_break && decor[range.clone()].contains('\n') {
						gap.end_of_line.append(&mut current);
					}

					current.push(block_comment(&decor[range]));
				}
				DecorPart::LineFeed if !has_line_break => {
					gap.end_of_line.append(&mut current);
					has_line_break = true;
				}
				DecorPart::LineFeed if current.is_empty() => {
					if gap.lines.last().is_none_or(Option::is_some) {
						gap.lines.push(None);
					}
				}
				DecorPart::LineFeed => gap.lines.push(Some(std::mem::take(&mut current))),
			}
		}

		if is_end && !has_line_break {
			gap.end_of_line.append(&mut current);
		}
		gap.inline = current;
		gap
	}

	/**
	Reads the gap before a closing bracket or the end of the document. When it has no line break, its comments are on the line of the token before it, because nothing else follows on that line.
	*/
	fn read_end(decor: &'a str) -> Self {
		Self::scan(decor, false, true)
	}

	/**
	The comment lines and blank lines after an opening bracket or the start of the document, without a blank line at the start, which would touch it. A run of blank lines is already one.
	*/
	fn lines_after_opening(&self) -> &[Option<Vec<Cow<'a, str>>>] {
		self.lines.strip_prefix(&[None]).unwrap_or(&self.lines)
	}

	/**
	Whether the gap has a comment. A comment line, which is `Some`, always holds at least one.
	*/
	fn has_comments(&self) -> bool {
		!self.end_of_line.is_empty()
			|| self.lines.iter().any(Option::is_some)
			|| !self.inline.is_empty()
	}

	/**
	Every comment in the gap, in order.
	*/
	fn all(&self) -> impl Iterator<Item = &str> {
		self.end_of_line
			.iter()
			.chain(self.lines.iter().flatten().flatten())
			.chain(&self.inline)
			.map(AsRef::as_ref)
	}

	/**
	The comment lines before a closing bracket or the end of the document, with the comments before it as one more line, and without a blank line at the end, which would touch it.
	*/
	fn lines_before_closing(&self) -> impl Iterator<Item = Option<&[Cow<'a, str>]>> {
		let lines = if self.inline.is_empty() {
			self.lines.strip_suffix(&[None]).unwrap_or(&self.lines)
		} else {
			&self.lines
		};
		lines
			.iter()
			.map(Option::as_deref)
			.chain((!self.inline.is_empty()).then_some(Some(self.inline.as_slice())))
	}
}

/**
Neighboring decor, borrowed when no join is needed.
*/
fn joined_decor<'a>(before: &'a str, line_break: &str, after: &'a str) -> Cow<'a, str> {
	if before.is_empty() && line_break.is_empty() {
		return Cow::Borrowed(after);
	}
	if after.is_empty() && line_break.is_empty() {
		return Cow::Borrowed(before);
	}
	Cow::Owned([before, line_break, after].concat())
}

/**
A block comment with the layout rule for every line outside a block string: trailing spaces and tabs go, and runs of blank lines collapse to one.
*/
fn block_comment(text: &str) -> Cow<'_, str> {
	// Note: Not using dedup_by because collecting comment lines adds allocations even when the text is unchanged.
	let mut previous_is_blank = false;
	let mut start = 0;
	let mut lines = text.split('\n');
	let changed = loop {
		let Some(line) = lines.next() else {
			return Cow::Borrowed(text);
		};
		let trimmed = line.trim_end_matches([' ', '\t']);
		let is_blank = trimmed.is_empty();
		if trimmed.len() != line.len() || is_blank && previous_is_blank {
			break line;
		}
		previous_is_blank = is_blank;
		start += line.len() + 1;
	};
	let mut output = String::with_capacity(text.len());
	let prefix = &text[..start];
	output.push_str(prefix.strip_suffix('\n').unwrap_or(prefix));
	let mut is_first = start == 0;
	for line in std::iter::once(changed).chain(lines) {
		let line = line.trim_end_matches([' ', '\t']);
		if line.is_empty() && previous_is_blank {
			continue;
		}
		if !is_first {
			output.push('\n');
		}
		output.push_str(line);
		previous_is_blank = line.is_empty();
		is_first = false;
	}
	Cow::Owned(output)
}

/**
Writes the formatted text a line at a time.
*/
#[derive(Default)]
struct Formatter {
	output: String,
	/**
	The current line, without its indentation.
	*/
	line: String,
	/**
	The indentation of the current line, in tabs.
	*/
	indentation: usize,
	/**
	Whether the current line ends in a line comment, so nothing more can go on it.
	*/
	is_line_closed: bool,
}

impl Formatter {
	fn start_line(&mut self, indentation: usize) {
		self.indentation = indentation;
	}

	/**
	Writes text on the current line, or on a new line at `indentation` when the current one ends in a line comment.
	*/
	fn push(&mut self, text: &str, indentation: usize) {
		if self.is_line_closed {
			self.end_line();
			self.start_line(indentation);
		}

		self.line.push_str(text);
	}

	fn end_line(&mut self) {
		// An empty line gets no indentation, so a blank line has no trailing tabs.
		if !self.line.is_empty() {
			self.output
				.extend(std::iter::repeat_n('\t', self.indentation));
			self.output.push_str(&self.line);
			self.line.clear();
		}

		self.output.push('\n');
		self.is_line_closed = false;
	}

	/**
	Writes comments on the current line, each after a space. A line comment closes the line, so anything after it goes on a new line at `indentation`.
	*/
	fn comments<'a>(&mut self, comments: impl IntoIterator<Item = &'a str>, indentation: usize) {
		for comment in comments {
			if self.is_line_closed {
				self.end_line();
				self.start_line(indentation);
			} else if !self.line.is_empty() {
				self.line.push(' ');
			}

			self.line.push_str(comment);
			self.is_line_closed = comment.starts_with('#');
		}
	}

	/**
	Writes comments that come before a token on its line, with a space between them and the token.
	*/
	fn comments_before(&mut self, comments: &[Cow<'_, str>], indentation: usize) {
		if !comments.is_empty() {
			self.comments(comments.iter().map(AsRef::as_ref), indentation);

			// A line comment, which only changed decor can have here, already moved what follows to a new line.
			if !self.is_line_closed {
				self.line.push(' ');
			}
		}
	}

	/**
	Writes comment lines and blank lines at `indentation`, each on a line of its own. The current line must be empty.
	*/
	fn lines<'a>(
		&mut self,
		lines: impl IntoIterator<Item = Option<&'a [Cow<'a, str>]>>,
		indentation: usize,
	) {
		for line in lines {
			if let Some(comments) = line {
				self.start_line(indentation);
				self.comments(comments.iter().map(AsRef::as_ref), indentation);
			}

			self.end_line();
		}
	}

	/**
	Writes a value that starts on the current line. `indentation` is that of the line the value starts on.
	*/
	fn node(&mut self, node: &Node, indentation: usize, is_one_line: bool) {
		match node {
			Node::Object(object) if is_one_line || node.is_on_one_line() => self.one_line(
				'{',
				'}',
				&object.members,
				&object.closing,
				indentation,
				Self::member,
			),
			Node::Array(array) if is_one_line || node.is_on_one_line() => self.one_line(
				'[',
				']',
				&array.items,
				&array.closing,
				indentation,
				|formatter, item, indentation, is_one_line| {
					formatter.node(&item.value, indentation, is_one_line)
				},
			),
			Node::Object(object) => self.container(
				'{',
				'}',
				&object.members,
				&object.closing,
				indentation,
				Self::member,
			),
			Node::Array(array) => self.container(
				'[',
				']',
				&array.items,
				&array.closing,
				indentation,
				|formatter, item, indentation, is_one_line| {
					formatter.node(&item.value, indentation, is_one_line)
				},
			),
			Node::Scalar(scalar) => self.scalar(scalar, indentation),
		}
	}

	fn member(&mut self, member: &Member, indentation: usize, is_one_line: bool) {
		self.push("", indentation);
		member.key.write(&mut self.line);
		self.line.push(':');

		// A value on the line after its `key:` moves up to that line, unless a comment comes between them. Then each comment keeps its line, and the value goes on a line of its own, one level deeper. Blank lines between them go.
		let gap = Gap::read(&member.after_colon);
		let is_block_string =
			matches!(&member.value, Node::Scalar(scalar) if scalar.is_block_string());

		if has_line_feed(&member.after_colon) && gap.has_comments() {
			self.comments(gap.end_of_line.iter().map(AsRef::as_ref), indentation + 1);
			self.end_line();
			self.lines(
				gap.lines.iter().filter_map(Option::as_deref).map(Some),
				indentation + 1,
			);
			self.start_line(indentation + 1);

			if is_block_string {
				// A block string begins on a line of its own, after the comments on its line.
				if !gap.inline.is_empty() {
					self.comments(gap.inline.iter().map(AsRef::as_ref), indentation + 1);
					self.end_line();
					self.start_line(indentation + 1);
				}
			} else {
				self.comments_before(&gap.inline, indentation + 1);
			}
		} else {
			// Without a line feed, every comment is on the key's line, also those before a block comment that spans lines.
			self.comments(gap.all(), indentation + 1);

			if is_block_string {
				// A block string begins on the line after its key, one level deeper, so its delimiters and content line up.
				self.end_line();
				self.start_line(indentation + 1);
			} else {
				self.line.push(' ');
			}
		}

		// The value starts on the current line, which is one level deeper than the key's line when the value moved to a new line.
		self.node(&member.value, self.indentation, is_one_line);
	}

	fn scalar(&mut self, scalar: &Scalar, indentation: usize) {
		let Some(text) = scalar.raw.as_deref() else {
			self.push("", indentation);
			scalar.write(&mut self.line);
			return;
		};

		// Only a block string has a line feed in its text: a parsed `'...'` or `"..."` string ends on the line it starts on, and an added or replaced string is written with escapes.
		let Some((opening, rest)) = text.split_once('\n') else {
			self.push(text, indentation);
			return;
		};

		// A block string, whose delimiters and content have the indentation of the line it starts on. Its closing delimiter's indentation is removed from every content line, so swapping it for another one on every line keeps the value.
		self.push(opening, indentation);
		self.end_line();

		let (content, closing) = match rest.rsplit_once('\n') {
			Some((content, closing)) => (Some(content), closing),
			None => (None, rest),
		};

		let old_indentation = indentation_of(closing);

		// Note: Not using split_terminator because a final empty content line must stay in a block string.
		for line in content.into_iter().flat_map(|content| content.split('\n')) {
			// A blank line stays as written, because it is an empty line in the value either way, and the spec's layout rules do not apply inside a block string.
			if line.bytes().all(|byte| byte == b' ' || byte == b'\t') {
				self.output.push_str(line);
			} else {
				self.output.extend(std::iter::repeat_n('\t', indentation));
				self.output
					.push_str(line.strip_prefix(old_indentation).expect(
						"every content line of a block string starts with the indentation of its closing delimiter",
					));
			}

			self.output.push('\n');
		}

		self.start_line(indentation);
		self.line.push_str(&closing[old_indentation.len()..]);
	}

	/**
	Writes a braced container that starts on the current line: its entries on lines of their own one level deeper, and the closing bracket on a line of its own at `indentation`.
	*/
	fn container<T: Entry>(
		&mut self,
		opening: char,
		closing: char,
		entries: &[T],
		closing_decor: &str,
		indentation: usize,
		mut entry: impl FnMut(&mut Self, &T, usize, bool),
	) {
		let inner = indentation + 1;
		self.push(opening.encode_utf8(&mut [0; 4]), indentation);

		let Some(first) = entries.first() else {
			let gap = Gap::read_end(closing_decor);
			// In an empty container, a blank line at the start would touch the opening bracket too.
			let mut lines = gap.lines_before_closing().peekable();
			if lines.peek() == Some(&None) {
				lines.next();
			}

			if gap.end_of_line.is_empty() && lines.peek().is_none() {
				self.line.push(closing);
				return;
			}

			self.comments(gap.end_of_line.iter().map(AsRef::as_ref), inner);
			self.end_line();
			self.lines(lines, inner);
			self.start_line(indentation);
			self.line.push(closing);
			return;
		};

		// The comments after the opening bracket.
		let gap = Gap::read(&first.separator().decor.leading);
		self.comments(gap.end_of_line.iter().map(AsRef::as_ref), inner);
		self.end_line();
		self.lines(
			gap.lines_after_opening().iter().map(Option::as_deref),
			inner,
		);
		self.start_line(inner);
		self.comments_before(&gap.inline, inner);

		for (index, current) in entries.iter().enumerate() {
			let separator = current.separator();
			self.start_line(inner);
			entry(self, current, inner, false);

			// The comma goes, because a line break separates the entries. A comment between the value and the comma, which is on the value's line, stays after the value.
			self.comments(Gap::read(&separator.before_comma).all(), inner);
			let next = entries.get(index + 1);
			let trailing = &separator.decor.trailing;
			let after = next.map_or(closing_decor, |next| &next.separator().decor.leading);
			// The line feed that the printer adds after a line comment at the end of changed decor, so the comment does not take in what follows.
			let line_break = if ends_in_line_comment(trailing) && !after.starts_with('\n') {
				"\n"
			} else {
				""
			};
			let decor = joined_decor(trailing, line_break, after);
			self.gap_after(
				Gap::scan(&decor, false, next.is_none()),
				next.is_none(),
				inner,
			);
		}

		self.start_line(indentation);
		self.line.push(closing);
	}

	/**
	Writes a container whose brackets are on one line on one line, with a comma and a space between its entries and no comma after the last one. Only block comments can be in it, and each one stays where it is among the entries and commas, so `[1 /* a */, /* b */ 2, /* c */]` becomes `[1 /* a */, /* b */ 2 /* c */]`.
	*/
	fn one_line<T: Entry>(
		&mut self,
		opening: char,
		closing: char,
		entries: &[T],
		closing_decor: &str,
		indentation: usize,
		mut entry: impl FnMut(&mut Self, &T, usize, bool),
	) {
		self.push(opening.encode_utf8(&mut [0; 4]), indentation);
		// Whether something follows the opening bracket, so a space goes before what comes next.
		let mut has_written = false;

		for (index, current) in entries.iter().enumerate() {
			let separator = current.separator();
			self.inline_comments(&separator.decor.leading, &mut has_written);

			if has_written {
				self.line.push(' ');
			}

			entry(self, current, indentation, true);
			has_written = true;
			self.inline_comments(&separator.before_comma, &mut has_written);

			// The comma after the last entry goes. Every other entry gets one, as the printer writes it for entries that share a line.
			if index + 1 < entries.len() {
				self.line.push(',');
			}

			self.inline_comments(&separator.decor.trailing, &mut has_written);
		}

		self.inline_comments(closing_decor, &mut has_written);
		self.line.push(closing);
	}

	/**
	Writes the comments in decor on one line, each after a space when something comes before it. Decor on one line holds only spaces, tabs, and block comments.
	*/
	fn inline_comments(&mut self, decor: &str, has_written: &mut bool) {
		for (_, range) in decor_parts(decor) {
			if *has_written {
				self.line.push(' ');
			}

			self.line.push_str(&decor[range]);
			*has_written = true;
		}
	}

	/**
	Writes a top-level object without braces: its members at column 0, with no commas.
	*/
	fn bare_object(&mut self, object: &Object) {
		let Some(first) = object.members.first() else {
			// A document cannot be empty, so an object without members has braces.
			self.start_line(0);
			self.line.push_str("{}");
			self.gap_after(Gap::read_end(&object.closing), true, 0);
			return;
		};

		let gap = Gap::read_document_start(&first.separator.decor.leading);
		self.lines(gap.lines_after_opening().iter().map(Option::as_deref), 0);
		self.start_line(0);
		self.comments_before(&gap.inline, 0);

		for (index, member) in object.members.iter().enumerate() {
			self.start_line(0);
			self.member(member, 0, false);

			let next = object.members.get(index + 1);
			let after = next.map_or(object.closing.as_str(), |next| {
				next.separator.decor.leading.as_str()
			});
			let trailing = &member.separator.decor.trailing;
			// The line break that the printer adds in changed decor: after a line comment at its end, before whatever follows, and between members whose decor has none, as after a member added through the node API.
			let ends_line_comment =
				ends_in_line_comment(trailing) && (next.is_some() || !after.is_empty());
			let separates_members = shares_line_with_next(&object.members, index);
			let line_break = if !after.starts_with('\n') && (ends_line_comment || separates_members)
			{
				"\n"
			} else {
				""
			};
			let decor = joined_decor(trailing, line_break, after);
			self.gap_after(Gap::scan(&decor, false, next.is_none()), next.is_none(), 0);
		}
	}

	/**
	Writes the gap after an entry: the comments at the end of its line, and then the lines up to the next entry, or up to the end of its container or document when `is_last`. Writes the inline comments before the next entry while they still borrow the gap.
	*/
	fn gap_after(&mut self, gap: Gap<'_>, is_last: bool, indentation: usize) {
		self.comments(gap.end_of_line.iter().map(AsRef::as_ref), indentation);
		self.end_line();

		if is_last {
			self.lines(gap.lines_before_closing(), indentation);
		} else {
			self.lines(gap.lines.iter().map(Option::as_deref), indentation);
			self.start_line(indentation);
			self.comments_before(&gap.inline, indentation);
		}
	}
}

/**
Formats a document from its parts.
*/
pub(crate) fn document(outer: &Decor, root: &Node, capacity: usize) -> String {
	let mut formatter = Formatter {
		output: String::with_capacity(capacity),
		line: String::with_capacity(128),
		..Formatter::default()
	};

	if let Node::Object(object) = root
		&& !object.is_braced
	{
		formatter.bare_object(object);
	} else {
		let before = Gap::read_document_start(&outer.leading);
		formatter.lines(before.lines_after_opening().iter().map(Option::as_deref), 0);
		formatter.start_line(0);
		formatter.comments_before(&before.inline, 0);
		formatter.node(root, 0, false);

		formatter.gap_after(Gap::read_end(&outer.trailing), true, 0);
	}

	formatter.output
}
