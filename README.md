# unity-test-runner

Cross-platform CLI for compiling Unity projects, running Unity Test Framework tests, and returning compact JSON diagnostics.

## Supported releases

Tagged releases publish native binaries and SHA-256 files for:

- Windows x86-64
- Linux x86-64
- macOS x86-64
- macOS arm64

The runner resolves Unity Hub editors from their standard locations: `C:\Program Files\Unity\Hub\Editor`, `/Applications/Unity/Hub/Editor`, and `~/Unity/Hub/Editor`. Use `--editor` or `--editor-base` for custom installations.

## Getting started

Install the runner as a small two-directory package. This layout is required:

```text
unity-test-runner/
  bin/
    unity-test-runner.exe        # Windows
    unity-test-runner            # Linux/macOS
  config/
    default.toml
```

The executable must be in `bin/`, and the default TOML config must be in `config/`, because the runner looks for `../config/default.toml` relative to the binary directory.

Platform setup guides:

- [Getting started on Windows](docs/getting-started-windows.md)
- [Getting started on Linux](docs/getting-started-linux.md)
- [Getting started on macOS](docs/getting-started-macos.md)

## Output formats

The CLI supports these `--format` values:

- `minimal`: prints `ok` on success; prints compact JSON on failure.
- `minimal-json`: prints `{"ok":true}` on success; prints compact JSON on failure.
- `compact-json`: prints the full result as compact JSON.
- `pretty-json`: prints the full result as formatted JSON.

Default output format is `minimal-json`. MCP tool calls expose `format` as `compact-json`, `minimal-json`, or `pretty-json`, also defaulting to `minimal-json`.

## MCP server

The binary can run as a local MCP server over stdio:

```bash
unity-test-runner mcp
```

It exposes three tools:

- `unity_doctor` — diagnostics, project setup, Unity Editor resolution/version.
- `unity_compile_check` — compilation check only; does not run tests.
- `unity_run_tests` — compiles the project and runs Unity tests.

The MCP server is a thin wrapper around the CLI. It implements `tools/list` and `tools/call`, keeps MCP JSON-RPC messages on stdout, and captures child CLI stdout/stderr into the tool result.

### Protocol smoke test

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}\n' | unity-test-runner mcp
```

Example `tools/call` request:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"unity_compile_check","arguments":{"project":".","timeoutSec":900,"format":"minimal-json"}}}
```

## MCP tool arguments

All tools accept these common optional arguments:

- `project`: Unity project root. Defaults to `.`.
- `editor`: explicit Unity editor executable path.
- `editorBase`: array of extra Unity Hub Editor search roots.
- `config`: explicit TOML config path.
- `format`: `compact-json`, `minimal-json`, or `pretty-json`. Defaults to `minimal-json`.
- `keep`: keep artifacts regardless of status.
- `timeoutSec`: override runner timeout in seconds.
- `logTailLines`: override Unity log tail line count.
- `artifactDir`: override artifacts directory.

`unity_compile_check` additionally accepts compile-check options such as `buildTarget`, `noGraphics`, `acceptApiupdate`, `forgetProjectPath`, and `dryRun`.

`unity_run_tests` additionally accepts test options such as `platform`, `filter`, `category`, `testNames`, `assembly`, `assemblyType`, `requiresPlayMode`, `runSynchronously`, `orderedTestList`, `testSettings`, `playerHeartbeatTimeoutSec`, `buildPlayerPath`, `buildTarget`, `noGraphics`, `acceptApiupdate`, `forgetProjectPath`, and `dryRun`.

Raw `--unity-arg` is intentionally not exposed through MCP. Keep custom Unity arguments in trusted project config or use the CLI directly.

## License

MIT. See `LICENSE-MIT`.
