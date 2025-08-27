# rotatelogs

An on-demand logrotate binary written in Rust that accepts piped stdout and rotates log files based on size or SIGHUP signals.

## Features

- **Piped Input**: Accepts stdout from other processes via pipe
- **Size-based Rotation**: Automatically rotates logs when they exceed a specified size
- **SIGHUP Support**: Immediate rotation when SIGHUP signal is received
- **Configurable**: Set output file, max size, and rotation count
- **Efficient**: Uses buffered I/O for performance

## Usage

```bash
# Basic usage - pipe output to a log file
some_command | rotatelogs --file app.log

# With custom size limit (1MB) and keep 10 rotated files
some_command | rotatelogs --file app.log --size 1048576 --count 10

# Rotate immediately on startup
some_command | rotatelogs --file app.log --rotate

# Force rotation by sending SIGHUP
kill -HUP <pid>
```

## Command Line Options

- `-f, --file <FILE>`: Output log file path (required)
- `-s, --size <SIZE>`: Maximum file size in bytes before rotation (default: 1MB)
- `-c, --count <COUNT>`: Number of rotated files to keep (default: 5)
- `-r, --rotate`: Rotate immediately on startup
- `-h, --help`: Print help information

## Examples

```bash
# Monitor system logs with rotation
journalctl -f | rotatelogs --file system.log --size 5242880 --count 7

# Application logging with immediate rotation
myapp | rotatelogs --file app.log --rotate

# Web server access logs
nginx -g "daemon off;" | rotatelogs --file access.log --size 10485760 --count 10
```

## File Naming

Rotated files follow the pattern:
- Current log: `filename.log`
- Rotated logs: `filename.log.1`, `filename.log.2`, etc.

## Building

```bash
cargo build --release
```

The binary will be available at `target/release/rotatelogs`.
