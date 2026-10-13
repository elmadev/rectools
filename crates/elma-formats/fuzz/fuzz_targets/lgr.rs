#![no_main]

use elma_formats::lgr::Lgr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = Lgr::parse(data);
});
