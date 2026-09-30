//! Stable, single-word account name candidates. Persistence owns collision resolution.
use sha2::{Digest as _, Sha256};

use crate::ProviderIdentity;

const ACCOUNT_ALIAS_WORDS: [&str; 44] = [
	"Alex", "Avery", "Bailey", "Blake", "Casey", "Charlie", "Clara", "Dana", "Drew", "Eden",
	"Elliot", "Emery", "Evan", "Finley", "Harper", "Hayden", "Iris", "Jamie", "Jordan", "Kai",
	"Kendall", "Lane", "Liam", "Logan", "Mason", "Maya", "Mia", "Morgan", "Noah", "Nora", "Owen",
	"Paige", "Parker", "Quinn", "Reese", "Remy", "Riley", "Rowan", "Sage", "Sasha", "Sidney",
	"Taylor", "Theo", "Val",
];
// Keep these tables ordered: the provider account ID deterministically selects a name.
const ACCOUNT_ALIAS_SURNAMES: &[&str] = &[
	"Abbott",
	"Adler",
	"Archer",
	"Ashford",
	"Ashton",
	"Atwood",
	"Baldwin",
	"Bancroft",
	"Barrett",
	"Baxter",
	"Bellamy",
	"Bennett",
	"Benson",
	"Bishop",
	"Blair",
	"Blake",
	"Bowen",
	"Bradley",
	"Brooks",
	"Browning",
	"Bryant",
	"Burke",
	"Callahan",
	"Campbell",
	"Carson",
	"Carter",
	"Chandler",
	"Clark",
	"Collins",
	"Cooper",
	"Crawford",
	"Dalton",
	"Dawson",
	"Delaney",
	"Donovan",
	"Douglas",
	"Duncan",
	"Easton",
	"Ellis",
	"Emerson",
	"Everett",
	"Fairfax",
	"Fletcher",
	"Flynn",
	"Ford",
	"Foster",
	"Fox",
	"Franklin",
	"Gardner",
	"Gibson",
	"Graham",
	"Grant",
	"Gray",
	"Griffin",
	"Hale",
	"Hamilton",
	"Harlow",
	"Harper",
	"Harrison",
	"Hart",
	"Hayes",
	"Henderson",
	"Holden",
	"Holland",
	"Hudson",
	"Hughes",
	"Hunter",
	"Irving",
	"Jackson",
	"James",
	"Jensen",
	"Jordan",
	"Keaton",
	"Keller",
	"Kennedy",
	"King",
	"Knight",
	"Lane",
	"Lawson",
	"Lennox",
	"Lewis",
	"Lincoln",
	"Logan",
	"Lowe",
	"Maddox",
	"Marshall",
	"Mason",
	"Maxwell",
	"Mercer",
	"Miller",
	"Monroe",
	"Morgan",
	"Morris",
	"Nash",
	"Nelson",
	"Nolan",
	"Oakley",
	"Oliver",
	"Palmer",
	"Parker",
	"Pierce",
	"Porter",
	"Prescott",
	"Quinn",
	"Reed",
	"Reeves",
	"Reynolds",
	"Rhodes",
	"Riley",
	"Rivera",
	"Rowan",
	"Russell",
	"Sawyer",
	"Scott",
	"Shaw",
	"Sinclair",
	"Spencer",
	"Sterling",
	"Stone",
	"Sullivan",
	"Sutton",
	"Taylor",
	"Turner",
	"Vaughn",
	"Walker",
	"Warren",
	"Wells",
	"Wilder",
];

/// Select a readable name from the provider identity and a collision attempt.
/// Local account IDs, emails, refresh tokens, and UI order do not affect the seed.
pub fn account_alias_candidate(provider: &ProviderIdentity, attempt: u64) -> String {
	let digest = Sha256::new()
		.chain_update(b"decodex/account-alias/v3\0chatgpt\0")
		.chain_update(provider.account_id().as_bytes())
		.finalize();
	let selector = u64::from_be_bytes(digest[..8].try_into().expect("digest segment"));
	let count = ACCOUNT_ALIAS_WORDS.len() + ACCOUNT_ALIAS_SURNAMES.len();

	if attempt < count as u64 {
		let index = ((selector % count as u64 + attempt) % count as u64) as usize;

		return ACCOUNT_ALIAS_WORDS
			.iter()
			.chain(ACCOUNT_ALIAS_SURNAMES)
			.nth(index)
			.expect("alias index is below the combined dictionary length")
			.to_string();
	}

	// A readable single word also works when the short-name dictionary is exhausted.
	let digest = Sha256::new().chain_update(digest).chain_update(attempt.to_be_bytes()).finalize();
	let starts = ["b", "d", "f", "g", "h", "k", "l", "m", "n", "p", "r", "s", "t", "v", "w", "z"];
	let vowels = ["a", "e", "i", "o", "u"];
	let mut name = String::with_capacity(10);

	for pair in digest[..10].as_chunks::<2>().0 {
		name.push_str(starts[pair[0] as usize % starts.len()]);
		name.push_str(vowels[pair[1] as usize % vowels.len()]);
	}

	name[..1].make_ascii_uppercase();

	name
}
