//! Packaging-only executable required by the current runtime-extension packager.
//! All declared contributions are frontend/declarative. No backend operations exist.
use std::io::{self, Read, Write};

fn main() -> io::Result<()> {
    // Drain at most one bounded process_per_call payload; never parse or reflect user input.
    let mut input = Vec::new();
    io::stdin().take(65_537).read_to_end(&mut input)?;
    let message = if input.len() > 65_536 {
        "Frontend-only extension: runtime input exceeds the 65536-byte limit"
    } else {
        "Frontend-only extension: no backend operations are declared; render a frontend contribution"
    };
    // Both messages are fixed ASCII; no user-controlled string enters this JSON response.
    writeln!(io::stdout().lock(), "{{\"ok\":false,\"result\":null,\"error\":{{\"message\":\"{message}\"}}}}")
}
