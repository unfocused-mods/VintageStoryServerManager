//! Background process constructors shared by the Manager, desktop and agent.
//! Suppress Windows console creation without hiding browser windows opened by authentication.
use std::ffi::OsStr;

pub fn command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)] // Only Windows needs to change process creation flags.
    let mut command = tokio::process::Command::new(program);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command
}

pub fn std_command(program: impl AsRef<OsStr>) -> std::process::Command {
    #[allow(unused_mut)] // Only Windows needs to change process creation flags.
    let mut command = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    const PROBE: &str = "subprocess::tests::windows_child_console_probe";
    #[test]
    fn windows_child_console_probe() {
        if std::env::var_os("VSM_TEST_CONSOLE_PROBE").is_none() {
            return;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetConsoleWindow() -> *mut std::ffi::c_void;
        }
        assert!(
            unsafe { GetConsoleWindow() }.is_null(),
            "Background child created a console window"
        );
        println!("background child output is retained");
    }
    #[test]
    fn synchronous_background_child_has_no_console() {
        let output = std_command(std::env::current_exe().unwrap())
            .args(["--exact", PROBE, "--nocapture"])
            .env("VSM_TEST_CONSOLE_PROBE", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("background child output is retained")
        );
    }
    #[tokio::test]
    async fn asynchronous_background_child_has_no_console() {
        let output = command(std::env::current_exe().unwrap())
            .args(["--exact", PROBE, "--nocapture"])
            .env("VSM_TEST_CONSOLE_PROBE", "1")
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("background child output is retained")
        );
    }
}
