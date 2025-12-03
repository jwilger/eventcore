use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;

pub struct TestDbLock {
    file: File,
}

impl TestDbLock {
    pub fn acquire() -> Self {
        let mut path = std::env::temp_dir();
        path.push("eventcore_postgres_db.lock");

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .expect("should create postgres test lock file");

        file.lock_exclusive()
            .expect("should acquire exclusive postgres test lock");

        Self { file }
    }
}

impl Drop for TestDbLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
