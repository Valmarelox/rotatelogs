# rotatelogs

An on-demand logrotate binary written in Rust that accepts piped stdout and rotates log files based on size, lines, or SIGHUP signals.

## Features

- **Piped Input**: Accepts stdout from other processes via pipe
- **Signal-based Rotation**: Default behavior - rotates only when SIGHUP signal is received
- **Size-based Rotation**: Automatically rotates logs when they exceed a specified size (optional)
- **Line-based Rotation**: Automatically rotates logs when they exceed a specified number of lines (optional)
- **SIGHUP Support**: Immediate rotation when SIGHUP signal is received
- **Configurable**: Set output file, max size, max lines, and rotation count
- **Efficient**: Uses buffered I/O for performance

## Usage

```bash
# Default behavior - only rotate on SIGHUP signal
some_command | rotatelogs --file app.log

# With custom size limit (1MB) and keep 10 rotated files
some_command | rotatelogs --file app.log --size 1048576 --count 10

# With line limit (1000 lines) and keep 5 rotated files
some_command | rotatelogs --file app.log --lines 1000 --count 5

# Combined size and line limits (whichever comes first)
some_command | rotatelogs --file app.log --size 1048576 --lines 1000 --count 5

# Rotate immediately on startup
some_command | rotatelogs --file app.log --rotate

# Force rotation by sending SIGHUP
kill -HUP <pid>
```

## Command Line Options

- `-f, --file <FILE>`: Output log file path (required)
- `-s, --size <SIZE>`: Maximum file size in bytes before rotation (default: 0 = no automatic rotation)
- `-l, --lines <LINES>`: Maximum number of lines before rotation (default: no limit)
- `-c, --count <COUNT>`: Number of rotated files to keep (default: 5)
- `-r, --rotate`: Rotate immediately on startup
- `-h, --help`: Print help information

## Examples

```bash
# Monitor system logs with signal-only rotation
journalctl -f | rotatelogs --file system.log --count 7

# Application logging with line-based rotation
myapp | rotatelogs --file app.log --lines 10000 --count 10

# Web server access logs with combined limits
nginx -g "daemon off;" | rotatelogs --file access.log --size 10485760 --lines 50000 --count 10

# High-frequency logging with small line limits
debug_app | rotatelogs --file debug.log --lines 100 --count 20

# Simple logging with manual rotation control
simple_app | rotatelogs --file simple.log
```

## File Naming

Rotated files follow the pattern:
- Current log: `filename.log`
- Rotated logs: `filename.log.1`, `filename.log.2`, etc.

## Rotation Logic

Rotation occurs when **any** of these conditions are met:
- SIGHUP signal is received (default behavior)
- File size exceeds the `--size` limit (when > 0)
- Number of lines exceeds the `--lines` limit (when specified)
- `--rotate` flag is used on startup

**Note**: By default, no automatic rotation occurs. Logs accumulate indefinitely until manually rotated via SIGHUP or the `--rotate` flag.

## Building

```bash
cargo build --release
```

The binary will be available at `target/release/rotatelogs`.
