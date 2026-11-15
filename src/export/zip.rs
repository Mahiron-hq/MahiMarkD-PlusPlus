//! Minimal ZIP writer for Office packages.

use std::io::Write;

use flate2::Compression;
use flate2::write::DeflateEncoder;

#[derive(Debug)]
struct Entry {
    name: String,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
    method: u16,
}

#[derive(Debug, Default)]
pub struct ZipWriter {
    buf: Vec<u8>,
    entries: Vec<Entry>,
}

const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = (1 << 5) | 1;
const FLAG_UTF8: u16 = 0x0800;

fn u16le(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn u32le(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

impl ZipWriter {
    pub fn add(&mut self, name: &str, data: &[u8], compress: bool) -> std::io::Result<()> {
        let too_big = || std::io::Error::other("file too large for the document package");
        let crc = crc32fast::hash(data);
        let (method, body) = if compress {
            let mut enc = DeflateEncoder::new(Vec::with_capacity(data.len() / 3), Compression::default());
            enc.write_all(data)?;
            (8u16, enc.finish()?)
        } else {
            (0u16, data.to_vec())
        };
        let offset = u32::try_from(self.buf.len()).map_err(|_| too_big())?;
        let compressed = u32::try_from(body.len()).map_err(|_| too_big())?;
        let size = u32::try_from(data.len()).map_err(|_| too_big())?;
        let b = &mut self.buf;
        u32le(b, 0x0403_4b50);
        u16le(b, 20);
        u16le(b, FLAG_UTF8);
        u16le(b, method);
        u16le(b, DOS_TIME);
        u16le(b, DOS_DATE);
        u32le(b, crc);
        u32le(b, compressed);
        u32le(b, size);
        u16le(b, name.len() as u16);
        u16le(b, 0);
        b.extend_from_slice(name.as_bytes());
        b.extend_from_slice(&body);
        self.entries.push(Entry { name: name.to_string(), crc, compressed, size, offset, method });
        Ok(())
    }

    pub fn finish(mut self) -> Vec<u8> {
        let cd_start = self.buf.len() as u32;
        let b = &mut self.buf;
        for e in &self.entries {
            u32le(b, 0x0201_4b50);
            u16le(b, 20);
            u16le(b, 20);
            u16le(b, FLAG_UTF8);
            u16le(b, e.method);
            u16le(b, DOS_TIME);
            u16le(b, DOS_DATE);
            u32le(b, e.crc);
            u32le(b, e.compressed);
            u32le(b, e.size);
            u16le(b, e.name.len() as u16);
            u16le(b, 0);
            u16le(b, 0);
            u16le(b, 0);
            u16le(b, 0);
            u32le(b, 0);
            u32le(b, e.offset);
            b.extend_from_slice(e.name.as_bytes());
        }
        let cd_size = b.len() as u32 - cd_start;
        u32le(b, 0x0605_4b50);
        u16le(b, 0);
        u16le(b, 0);
        u16le(b, self.entries.len() as u16);
        u16le(b, self.entries.len() as u16);
        u32le(b, cd_size);
        u32le(b, cd_start);
        u16le(b, 0);
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::DeflateDecoder;
    use std::io::Read;

    fn rd16(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }

    fn rd32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    #[test]
    fn writes_readable_archive() {
        let mut z = ZipWriter::default();
        let text = "hello ".repeat(100);
        z.add("a/b.xml", text.as_bytes(), true).unwrap();
        z.add("raw.bin", &[1, 2, 3], false).unwrap();
        let data = z.finish();

        let eocd = data.len() - 22;
        assert_eq!(rd32(&data, eocd), 0x0605_4b50);
        assert_eq!(rd16(&data, eocd + 10), 2);
        let cd = rd32(&data, eocd + 16) as usize;
        assert_eq!(rd32(&data, cd), 0x0201_4b50);

        assert_eq!(rd32(&data, 0), 0x0403_4b50);
        let name_len = rd16(&data, 26) as usize;
        let csize = rd32(&data, 18) as usize;
        let body = &data[30 + name_len..30 + name_len + csize];
        let mut out = String::new();
        DeflateDecoder::new(body).read_to_string(&mut out).unwrap();
        assert_eq!(out, text);
        assert_eq!(rd32(&data, 14), crc32fast::hash(text.as_bytes()));
    }
}
