use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn mcp_tools_list_and_call_are_handled() {
    let exe = env!("CARGO_BIN_EXE_unity-test-runner");
    let mut child = Command::new(exe)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start MCP server");

    {
        let mut stdin = child.stdin.take().expect("missing MCP stdin");
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{}}}}"#
        )
        .expect("failed to write tools/list request");
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"missing_tool","arguments":{{}}}}}}"#
        )
        .expect("failed to write tools/call request");
        stdin.flush().expect("failed to flush MCP stdin");
    }

    let stdout = child.stdout.take().expect("missing MCP stdout");
    let mut lines = BufReader::new(stdout).lines();

    let list_response: Value = serde_json::from_str(
        &lines
            .next()
            .expect("missing tools/list response")
            .expect("failed to read tools/list response"),
    )
    .expect("tools/list response is not valid JSON");
    let tools = list_response["result"]["tools"]
        .as_array()
        .expect("tools/list result should contain a tools array");
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(names, vec!["unity_doctor", "unity_compile_check", "unity_run_tests"]);

    for tool in tools {
        let format_values: Vec<&str> = tool["inputSchema"]["properties"]["format"]["enum"]
            .as_array()
            .expect("format enum should be an array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(format_values, vec!["compact-json", "minimal-json", "pretty-json"]);
        assert_eq!(
            tool["inputSchema"]["properties"]["format"]["default"].as_str(),
            Some("minimal-json")
        );
    }

    let call_response: Value = serde_json::from_str(
        &lines
            .next()
            .expect("missing tools/call response")
            .expect("failed to read tools/call response"),
    )
    .expect("tools/call response is not valid JSON");
    assert_eq!(call_response["error"]["code"].as_i64(), Some(-32602));
    assert!(call_response["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("Unknown tool"));

    let status = child.wait().expect("failed to wait for MCP server");
    assert!(status.success());
}
