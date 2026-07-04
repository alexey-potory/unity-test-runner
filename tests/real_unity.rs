use std::process::Command;

#[test]
#[ignore = "requires a local Unity project and installed Unity editor"]
fn real_unity_doctor() {
    let exe = env!("CARGO_BIN_EXE_unity-test-runner");
    let status = Command::new(exe)
        .arg("doctor")
        .arg("--project")
        .arg(".")
        .status()
        .expect("failed to run unity-test-runner doctor");
    assert!(status.success());
}
