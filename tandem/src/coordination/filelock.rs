use std::fs::File;
use std::io::Result;

pub struct FileLockGuard<'a> {
    file: &'a File,
}

impl<'a> Drop for FileLockGuard<'a> {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn exclusive_lock(file: &File) -> Result<FileLockGuard<'_>> {
    file.lock()?;
    Ok(FileLockGuard { file })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_and_unlock() {
        let path = std::env::temp_dir().join(format!("tandem-lock-test-{}", uuid::Uuid::new_v4()));
        let file = File::create(&path).unwrap();

        {
            let _guard = exclusive_lock(&file).unwrap();
        }

        let _ = std::fs::remove_file(&path);
    }
}
