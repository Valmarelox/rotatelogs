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

    /// Maximum file size in bytes before rotation
    #[arg(short, long, default_value = "1048576")]
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

        // Check if rotation is needed
        let size_exceeded = self.current_size + line_len > self.max_size;
        let lines_exceeded = self.max_lines.map_or(false, |max| self.current_lines + 1 > max);

        if size_exceeded || lines_exceeded {
            self.rotate()?;
        }

        if let Some(ref mut writer) = self.current_file {
            writer.write_all(line_bytes)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            self.current_size += line_len + 1;
            self.current_lines += 1;
        }

        Ok(())
    }

    fn rotate(&mut self) -> anyhow::Result<()> {
        // Flush and close current file
        if let Some(mut writer) = self.current_file.take() {
            writer.flush()?;
            drop(writer);
        }

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

        // Rename current file to .1
        if Path::new(&self.base_path).exists() {
            let rotated_path = format!("{}.1", self.base_path);
            fs::rename(&self.base_path, &rotated_path)?;
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
