use clap::Parser;
use signal_hook::flag;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
};

#[derive(Parser)]
#[command(name = "rotatelogs")]
#[command(about = "On-demand logrotate for piped stdout")]
struct Args {
    /// Output log file path
    #[arg(short, long)]
    file: String,

    /// Maximum file size in bytes before rotation (0 = no automatic rotation)
    #[arg(short, long, default_value = "0")]
    size: u64,

    /// Maximum number of lines before rotation
    #[arg(short, long)]
    lines: Option<u64>,

    /// Number of rotated files to keep
    #[arg(short, long, default_value = "5")]
    count: usize,

    /// Rotate immediately on startup
    #[arg(short, long)]
    rotate: bool,
}

struct LogRotator {
    base_path: String,
    max_size: u64,
    max_lines: Option<u64>,
    max_count: usize,
    current_file: Option<BufWriter<File>>,
    current_size: u64,
    current_lines: u64,
}

impl LogRotator {
    fn new(path: &str, max_size: u64, max_lines: Option<u64>, max_count: usize) -> anyhow::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        
        let metadata = file.metadata()?;
        let current_size = metadata.len();
        
        // Count existing lines
        let mut current_lines = 0;
        if current_size > 0 {
            let mut reader = BufReader::new(File::open(path)?);
            let mut buffer = Vec::new();
            while reader.read_until(b'\n', &mut buffer)? > 0 {
                if buffer.ends_with(b"\n") {
                    current_lines += 1;
                }
                buffer.clear();
            }
        }

