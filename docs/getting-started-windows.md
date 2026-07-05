# Getting started on Windows

This guide installs `unity-test-runner` as a local Codex MCP server on Windows.

## 1. Build

From the Rust project root:

```powershell
cargo build --release
```

The binary is created at:

```text
target/release/unity-test-runner.exe
```

## 2. Install files

Use this exact package layout:

```text
  bin/
    unity-test-runner.exe
  config/
    default.toml
```

The `bin/` and `config/` directories are required. The runner looks for `../config/default.toml` relative to the binary directory.

PowerShell:

```powershell
$InstallDir = "$env:USERPROFILE/Tools/unity-test-runner"

New-Item -ItemType Directory -Force "$InstallDir/bin" | Out-Null
New-Item -ItemType Directory -Force "$InstallDir/config" | Out-Null

Copy-Item "target/release/unity-test-runner.exe" "$InstallDir/bin/"
Copy-Item "config/default.toml" "$InstallDir/config/"
```

## 3. Configure Codex MCP

Edit:

```text
C:/Users/<you>/.codex/config.toml
```

Add:

```toml
[mcp_servers.unity_tests]
command = "C:/Users/<you>/Tools/unity-test-runner/bin/unity-test-runner.exe"
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
cwd = "D:/Projects/MyUnityGame"
```

## 4. Smoke test

In Codex, run:

```text
/mcp
```

You should see the `unity_tests` MCP server