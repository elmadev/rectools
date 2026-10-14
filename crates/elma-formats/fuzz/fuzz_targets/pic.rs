#![no_main]

use elma_formats::abc::decode_abc;
use elma_formats::pic::{decode_bmp, decode_pcx, decode_shirt, decode_spr, pcx_palette};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = decode_pcx(data);
    let _ = pcx_palette(data);
    let _ = decode_spr(data);
    let _ = decode_bmp(data);
    let _ = decode_shirt(data);
    let _ = decode_abc(data);
});
