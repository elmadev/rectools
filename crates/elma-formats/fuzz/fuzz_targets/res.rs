#![no_main]

use elma_formats::res::Res;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for res in [Res::parse(data), Res::parse_across(data)]
        .into_iter()
        .flatten()
    {
        for name in res.names() {
            let _ = res.get(name);
        }
    }
});
