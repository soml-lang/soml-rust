#![no_main]

use libfuzzer_sys::fuzz_target;

/*
Any input is either rejected, or it reads as a value whose canonical form reads back as the same value and is a fixed point.
*/
fuzz_target!(|bytes: &[u8]| {
	let Ok(value) = soml::from_slice::<soml::Value>(bytes) else {
		return;
	};

	let canonical = soml::to_string_canonical(&value).expect("a value that was read can be written");
	let reread: soml::Value = canonical.parse().expect("canonical form can be read");
	assert_eq!(reread, value);
	assert_eq!(soml::to_string_canonical(&reread).expect("written again"), canonical);
});
