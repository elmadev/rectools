#![no_main]

use elma_formats::across::{self, DEFAULT_OBJECT_KINDS};
use elma_formats::lev::Level;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = Level::parse(data, false);
    let _ = Level::parse(data, true);
    let _ = across::level_from_lev(data);
    let _ = across::txt::parse(data, &DEFAULT_OBJECT_KINDS);
    let _ = across::gfx::object_types(data);
});
