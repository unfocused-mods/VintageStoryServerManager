# Contributing

This community project welcomes small, reviewable contributions. Start with the README and the implementation status document. Do not describe unverified cloud behavior as tested.

## Development

Install stable Rust, Node.js 22+, and the platform prerequisites for Tauri 2. On Linux the core also needs `libdbus-1-dev` and `pkg-config` for OS credential storage.

Run `npm ci`, `npm run build`, `npm test`, `cargo test -p vsm`, and `cargo clippy -p vsm --all-targets -- -D warnings`. Format Rust using `cargo fmt --all`. Generate API artifacts with `npm run typegen` when contracts change.

Provider changes need failure-path coverage, including partial resource creation, token expiry, ownership mismatch, and rate limits. Never include credentials, real world data, or customer identifiers in fixtures.

Cloud smoke tests incur costs and must run only with explicitly supplied test accounts. Tests must report resource IDs and cleanup results. An operation is not complete merely because a provider accepted its request.

Contributions are licensed under Apache-2.0. Keep the community welcoming, explain tradeoffs plainly, and respect mod authors' licensing terms.
