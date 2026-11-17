//! Writes an empty project, with no tracks, to the path given.
//!
//! The desktop drop check opens it so Vibez auto-saves what the drops add.

use std::error::Error;
use std::path::PathBuf;

use vibez_project::project_format_v1::save_project_v1;
use vibez_project::Project;

fn main() -> Result<(), Box<dyn Error>> {
    let path = PathBuf::from(std::env::args().nth(1).ok_or("give the project's path")?);
    let project = Project {
        name: "Desktop drop".to_string(),
        ..Project::default()
    };
    save_project_v1(&path, None, project)?;
    Ok(())
}
