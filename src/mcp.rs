use crate::error::RunnerError;
use serde_json::{json, Map, Value};
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

const SERVER_NAME: &str = "unity-test-runner-mcp";
const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];

const TOOL_DOCTOR: &str = "unity_doctor";
const TOOL_COMPILE_CHECK: &str = "unity_compile_check";
const TOOL_RUN_TESTS: &str = "unity_run_tests";

const COMMON_ARGUMENTS: &[&str] = &[
    "project",
    "editor",
    "editorBase",
    "config",
    "format",
    "keep",
    "timeoutSec",
    "logTailLines",
    "artifactDir",
];

const COMPILE_CHECK_ARGUMENTS: &[&str] = &[
    "project",
    "editor",
    "editorBase",
    "config",
    "format",
    "keep",
    "timeoutSec",
    "logTailLines",
    "artifactDir",
    "noGraphics",
    "acceptApiupdate",
    "forgetProjectPath",
    "buildTarget",
    "dryRun",
];

const RUN_TESTS_ARGUMENTS: &[&str] = &[
    "project",
    "editor",
    "editorBase",
    "config",
    "format",
    "keep",
    "timeoutSec",
    "logTailLines",
    "artifactDir",
    "platform",
    "filter",
    "category",
    "testNames",
    "assembly",
    "assemblyType",
    "requiresPlayMode",
    "runSynchronously",
    "orderedTestList",
    "testSettings",
    "playerHeartbeatTimeoutSec",
    "buildPlayerPath",
    "buildTarget",
    "noGraphics",
    "acceptApiupdate",
    "forgetProjectPath",
    "dryRun",
];

/// Serve a small MCP server on stdin/stdout.
///
/// The server intentionally wraps the existing CLI instead of duplicating runner logic:
/// MCP stdout stays reserved for JSON-RPC messages, while the child CLI stdout/stderr are
/// captured and returned as structured tool output.
pub fn serve_stdio() -> Result<(), RunnerError> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle_message(request),
            Err(err) => Some(jsonrpc_error(Value::Null, -32700, format!("Parse error: {err}"), None)),
        };

        if let Some(response) = response {
            write_message(&mut stdout, &response)?;
        }
    }

    Ok(())
}

fn handle_message(message: Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message.get("method").and_then(Value::as_str);

    match method {
        Some("initialize") => id.map(|id| initialize_response(id, message.get("params"))),
        Some("notifications/initialized") => None,
        Some("ping") => id.map(|id| jsonrpc_result(id, json!({}))),
        Some("tools/list") => id.map(|id| jsonrpc_result(id, list_tools_result())),
        Some("tools/call") => id.map(|id| call_tool_result(id, message.get("params"))),
        Some(method) => id.map(|id| jsonrpc_error(id, -32601, format!("Method not found: {method}"), None)),
        None => id.map(|id| jsonrpc_error(id, -32600, "Invalid Request: missing method", None)),
    }
}

fn initialize_response(id: Value, params: Option<&Value>) -> Value {
    let requested = params
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str);
    let protocol_version = requested
        .filter(|version| SUPPORTED_PROTOCOL_VERSIONS.contains(version))
        .unwrap_or(LATEST_PROTOCOL_VERSION);

    jsonrpc_result(
        id,
        json!({
            "protocolVersion": protocol_version,
            "capabilities": {
                "tools": {
                    "listChanged": false
                }
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "title": "Unity Test Runner MCP",
                "version": env!("CARGO_PKG_VERSION"),
                "description": "MCP wrapper around unity-test-runner CLI tools."
            },
            "instructions": "Use unity_doctor for diagnostics/editor version, unity_compile_check for compilation only, and unity_run_tests when test execution is needed. These tools are independent. Tool results include parsed runner JSON in structuredContent.output when available."
        }),
    )
}

fn list_tools_result() -> Value {
    json!({
        "tools": tool_definitions()
    })
}

fn call_tool_result(id: Value, params: Option<&Value>) -> Value {
    let Some(params) = params.and_then(Value::as_object) else {
        return jsonrpc_error(id, -32602, "Invalid params: expected object", None);
    };

    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return jsonrpc_error(id, -32602, "Invalid params: missing tool name", None);
    };

    let arguments = params
        .get("arguments")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let result = match name {
        TOOL_DOCTOR => call_doctor(&arguments),
        TOOL_COMPILE_CHECK => call_compile_check(&arguments),
        TOOL_RUN_TESTS => call_run_tests(&arguments),
        _ => return jsonrpc_error(id, -32602, format!("Unknown tool: {name}"), None),
    };

    jsonrpc_result(id, result)
}

