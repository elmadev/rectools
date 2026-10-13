use crate::Error;
use crate::reader::Reader;

// 16-bit mono PCM samples of a .wav file
pub fn decode_wav(data: &[u8]) -> Result<Vec<i16>, Error> {
    let mut r = Reader::new(data);
    let _riff = r.array::<4>()?;
    let _riff_size = r.i32()?;
    let _wave = r.array::<4>()?;
    // The chunks can come in any order, and ones like LIST are skipped
    let mut format_seen = false;
    loop {
        let id = r.array::<4>()?;
        let size = r.u32()? as usize;
        match &id {
            b"fmt " => {
                if size < 16 {
                    return r.invalid("wav file has a short format chunk");
                }
                let format = r.i16()?;
                let channels = r.i16()?;
                let _sample_rate = r.i32()?;
                let _byte_rate = r.i32()?;
                let block_align = r.i16()?;
                let bits_per_sample = r.i16()?;
                if format != 1 {
                    return r.invalid("wav file is not PCM");
                }
                if channels != 1 {
                    return r.invalid("wav file is not mono");
                }
                if block_align != 2 || bits_per_sample != 16 {
                    return r.invalid("wav file must be plain 16-bit PCM");
                }
                r.bytes(size - 16)?;
                format_seen = true;
            }
            b"data" => {
                if !format_seen {
                    return r.invalid("wav file has no format before its data");
                }
                if !size.is_multiple_of(2) {
                    return r.invalid("wav file has an odd number of data bytes");
                }
                let bytes = r.bytes(size)?;
                return Ok(bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&b| i16::from_le_bytes(b))
                    .collect());
            }
            // Chunks are padded to an even size
            _ => {
                let padded = size.checked_add(size % 2);
                r.bytes(padded.ok_or(Error::Eof { at: r.pos() })?)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        if !body.len().is_multiple_of(2) {
            out.push(0);
        }
        out
    }

    fn format(channels: i16, bits: i16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(1i16.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(22050i32.to_le_bytes());
        out.extend((22050 * 2i32).to_le_bytes());
        out.extend((channels * bits / 8).to_le_bytes());
        out.extend(bits.to_le_bytes());
        out
    }

    fn wav(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = chunks.concat();
        let mut out = b"RIFF".to_vec();
        out.extend((body.len() as i32 + 4).to_le_bytes());
        out.extend(b"WAVE");
        out.extend(body);
        out
    }

    const SAMPLES: [u8; 4] = [1, 0, 0xff, 0xff];

    #[test]
    fn reads_samples() {
        let data = wav(&[chunk(b"fmt ", &format(1, 16)), chunk(b"data", &SAMPLES)]);
        assert_eq!(decode_wav(&data), Ok(vec![1, -1]));
    }

    #[test]
    fn skips_other_chunks() {
        let data = wav(&[
            chunk(b"LIST", b"odd"),
            chunk(b"fmt ", &format(1, 16)),
            chunk(b"fact", &[0; 4]),
            chunk(b"data", &SAMPLES),
        ]);
        assert_eq!(decode_wav(&data), Ok(vec![1, -1]));
    }

    #[test]
    fn reads_an_extended_format_chunk() {
        let mut extended = format(1, 16);
        extended.extend(0u16.to_le_bytes());
        let data = wav(&[chunk(b"fmt ", &extended), chunk(b"data", &SAMPLES)]);
        assert_eq!(decode_wav(&data), Ok(vec![1, -1]));
    }

    #[test]
    fn rejects_stereo_and_8_bit() {
        for fmt in [format(2, 16), format(1, 8)] {
            let data = wav(&[chunk(b"fmt ", &fmt), chunk(b"data", &SAMPLES)]);
            assert!(matches!(decode_wav(&data), Err(Error::Invalid { .. })));
        }
    }

    #[test]
    fn rejects_data_before_format() {
        let data = wav(&[chunk(b"data", &SAMPLES), chunk(b"fmt ", &format(1, 16))]);
        assert!(matches!(decode_wav(&data), Err(Error::Invalid { .. })));
    }

    #[test]
    fn missing_data_is_eof() {
        let data = wav(&[chunk(b"fmt ", &format(1, 16))]);
        assert_eq!(decode_wav(&data), Err(Error::Eof { at: data.len() }));
    }
}
