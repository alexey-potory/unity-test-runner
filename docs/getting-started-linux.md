# Getting started on Linux

This guide installs `unity-test-runner` as a local Codex MCP server on Linux.

## 1. Build

From the Rust project root:

```bash
cargo build --release
```

The binary is created at:

```text
target/release/unity-test-runner
```

## 2. Install files

Use this exact package layout:

```text
~/Tools/unity-test-runner/
  bin/
    unity-test-runner
  config/
    default.toml
```

The `bin/` and `config/` directories are required. The runner looks for `../config/default.toml` relative to the binary directory.

```bash
install_dir="$HOME/Tools/unity-test-runner"

mkdir -p "$install_dir/bin" "$install_dir/config"
cp target/release/unity-test-runner "$install_dir/bin/"
cp config/default.toml "$install_dir/config/"
chmod +x "$install_dir/bin/unity-test-runner"
```

## 3. Configure Codex MCP

Edit:

```text
~/.codex/config.toml
```

Add:

```toml
[mcp_servers.unity_tests]
command = "/home/<you>/Tools/unity-test-runner/bin/unity-test-runner"
args = ["mcp"]
startup_timeout_sec = 10
tool_timeout_sec = 1200
enabled = true
default_tools_approval_mode = "prompt"

[mcp_servers.unity_tests.tools.unity_doctor]
approval_mode = "approve"

[mcp_servers.unity_tests.tools.unity_compile_check]
approval_mode = "prompt"

[mcp_servers.unity_tests.tools.unity_run_tests]
approval_mode = "prompt"
```

For a global setup used across multiple Unity projects, do not set `cwd`. Start Codex from the Unity project root.

For a single-project setup, you may add:

```toml
cwd = "/home/<you>/projects/MyUnityGame"
```

## 4. Smoke test

Without launching Unity:

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}\n' | "$HOME/Tools/unity-test-runner/bin/unity-test-runner" mcp
```

In Codex, run:

```text
/mcp
```

You should see the `unity_tests` MCP server and the three tools: `unity_doctor`, `unity_compile_check`, and `unity_run_tests`.
