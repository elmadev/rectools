#![no_main]

use elma_formats::across::rec::{AcrossRec, is_across_rec};
use elma_formats::rec::Rec;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = Rec::parse(data);
    let _ = is_across_rec(data);
    let _ = AcrossRec::parse(data);
});
