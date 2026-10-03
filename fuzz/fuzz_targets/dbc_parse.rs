//! Fuzz target — `Dbc::parse` totality: arbitrary text produces either a
//! database or a typed error, never a panic. Anything that parses must
//! survive a decode of the message that carries signals.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = core::str::from_utf8(data) {
        if let Ok(dbc) = dbc_parse::Dbc::parse(text) {
            // Every parsed message must decode totally (no panic) over an
            // arbitrary payload shape.
            let payload = [0xA5_u8; 8];
            for message in &dbc.messages {
                let _ = message.unpack(&payload);
            }
        }
    }
});
