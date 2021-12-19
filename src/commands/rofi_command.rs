use crate::commands::Command;
use anyhow::{anyhow, Result};

pub struct RofiCommand;

impl Command for RofiCommand {
    fn execute(&self) -> Result<()> {
        std::process::Command::new("rofi")
            .arg("-show")
            .arg("run")
            .spawn()
            .map(|_| ())
            .map_err(|e| anyhow!(e))
    }
}
