use anyhow::Result;
use std::process::Command;

pub fn send_notification(title: &str, message: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        send_macos_notification(title, message)
    }

    #[cfg(target_os = "linux")]
    {
        send_linux_notification(title, message)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn send_macos_notification(title: &str, message: &str) -> Result<()> {
    let script = format!(
        r#"display notification "{}" with title "{}""#,
        message.replace('\"', "\\\""),
        title.replace('\"', "\\\"")
    );

    Command::new("osascript").arg("-e").arg(&script).output()?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn send_linux_notification(title: &str, message: &str) -> Result<()> {
    Command::new("notify-send")
        .arg(title)
        .arg(message)
        .arg("--icon=system-software-update")
        .output()?;

    Ok(())
}
