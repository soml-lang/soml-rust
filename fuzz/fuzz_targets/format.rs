#![no_main]

use libfuzzer_sys::fuzz_target;

fn comment_texts(document: &soml::Document) -> Vec<String> {
	// The formatter removes trailing whitespace and blank lines inside a block comment.
	let mut texts: Vec<String> = document
		.comments()
		.into_iter()
		.map(|comment| {
			comment
				.text
				.split('\n')
				.map(|line| line.trim_end_matches([' ', '\t']))
				.filter(|line| !line.is_empty())
				.collect::<Vec<_>>()
				.join("\n")
		})
		.collect();
	texts.sort();
	texts
}

/*
Formatting a valid document gives a valid document with the same value and the same comments, and formatting that gives the same text.
*/
fuzz_target!(|text: &str| {
	let Ok(document) = text.parse::<soml::Document>() else {
		return;
	};

	let formatted = document.format();
	let reread: soml::Document = formatted.parse().unwrap_or_else(|error| panic!("{error}\n{formatted}"));
	assert_eq!(reread.to_value().expect("valid"), document.to_value().expect("valid"), "{formatted}");
	assert_eq!(comment_texts(&reread), comment_texts(&document), "{formatted}");
	assert_eq!(reread.format(), formatted);
});
