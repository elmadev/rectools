use crate::reader::{Reader, cstr, xor_decrypt};
use crate::{Error, invalid};

const MAGIC_NUMBER: i32 = 1347839;
const MAX_FILES_LEGACY: usize = 150;
const MAX_FILES: usize = 3000;
const ENTRY_SIZE: usize = 24;
const NAME_SIZE: usize = 16;
const ELMA_KEY: (i16, i16, i16) = (23, 9982, 3391);
// across.res: a different key, and each entry has its offset before its length
const ACROSS_KEY: (i16, i16, i16) = (23, 9782, 3391);

// The encrypted archive (elma.res) holding the game's sounds and internal levels
pub struct Res<'a> {
    data: &'a [u8],
    entries: Vec<(String, usize, usize)>,
}

impl<'a> Res<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Res<'a>, Error> {
        Self::parse_with(data, false)
    }

    // Across's archive of the same shape, across.res
    pub fn parse_across(data: &'a [u8]) -> Result<Res<'a>, Error> {
        Self::parse_with(data, true)
    }

    fn parse_with(data: &'a [u8], across: bool) -> Result<Res<'a>, Error> {
        let magic_at = |files: usize| {
            let offset = 4 + ENTRY_SIZE * files;
            data.get(offset..offset + 4)
                .map(|b| i32::from_le_bytes(b.try_into().unwrap()) == MAGIC_NUMBER)
                .unwrap_or(false)
        };
        let max_files = if magic_at(MAX_FILES_LEGACY) {
            MAX_FILES_LEGACY
        } else if magic_at(MAX_FILES) && !across {
            MAX_FILES
        } else {
            return invalid(".res file is corrupt");
        };

        let mut r = Reader::new(data);
        let file_count = r.i32()?;
        if file_count <= 0 || file_count as usize > max_files {
            return r.invalid(".res file is corrupt");
        }
        let mut table = r.bytes(ENTRY_SIZE * max_files)?.to_vec();
        let (a, b, c) = if across { ACROSS_KEY } else { ELMA_KEY };
        xor_decrypt(&mut table, a, b, c);

        let mut entries = Vec::new();
        for entry in table
            .as_chunks::<ENTRY_SIZE>()
            .0
            .iter()
            .take(file_count as usize)
        {
            let name = cstr(&entry[..NAME_SIZE]);
            let first = i32::from_le_bytes(entry[16..20].try_into().unwrap());
            let second = i32::from_le_bytes(entry[20..24].try_into().unwrap());
            let (offset, length) = if across {
                (first, second)
            } else {
                (second, first)
            };
            if length < 0 || offset < 0 {
                return r.invalid(".res file is corrupt");
            }
            entries.push((name, offset as usize, length as usize));
        }
        Ok(Res { data, entries })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(name, _, _)| name.as_str())
    }

    pub fn get(&self, name: &str) -> Option<&'a [u8]> {
        let (_, offset, length) = self
            .entries
            .iter()
            .find(|(n, _, _)| n.eq_ignore_ascii_case(name))?;
        self.data.get(*offset..offset + length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pack `files` into an across.res, as the game's own tools did
    fn build_across(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, Error> {
        if files.is_empty() || files.len() > MAX_FILES_LEGACY {
            return invalid("across.res holds 1 to 150 files");
        }
        let mut table = vec![0u8; ENTRY_SIZE * MAX_FILES_LEGACY];
        let mut offset = 4 + table.len() + 4;
        for ((name, data), entry) in files.iter().zip(table.chunks_mut(ENTRY_SIZE)) {
            if name.len() >= NAME_SIZE || !name.is_ascii() {
                return invalid(format!("{name} is too long for across.res"));
            }
            entry[..name.len()].copy_from_slice(name.as_bytes());
            entry[16..20].copy_from_slice(&(offset as i32).to_le_bytes());
            entry[20..24].copy_from_slice(&(data.len() as i32).to_le_bytes());
            offset += data.len();
        }
        let (a, b, c) = ACROSS_KEY;
        xor_decrypt(&mut table, a, b, c);

        let mut out = Vec::with_capacity(offset);
        out.extend((files.len() as i32).to_le_bytes());
        out.extend(table);
        out.extend(MAGIC_NUMBER.to_le_bytes());
        for (_, data) in files {
            out.extend(data);
        }
        Ok(out)
    }

    #[test]
    fn across_archives_round_trip() {
        let files = vec![
            ("leir1.lei".to_string(), b"hello".to_vec()),
            ("4_EG.pcx".to_string(), vec![1, 2, 3]),
        ];
        let data = build_across(&files).unwrap();
        let res = Res::parse_across(&data).unwrap();
        assert_eq!(res.names().collect::<Vec<_>>(), ["leir1.lei", "4_EG.pcx"]);
        assert_eq!(res.get("4_eg.PCX"), Some(&[1u8, 2, 3][..]));
        assert!(
            Res::parse(&data).is_err() || Res::parse(&data).unwrap().get("leir1.lei").is_none()
        );
    }
}
