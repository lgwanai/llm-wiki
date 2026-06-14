//! Self-update: git pull + backup.

use crate::error::WikiResult;

pub fn update_from_git() -> WikiResult<()> {
    let output = duct::cmd!("git", "pull").stderr_to_stdout().run();
    match output {
        Ok(o) => {
            println!("{}", String::from_utf8_lossy(&o.stdout));
            Ok(())
        }
        Err(e) => {
            eprintln!("Git pull failed: {e}");
            Err(crate::error::WikiError::Internal(format!("Update failed: {e}")))
        }
    }
}
