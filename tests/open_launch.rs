#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Fixture(std::path::PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        // Release the fake desktop handler even if an assertion fails.
        let _ = fs::write(self.0.join("release"), "");
    }
}

#[test]
fn open_returns_while_desktop_handler_is_still_running() {
    let fixture =
        Fixture(std::env::temp_dir().join(format!("obsidian-open-launch-{}", std::process::id())));
    fs::create_dir_all(fixture.0.join("vault")).unwrap();
    let launcher = fixture.0.join("xdg-open");
    fs::write(
        &launcher,
        "#!/bin/sh\nprintf '%s' \"$1\" > \"$TEST_DIR/uri\"\nwhile [ ! -f \"$TEST_DIR/release\" ]; do /bin/sleep 0.05; done\n/usr/bin/touch \"$TEST_DIR/exited\"\n",
    )
    .unwrap();
    fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_obsidian-daily-qs"))
        .args(["--vault", fixture.0.join("vault").to_str().unwrap()])
        .args(["open", "--date", "2026-09-21"])
        .env("PATH", &fixture.0)
        .env("TEST_DIR", &fixture.0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() && fixture.0.join("uri").exists() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("open waited for the desktop handler to exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    let snapshot: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(snapshot["state"], "ok");
    assert_eq!(snapshot["exists"], true);
    assert_eq!(
        fs::read_to_string(fixture.0.join("uri")).unwrap(),
        snapshot["obsidianUri"].as_str().unwrap()
    );
    assert!(!fixture.0.join("exited").exists());

    fs::write(fixture.0.join("release"), "").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.0.join("exited").exists() {
        assert!(Instant::now() < deadline, "fake handler did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::remove_dir_all(&fixture.0).unwrap();
}
