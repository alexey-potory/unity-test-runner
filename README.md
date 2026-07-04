# unity-test-runner

Cross-platform CLI for compiling Unity projects, running Unity Test Framework tests, and returning compact JSON diagnostics.

## Supported releases

Tagged releases publish native binaries and SHA-256 files for:

- Windows x86-64
- Linux x86-64
- macOS x86-64
- macOS arm64

The runner resolves Unity Hub editors from their standard locations: `C:\Program Files\Unity\Hub\Editor`, `/Applications/Unity/Hub/Editor`, and `~/Unity/Hub/Editor`. Use `--editor` or `--editor-base` for custom installations.

## License

MIT. See `LICENSE-MIT`.
