use crate::Error;

pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    // Where the last field read starts, which is what an error about it points at
    last: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            last: 0,
        }
    }

    pub fn seek(&mut self, pos: usize) -> Result<(), Error> {
        if pos > self.data.len() {
            return Err(Error::Eof { at: pos });
        }
        self.pos = pos;
        Ok(())
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    // Fails unless `count` items of `size` bytes are left, so a count read from the file
    // can't make room for more than the file holds
    pub fn has_items(&self, count: usize, size: usize) -> Result<(), Error> {
        match count.checked_mul(size) {
            Some(n) if n <= self.remaining() => Ok(()),
            _ => Err(self.eof()),
        }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    fn eof(&self) -> Error {
        Error::Eof { at: self.pos }
    }

    // An error about the field just read, at its first byte
    pub fn invalid<T>(&self, message: impl Into<String>) -> Result<T, Error> {
        Err(Error::Invalid {
            message: message.into(),
            at: Some(self.last),
        })
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).ok_or_else(|| self.eof())?;
        let slice = self.data.get(self.pos..end).ok_or_else(|| self.eof())?;
        self.last = self.pos;
        self.pos = end;
        Ok(slice)
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        Ok(self.bytes(N)?.try_into().unwrap())
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.array::<1>()?[0])
    }

    pub fn i16(&mut self) -> Result<i16, Error> {
        Ok(i16::from_le_bytes(self.array()?))
    }

    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    pub fn f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    // A NUL-padded fixed-size string field
    pub fn cstr(&mut self, n: usize) -> Result<String, Error> {
        Ok(cstr(self.bytes(n)?))
    }
}

pub(crate) fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes[..end].iter().map(|&b| b as char).collect()
}

// The game's stream cipher for elma.res and the levels' best times
pub(crate) fn xor_decrypt(data: &mut [u8], mut a: i16, mut b: i16, c: i16) {
    for byte in data.iter_mut() {
        *byte ^= a as u8;
        a %= c;
        b = b.wrapping_add(a.wrapping_mul(c));
        a = 31i16.wrapping_mul(b).wrapping_add(c);
    }
}