fn call_doctor(arguments: &Map<String, Value>) -> Value {
    let mut input = ToolInput::new(arguments);
    input.reject_unknown(COMMON_ARGUMENTS);
    let mut args = vec![OsString::from("doctor")];
    push_common_args(&mut args, &mut input);
    finalize_tool_call(input, args)
}

fn call_compile_check(arguments: &Map<String, Value>) -> Value {
    let mut input = ToolInput::new(arguments);
    input.reject_unknown(COMPILE_CHECK_ARGUMENTS);
    let mut args = vec![OsString::from("compile-check")];
    push_common_args(&mut args, &mut input);

    push_bool_flag(&mut args, &mut input, "noGraphics", "--no-graphics");
    push_bool_flag(&mut args, &mut input, "acceptApiupdate", "--accept-apiupdate");
    push_bool_flag(&mut args, &mut input, "forgetProjectPath", "--forget-project-path");
    push_string_arg(&mut args, &mut input, "buildTarget", "--build-target");
    push_bool_flag(&mut args, &mut input, "dryRun", "--dry-run");

    finalize_tool_call(input, args)
}

fn call_run_tests(arguments: &Map<String, Value>) -> Value {
    let mut input = ToolInput::new(arguments);
    input.reject_unknown(RUN_TESTS_ARGUMENTS);
    let mut args = vec![OsString::from("run")];
    push_common_args(&mut args, &mut input);

    match input.optional_enum("platform", &["EditMode", "PlayMode", "All"]) {
        Some(platform) => push_pair(&mut args, "--platform", platform),
        None => push_pair(&mut args, "--platform", "EditMode"),
    }
    push_string_arg(&mut args, &mut input, "filter", "--filter");
    push_string_arg(&mut args, &mut input, "category", "--category");
    push_string_arg(&mut args, &mut input, "testNames", "--test-names");
    push_string_array_args(&mut args, &mut input, "assembly", "--assembly");

    if let Some(assembly_type) = input.optional_enum("assemblyType", &["EditorOnly", "EditorAndPlatforms"]) {
        push_pair(&mut args, "--assembly-type", assembly_type);
    }

    if let Some(value) = input.optional_bool("requiresPlayMode") {
        push_pair(&mut args, "--requires-play-mode", if value { "true" } else { "false" });
    }

    push_bool_flag(&mut args, &mut input, "runSynchronously", "--run-synchronously");
    push_string_arg(&mut args, &mut input, "orderedTestList", "--ordered-test-list");
    push_string_arg(&mut args, &mut input, "testSettings", "--test-settings");
    push_u64_arg(&mut args, &mut input, "playerHeartbeatTimeoutSec", "--player-heartbeat-timeout");
    push_string_arg(&mut args, &mut input, "buildPlayerPath", "--build-player-path");
    push_string_arg(&mut args, &mut input, "buildTarget", "--build-target");
    push_bool_flag(&mut args, &mut input, "noGraphics", "--no-graphics");
    push_bool_flag(&mut args, &mut input, "acceptApiupdate", "--accept-apiupdate");
    push_bool_flag(&mut args, &mut input, "forgetProjectPath", "--forget-project-path");
    push_bool_flag(&mut args, &mut input, "dryRun", "--dry-run");

    finalize_tool_call(input, args)
}

fn push_common_args(args: &mut Vec<OsString>, input: &mut ToolInput<'_>) {
    push_string_arg(args, input, "project", "--project");
    if !input.has("project") {
        push_pair(args, "--project", ".");
    }

    push_string_arg(args, input, "editor", "--editor");
    push_string_array_args(args, input, "editorBase", "--editor-base");
    push_string_arg(args, input, "config", "--config");
    match input.optional_enum("format", &["compact-json", "minimal-json", "pretty-json"]) {
        Some(format) => push_pair(args, "--format", format),
        None => push_pair(args, "--format", "minimal-json"),
    }
    push_bool_flag(args, input, "keep", "--keep");
    push_u64_arg(args, input, "timeoutSec", "--timeout");
    push_usize_arg(args, input, "logTailLines", "--log-tail");
    push_string_arg(args, input, "artifactDir", "--artifact-dir");
}

