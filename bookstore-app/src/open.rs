use std::path::Path;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use std::process::Command;

#[cfg(target_os = "windows")]
const OPEN_BOOK_IN_DIR_PY: &str = r#"import sys
import subprocess
import os

path = os.path.join(os.getenv('WINDIR'), 'explorer.exe')
subprocess.Popen(f'{path} /select,"{sys.argv[1]}"')
"#;

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
/// Opens the book and selects it, in File Explorer on Windows, or in Nautilus on Linux.
/// Other operating systems not currently supported
///
/// # Arguments
///
/// * ` book ` - The book to open.
/// * ` index ` - The index of the path to open.
///
/// # Errors
/// This function may error if the book's variants do not exist,
/// or if the command itself fails.
pub(crate) fn open_in_dir<P: AsRef<Path>>(path: P) -> Result<(), std::io::Error> {
    #[cfg(target_os = "windows")]
    {
        use std::io::Write;

        let mut open_book_path = std::env::current_dir()?;
        open_book_path.push("open_book_in_dir.py");

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&open_book_path)?;

        file.write_all(OPEN_BOOK_IN_DIR_PY.as_bytes())?;

        // TODO: Find a way to do this entirely in Rust
        Command::new("python")
            .args(&[open_book_path.as_path(), path.as_ref()])
            .spawn()?;
    }
    #[cfg(target_os = "linux")]
    Command::new("nautilus")
        .arg("--select")
        .arg(path.as_ref())
        .spawn()?;
    #[cfg(target_os = "macos")]
    Command::new("open").arg("-R").arg(path.as_ref()).spawn()?;

    Ok(())
}
