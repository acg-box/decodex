use sha2::{Digest as _, Sha256};

use crate::{prelude::Result, research_design::ResearchDesignRunInput};

pub(in crate::research_design) fn generated_contract_id(
	input: &ResearchDesignRunInput,
) -> Result<String> {
	let slug = intent_slug(&input.intent);
	let encoded = serde_json::to_vec(input)?;
	let digest = Sha256::digest(&encoded);
	let mut hash = String::with_capacity(12);

	for byte in digest.iter().take(6) {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	Ok(format!("research-design-{slug}-{hash}"))
}

fn intent_slug(intent: &str) -> String {
	let mut slug = String::new();
	let mut previous_dash = false;

	for character in intent.chars() {
		if character.is_ascii_alphanumeric() {
			slug.push(character.to_ascii_lowercase());

			previous_dash = false;
		} else if !previous_dash && !slug.is_empty() {
			slug.push('-');

			previous_dash = true;
		}
		if slug.len() >= 40 {
			break;
		}
	}

	while slug.ends_with('-') {
		slug.pop();
	}

	if slug.is_empty() { String::from("research") } else { slug }
}
