#![no_main]

use elma_formats::wav::decode_wav;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = decode_wav(data);
});