fn finalize_tool_call(input: ToolInput<'_>, args: Vec<OsString>) -> Value {
    if input.has_errors() {
        return tool_error(input.error_text());
    }

    match run_self(args) {
        Ok(result) => result,
        Err(err) => tool_error(err),
    }
}

fn run_self(args: Vec<OsString>) -> Result<Value, String> {
    let exe = std::env::current_exe().map_err(|err| format!("Cannot resolve current executable: {err}"))?;
    let output = Command::new(&exe)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| format!("Failed to launch {}: {err}", exe.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let parsed_stdout = serde_json::from_str::<Value>(&stdout).ok();

    let mut structured = Map::new();
    structured.insert("ok".to_string(), parsed_stdout
        .as_ref()
        .and_then(|value| value.get("ok"))
        .and_then(Value::as_bool)
        .map(Value::Bool)
        .unwrap_or(Value::Bool(output.status.success())));
    structured.insert("exitCode".to_string(), output.status.code().map_or(Value::Null, |code| json!(code)));
    structured.insert("command".to_string(), json!(command_for_display(&args)));

    if let Some(parsed) = parsed_stdout {
        structured.insert("output".to_string(), parsed);
    } else if !stdout.is_empty() {
        structured.insert("stdout".to_string(), Value::String(truncate_text(&stdout)));
    }

    if !stderr.is_empty() {
        structured.insert("stderr".to_string(), Value::String(truncate_text(&stderr)));
    }

    let structured = Value::Object(structured);
    let text = serde_json::to_string(&structured).unwrap_or_else(|_| "{}".to_string());

    Ok(json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ],
        "structuredContent": structured,
        "isError": false
    }))
}

