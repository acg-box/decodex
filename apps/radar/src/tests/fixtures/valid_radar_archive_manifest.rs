use serde_json::Value;

pub(crate) fn valid_radar_archive_manifest() -> Value {
	serde_json::json!({
		"schema": "radar_archive_manifest/v1",
		"archive_id": "radar-archive-2026-06-02",
		"created_at": "2026-06-02T03:30:00Z",
		"retention_days": 21,
		"source_commit": "0123456789abcdef0123456789abcdef01234567",
		"release_tag": "radar-archive-2026-06-02",
		"release_url": "https://github.com/hack-ink/decodex/releases/tag/radar-archive-2026-06-02",
		"archive_asset": {
			"name": "radar-archive-2026-06-02.tar.zst",
			"size_bytes": 1_024,
			"sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
		},
		"checksum_asset": {
			"name": "SHA256SUMS",
			"sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
		},
		"files": [
			{
				"path": ".agent/automations/radar/cache/github/bundles/openai-codex-pr-22414.json",
				"kind": "bundle",
				"sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
				"size_bytes": 512
			}
		]
	})
}
