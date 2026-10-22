// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Minimal test binary used as a binary-under-test for the dev server.
//!
//! Usage:
//!   sqt-test-binary              # prints "hello world", exits 0
//!   sqt-test-binary --leak N     # leaks N bytes (for valgrind testing)
//!   sqt-test-binary --exit CODE  # exits with CODE
//!   sqt-test-binary --busy MS    # busy-loops for ~MS milliseconds

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut exit_code: i32 = 0;
    let mut leak_bytes: usize = 0;
    let mut busy_ms: u64 = 0;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--exit" => {
                i += 1;
                exit_code = args[i].parse().expect("--exit requires an integer");
            }
            "--leak" => {
                i += 1;
                leak_bytes = args[i].parse().expect("--leak requires an integer");
            }
            "--busy" => {
                i += 1;
                busy_ms = args[i].parse().expect("--busy requires an integer");
            }
            other => {
                eprintln!("unknown flag: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    println!("hello world");

    // Optionally leak memory (detectable by valgrind)
    if leak_bytes > 0 {
        let leaked = vec![0u8; leak_bytes].into_boxed_slice();
        std::mem::forget(leaked);
        eprintln!("leaked {leak_bytes} bytes");
    }

    // Optionally burn CPU for a while
    if busy_ms > 0 {
        let start = std::time::Instant::now();
        let duration = std::time::Duration::from_millis(busy_ms);
        while start.elapsed() < duration {
            std::hint::spin_loop();
        }
        eprintln!("busy-looped for {busy_ms}ms");
    }

    std::process::exit(exit_code);
}
