pub mod golden;
pub mod parity;
pub mod ready_deps;
pub mod tools;
pub mod topology;

#[allow(unused_imports)]
pub use golden::assert_golden;
#[allow(unused_imports)]
pub use tools::require_tool;
#[allow(unused_imports)]
pub use topology::capture_topology;

use std::path::{Path, PathBuf};

#[allow(dead_code)]
pub struct TempDir {
    path: PathBuf,
}

#[allow(dead_code)]
impl TempDir {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!("tandem-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