fn tool_error(message: impl Into<String>) -> Value {
    let message = message.into();
    json!({
        "content": [
            {
                "type": "text",
                "text": message
            }
        ],
        "structuredContent": {
            "ok": false,
            "error": message
        },
        "isError": true
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![doctor_tool(), compile_check_tool(), run_tests_tool()]
}

fn doctor_tool() -> Value {
    json!({
        "name": TOOL_DOCTOR,
        "title": "Unity Doctor",
        "description": "Validate Unity project, runner config, Unity editor resolution, and artifact directory. Does not run tests.",
        "inputSchema": common_schema(),
        "annotations": {
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false
        }
    })
}

fn compile_check_tool() -> Value {
    let mut schema = common_schema_object();
    let properties = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("common schema properties are object");
    properties.extend([
        ("noGraphics".to_string(), json!({ "type": "boolean", "description": "Pass Unity -nographics." })),
        ("acceptApiupdate".to_string(), json!({ "type": "boolean", "description": "Pass Unity -accept-apiupdate." })),
        ("forgetProjectPath".to_string(), json!({ "type": "boolean", "description": "Pass Unity -forgetProjectPath." })),
        ("buildTarget".to_string(), json!({ "type": "string", "description": "Unity -buildTarget name, for example win64, android, ios, or webgl." })),
        ("dryRun".to_string(), json!({ "type": "boolean", "description": "Build and return the Unity command without launching Unity." })),
    ]);

    json!({
        "name": TOOL_COMPILE_CHECK,
        "title": "Unity Compile Check",
        "description": "Open Unity in batchmode and verify that project scripts compile. Returns JSON diagnostics.",
        "inputSchema": Value::Object(schema),
        "annotations": {
            "readOnlyHint": false,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false
        }
    })
}

fn run_tests_tool() -> Value {
    let mut schema = common_schema_object();
    let properties = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("common schema properties are object");
    properties.extend([
        ("platform".to_string(), json!({ "type": "string", "enum": ["EditMode", "PlayMode", "All"], "description": "Unity Test Framework platform. Defaults to EditMode." })),
        ("filter".to_string(), json!({ "type": "string", "description": "Unity -testFilter." })),
        ("category".to_string(), json!({ "type": "string", "description": "Unity -testCategory, e.g. Smoke, A;B, or !Slow." })),
        ("testNames".to_string(), json!({ "type": "string", "description": "Unity -testNames: semicolon-separated full test names." })),
        ("assembly".to_string(), json!({ "type": "array", "items": { "type": "string" }, "description": "Unity -assemblyNames values. Repeat values are passed as repeated --assembly flags." })),
        ("assemblyType".to_string(), json!({ "type": "string", "enum": ["EditorOnly", "EditorAndPlatforms"], "description": "Unity -assemblyType." })),
        ("requiresPlayMode".to_string(), json!({ "type": "boolean", "description": "Unity -requiresPlayMode=true|false." })),
        ("runSynchronously".to_string(), json!({ "type": "boolean", "description": "Unity -runSynchronously. EditMode only." })),
        ("orderedTestList".to_string(), json!({ "type": "string", "description": "Path passed to Unity -orderedTestListFile." })),
        ("testSettings".to_string(), json!({ "type": "string", "description": "Path passed to Unity -testSettingsFile." })),
        ("playerHeartbeatTimeoutSec".to_string(), json!({ "type": "integer", "minimum": 1, "description": "Unity -playerHeartbeatTimeout seconds for player-based tests." })),
        ("buildPlayerPath".to_string(), json!({ "type": "string", "description": "Path passed to Unity -buildPlayerPath." })),
        ("buildTarget".to_string(), json!({ "type": "string", "description": "Unity -buildTarget name, for example win64, android, ios, or webgl." })),
        ("noGraphics".to_string(), json!({ "type": "boolean", "description": "Pass Unity -nographics." })),
        ("acceptApiupdate".to_string(), json!({ "type": "boolean", "description": "Pass Unity -accept-apiupdate." })),
        ("forgetProjectPath".to_string(), json!({ "type": "boolean", "description": "Pass Unity -forgetProjectPath." })),
        ("dryRun".to_string(), json!({ "type": "boolean", "description": "Build and return the Unity command without launching Unity." })),
    ]);

    json!({
        "name": TOOL_RUN_TESTS,
        "title": "Unity Run Tests",
        "description": "Run Unity Test Framework tests and return JSON diagnostics, summaries, failures, and artifacts.",
        "inputSchema": Value::Object(schema),
        "annotations": {
            "readOnlyHint": false,
            "destructiveHint": false,
            "idempotentHint": false,
            "openWorldHint": false
        }
    })
}

fn common_schema() -> Value {
    Value::Object(common_schema_object())
}

fn common_schema_object() -> Map<String, Value> {
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("additionalProperties".to_string(), Value::Bool(false));
    schema.insert(
        "properties".to_string(),
        json!({
            "project": {
                "type": "string",
                "description": "Unity project root. Defaults to current directory."
            },
            "editor": {
                "type": "string",
                "description": "Explicit Unity editor executable path."
            },
            "editorBase": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Extra Unity Hub Editor search roots."
            },
            "config": {
                "type": "string",
                "description": "Extra/explicit TOML config path."
            },
            "format": {
                "type": "string",
                "enum": ["compact-json", "minimal-json", "pretty-json"],
                "default": "minimal-json",
                "description": "Runner output format for the wrapped CLI call. MCP omits plain minimal because it is not always JSON."
            },
            "keep": {
                "type": "boolean",
                "description": "Keep artifacts regardless of status."
            },
            "timeoutSec": {
                "type": "integer",
                "minimum": 1,
                "description": "Override runner timeout in seconds."
            },
            "logTailLines": {
                "type": "integer",
                "minimum": 0,
                "description": "Override Unity log tail line count."
            },
            "artifactDir": {
                "type": "string",
                "description": "Override artifacts directory. Supports {temp}, {project}, {project_hash}, {platform}."
            }
        }),
    );
    schema
}

fn push_string_arg(args: &mut Vec<OsString>, input: &mut ToolInput<'_>, key: &str, flag: &str) {
    if let Some(value) = input.optional_string(key) {
        push_pair(args, flag, value);
    }
}

fn push_string_array_args(args: &mut Vec<OsString>, input: &mut ToolInput<'_>, key: &str, flag: &str) {
    for value in input.optional_string_array(key) {
        push_pair(args, flag, value);
    }
}

fn push_bool_flag(args: &mut Vec<OsString>, input: &mut ToolInput<'_>, key: &str, flag: &str) {
    if input.optional_bool(key).unwrap_or(false) {
        args.push(OsString::from(flag));
    }
}

fn push_u64_arg(args: &mut Vec<OsString>, input: &mut ToolInput<'_>, key: &str, flag: &str) {
    if let Some(value) = input.optional_u64(key) {
        push_pair(args, flag, value.to_string());
    }
}

fn push_usize_arg(args: &mut Vec<OsString>, input: &mut ToolInput<'_>, key: &str, flag: &str) {
    if let Some(value) = input.optional_u64(key) {
        push_pair(args, flag, value.to_string());
    }
}

fn push_pair(args: &mut Vec<OsString>, flag: impl Into<OsString>, value: impl Into<OsString>) {
    args.push(flag.into());
    args.push(value.into());
}

fn command_for_display(args: &[OsString]) -> Vec<String> {
    let mut display = Vec::with_capacity(args.len() + 1);
    display.push("unity-test-runner".to_string());
    display.extend(args.iter().map(|arg| arg.to_string_lossy().into_owned()));
    display
}

fn truncate_text(text: &str) -> String {
    const MAX_CHARS: usize = 65_536;
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }

    let mut truncated = text.chars().take(MAX_CHARS).collect::<String>();
    truncated.push_str("\n...[truncated by unity-test-runner MCP wrapper]");
    truncated
}

