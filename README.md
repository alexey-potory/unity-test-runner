# unity-test-runner

Cross-platform CLI for compiling Unity projects, running Unity Test Framework tests, and returning compact JSON diagnostics.

## Supported releases

Tagged releases publish native binaries and SHA-256 files for:

- Windows x86-64
- Linux x86-64
- macOS x86-64
- macOS arm64

The runner resolves Unity Hub editors from their standard locations: `C:\Program Files\Unity\Hub\Editor`, `/Applications/Unity/Hub/Editor`, and `~/Unity/Hub/Editor`. Use `--editor` or `--editor-base` for custom installations.

## Build and test

```text
cargo test --locked
cargo build --release --locked
```

## Publish a release

Set the version in `Cargo.toml`, commit it, then push the matching tag:

```text
git tag v0.1.0
git push origin v0.1.0
```

The tag workflow tests every target, builds native binaries, writes SHA-256 sidecars, and creates the GitHub Release. A mismatched tag and Cargo version fails before publication.

## License

MIT. See `LICENSE-MIT`.