        Ok(Self {
            base_path: path.to_string(),
            max_size,
            max_lines,
            max_count,
            current_file: Some(BufWriter::new(file)),
            current_size,
            current_lines,
        })
    }

    fn write_line(&mut self, line: &str) -> anyhow::Result<()> {
        let line_bytes = line.as_bytes();
        let line_len = line_bytes.len() as u64;
        let line_with_newline_len = line_len + 1;

        if let Some(ref mut writer) = self.current_file {
            writer.write_all(line_bytes)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            self.current_size += line_with_newline_len;
            self.current_lines += 1;
        }

        // Check if rotation is needed after writing this line
        let size_exceeded = self.max_size > 0 && self.current_size > self.max_size;
        let lines_exceeded = self.max_lines.is_some_and(|max| self.current_lines >= max);

        if size_exceeded || lines_exceeded {
            self.rotate()?;
        }

        Ok(())
    }

    fn rotate(&mut self) -> anyhow::Result<()> {
        // Flush and close current file first
        if let Some(mut writer) = self.current_file.take() {
            writer.flush()?;
            std::mem::drop(writer);
        }

        // Force a sync to ensure all data is written to disk
        let file = std::fs::File::open(&self.base_path)?;
        file.sync_all()?;
        std::mem::drop(file);
        
        // Wait a moment for file system operations to complete
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Rotate existing files
        for i in (1..=self.max_count).rev() {
            let old_path = format!("{}.{}", self.base_path, i);
            let new_path = format!("{}.{}", self.base_path, i + 1);
            
            if Path::new(&old_path).exists() {
                if i == self.max_count {
                    fs::remove_file(&old_path)?;
                } else {
                    fs::rename(&old_path, &new_path)?;
                }
            }
        }

        // Copy current file to .1 and then truncate
        if Path::new(&self.base_path).exists() {
            let rotated_path = format!("{}.1", self.base_path);
            fs::copy(&self.base_path, &rotated_path)?;
            
            // Truncate the original file
            let file = OpenOptions::new().write(true).truncate(true).open(&self.base_path)?;
            file.set_len(0)?;
            drop(file);
        }

        // Create new current file
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.base_path)?;
        
        self.current_file = Some(BufWriter::new(file));
        self.current_size = 0;
        self.current_lines = 0;

        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    
    // Check if help was requested - if so, exit immediately
    if std::env::args().any(|arg| arg == "-h" || arg == "--help") {
        return Ok(());
    }
    
    let mut rotator = LogRotator::new(&args.file, args.size, args.lines, args.count)?;
    
    if args.rotate {
        rotator.rotate()?;
    }

    // Set up SIGHUP handler
    let rotate_flag = Arc::new(AtomicBool::new(false));
    flag::register(signal_hook::consts::SIGHUP, Arc::clone(&rotate_flag))?;

    let stdin = io::stdin();
    let reader = BufReader::new(stdin);

    for line in reader.lines() {
        let line = line?;
        
        if rotate_flag.load(Ordering::Relaxed) {
            rotator.rotate()?;
            rotate_flag.store(false, Ordering::Relaxed);
        }
        
        rotator.write_line(&line)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_test_dir() -> tempfile::TempDir {
        tempdir().expect("Failed to create temp directory")
    }

    fn cleanup_test_files(dir: &tempfile::TempDir, base_name: &str) {
        for i in 0..5 { // Assuming max_count is 5
            let path = dir.path().join(format!("{}.{}", base_name, i + 1));
            if path.exists() {
                fs::remove_file(path).expect("Failed to remove rotated file");
            }
        }
        let current_path = dir.path().join(base_name);
        if current_path.exists() {
            fs::remove_file(current_path).expect("Failed to remove current file");
        }
    }

    #[test]
    fn test_size_based_rotation() -> anyhow::Result<()> {
        let dir = create_test_dir();
        let log_file = dir.path().join("test.log");
        let log_path = log_file.to_str().unwrap();

        let mut rotator = LogRotator::new(log_path, 20, None, 3)?;
        
        // Write a line that should trigger rotation
        rotator.write_line("this is a very long line that exceeds the size limit")?;
        
        // Check that rotation occurred
        assert!(log_file.exists());
        assert!(dir.path().join("test.log.1").exists());
        
        // Verify rotated content - the long line should be in the rotated file
        let rotated_content = fs::read_to_string(dir.path().join("test.log.1"))?;
        assert!(rotated_content.contains("this is a very long line that exceeds the size limit"));
        
        // Current file should be empty or contain minimal content
        let current_content = fs::read_to_string(&log_file)?;
        assert!(current_content.is_empty() || current_content.len() < 20);
        
        cleanup_test_files(&dir, "test.log");
        Ok(())
    }

    #[test]
    fn test_default_behavior_no_rotation() -> anyhow::Result<()> {
        let dir = create_test_dir();
        let log_file = dir.path().join("test.log");
        let log_path = log_file.to_str().unwrap();

        let mut rotator = LogRotator::new(log_path, 0, None, 5)?;
        
        // Write multiple lines - should not rotate
        rotator.write_line("line 1")?;
        rotator.write_line("line 2")?;
        rotator.write_line("line 3")?;
        
        // Check that no rotation occurred
        assert!(log_file.exists());
        assert!(!dir.path().join("test.log.1").exists());
        
        // Verify content
        let content = fs::read_to_string(&log_file)?;
        assert!(content.contains("line 1"));
        assert!(content.contains("line 2"));
        assert!(content.contains("line 3"));
        
        cleanup_test_files(&dir, "test.log");
        Ok(())
    }

    #[test]
    fn test_line_based_rotation() -> anyhow::Result<()> {
        let dir = create_test_dir();
        let log_file = dir.path().join("test.log");
        let log_path = log_file.to_str().unwrap();

        let mut rotator = LogRotator::new(log_path, 0, Some(2), 3)?;
        
        // Write lines to trigger rotation
        rotator.write_line("line 1")?;
        rotator.write_line("line 2")?;
        rotator.write_line("line 3")?;
        
        // Check that rotation occurred
        assert!(log_file.exists());
        assert!(dir.path().join("test.log.1").exists());
        
        // Verify rotated content
        let rotated_content = fs::read_to_string(dir.path().join("test.log.1"))?;
        assert!(rotated_content.contains("line 1"));
        assert!(rotated_content.contains("line 2"));
        
        cleanup_test_files(&dir, "test.log");
        Ok(())
    }

    #[test]
    fn test_combined_size_and_line_limits() -> anyhow::Result<()> {
        let dir = create_test_dir();
        let log_file = dir.path().join("test.log");
        let log_path = log_file.to_str().unwrap();

        let mut rotator = LogRotator::new(log_path, 50, Some(3), 3)?;
        
        // Write a long line that should trigger size-based rotation
        rotator.write_line("this is a very long line that exceeds the size limit")?;
        
        // Check that rotation occurred due to size
        assert!(log_file.exists());
        assert!(dir.path().join("test.log.1").exists());
        
        // Write more lines to test line limit
        rotator.write_line("short line")?;
        rotator.write_line("another short line")?;
        rotator.write_line("third short line")?;
        rotator.write_line("fourth line should trigger rotation")?;
        
        // Check that another rotation occurred due to line count
        assert!(dir.path().join("test.log.2").exists());
        
        cleanup_test_files(&dir, "test.log");
        Ok(())
    }

    #[test]
    fn test_rotation_count_limit() -> anyhow::Result<()> {
        let dir = create_test_dir();
        let log_file = dir.path().join("test.log");
        let log_path = log_file.to_str().unwrap();

        let mut rotator = LogRotator::new(log_path, 10, None, 2)?;
        
        // Trigger multiple rotations
        for i in 1..=5 {
            rotator.write_line(&format!("line {} that exceeds size limit", i))?;
        }
        
        // Check that only 2 rotated files exist (due to count limit)
        assert!(log_file.exists());
        assert!(dir.path().join("test.log.1").exists());
        assert!(dir.path().join("test.log.2").exists());
        assert!(!dir.path().join("test.log.3").exists());
        
        cleanup_test_files(&dir, "test.log");
        Ok(())
    }

    #[test]
    fn test_rotate_on_startup() -> anyhow::Result<()> {
        let dir = create_test_dir();
        let log_file = dir.path().join("test.log");
        let log_path = log_file.to_str().unwrap();

        // Create a file with some content first
        fs::write(&log_file, "existing content\n")?;
        
        let mut rotator = LogRotator::new(log_path, 0, None, 3)?;
        
        // Write a line to populate content
        rotator.write_line("new content")?;
        
        // Rotate on startup
        rotator.rotate()?;
        
        // Check that rotation occurred
        assert!(log_file.exists());
        assert!(dir.path().join("test.log.1").exists());
        
        // Verify rotated content contains new content
        let rotated_content = fs::read_to_string(dir.path().join("test.log.1"))?;
        assert!(rotated_content.contains("new content"));
        
        cleanup_test_files(&dir, "test.log");
        Ok(())
    }

    #[test]
    fn test_existing_file_line_counting() -> anyhow::Result<()> {
        let dir = create_test_dir();
        let log_file = dir.path().join("test.log");
        let log_path = log_file.to_str().unwrap();

        // Create a file with existing content
        fs::write(&log_file, "line 1\nline 2\nline 3\n")?;
        
        let mut rotator = LogRotator::new(log_path, 0, Some(5), 3)?;
        
        // Write one more line - should not trigger rotation yet
        rotator.write_line("line 4")?;
        
        // Check that no rotation occurred
        assert!(log_file.exists());
        assert!(!dir.path().join("test.log.1").exists());
        
        // Write another line - should trigger rotation
        rotator.write_line("line 5")?;
        
        // Check that rotation occurred
        assert!(dir.path().join("test.log.1").exists());
        
        // Verify rotated content contains the lines written during this session
        let rotated_content = fs::read_to_string(dir.path().join("test.log.1"))?;
        assert!(rotated_content.contains("line 4"));
        assert!(rotated_content.contains("line 5"));
        
        cleanup_test_files(&dir, "test.log");
        Ok(())
    }

    #[test]
    fn test_empty_file_creation() -> anyhow::Result<()> {
        let dir = create_test_dir();
        let log_file = dir.path().join("test.log");
        let log_path = log_file.to_str().unwrap();

        let mut rotator = LogRotator::new(log_path, 0, None, 3)?;
        
        // File should be created even if empty
        assert!(log_file.exists());
        
        // Write a line
        rotator.write_line("test line")?;
        
        // Verify content
        let content = fs::read_to_string(&log_file)?;
        assert!(content.contains("test line"));
        
        cleanup_test_files(&dir, "test.log");
        Ok(())
    }
}