struct ToolInput<'a> {
    values: &'a Map<String, Value>,
    errors: Vec<String>,
}

impl<'a> ToolInput<'a> {
    fn new(values: &'a Map<String, Value>) -> Self {
        Self {
            values,
            errors: Vec::new(),
        }
    }

    fn has(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    fn reject_unknown(&mut self, allowed: &[&str]) {
        for key in self.values.keys() {
            if !allowed.iter().any(|allowed_key| *allowed_key == key) {
                self.errors.push(format!("unknown argument: {key}"));
            }
        }
    }

    fn optional_string(&mut self, key: &str) -> Option<String> {
        match self.values.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) if !value.trim().is_empty() => Some(value.clone()),
            Some(Value::String(_)) => {
                self.errors.push(format!("{key} must not be empty"));
                None
            }
            Some(_) => {
                self.errors.push(format!("{key} must be a string"));
                None
            }
        }
    }

    fn optional_string_array(&mut self, key: &str) -> Vec<String> {
        match self.values.get(key) {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .enumerate()
                .filter_map(|(idx, value)| match value {
                    Value::String(item) if !item.trim().is_empty() => Some(item.clone()),
                    Value::String(_) => {
                        self.errors.push(format!("{key}[{idx}] must not be empty"));
                        None
                    }
                    _ => {
                        self.errors.push(format!("{key}[{idx}] must be a string"));
                        None
                    }
                })
                .collect(),
            Some(_) => {
                self.errors.push(format!("{key} must be an array of strings"));
                Vec::new()
            }
        }
    }

    fn optional_bool(&mut self, key: &str) -> Option<bool> {
        match self.values.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::Bool(value)) => Some(*value),
            Some(_) => {
                self.errors.push(format!("{key} must be a boolean"));
                None
            }
        }
    }

    fn optional_u64(&mut self, key: &str) -> Option<u64> {
        match self.values.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::Number(value)) => match value.as_u64() {
                Some(number) => Some(number),
                None => {
                    self.errors.push(format!("{key} must be a non-negative integer"));
                    None
                }
            },
            Some(_) => {
                self.errors.push(format!("{key} must be an integer"));
                None
            }
        }
    }

    fn optional_enum(&mut self, key: &str, allowed: &[&str]) -> Option<String> {
        let value = self.optional_string(key)?;
        if allowed.iter().any(|allowed_value| *allowed_value == value) {
            Some(value)
        } else {
            self.errors.push(format!("{key} must be one of: {}", allowed.join(", ")));
            None
        }
    }

    fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    fn error_text(&self) -> String {
        format!("Invalid tool arguments: {}", self.errors.join("; "))
    }
}

fn jsonrpc_result(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>, data: Option<Value>) -> Value {
    let mut error = Map::new();
    error.insert("code".to_string(), json!(code));
    error.insert("message".to_string(), Value::String(message.into()));
    if let Some(data) = data {
        error.insert("data".to_string(), data);
    }

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": Value::Object(error)
    })
}

fn write_message(stdout: &mut io::Stdout, message: &Value) -> Result<(), RunnerError> {
    serde_json::to_writer(&mut *stdout, message)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
