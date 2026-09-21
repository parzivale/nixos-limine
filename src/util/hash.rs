use blake2::{Blake2b512, Digest as _};
use std::fmt::Write as _;

/// A running digest, so that a caller can feed it a file without holding the
/// file in memory -- these are kernels, and there is one per generation.
pub(crate) struct Hasher(Blake2b512);

impl Hasher {
    pub(crate) fn new() -> Self {
        Self(Blake2b512::new())
    }

    pub(crate) fn finish(self) -> String {
        hex(self.0.finalize().as_slice())
    }
}

impl std::io::Write for Hasher {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The digest limine expects after the `#` of a `boot():` URI, and the one
/// `limine enroll-config` is handed.
pub(crate) fn blake2b(bytes: &[u8]) -> String {
    hex(Blake2b512::digest(bytes).as_slice())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::blake2b;

    /// Vectors from `b2sum`, which is what limine's checksums have to agree
    /// with.
    #[test]
    fn matches_b2sum() {
        assert!(blake2b(b"").starts_with("786a02f742015903c6c6fd852552d272"));
        assert!(blake2b(b"abc").starts_with("ba80a53f981c4d0d6a2797b69f12f6e9"));
    }

    /// Feeding it in pieces has to match hashing it all at once.
    #[test]
    fn a_streamed_digest_matches_a_one_shot_one() {
        use std::io::Write as _;

        let mut hasher = super::Hasher::new();
        hasher.write_all(b"abc").expect("write");
        hasher.write_all(b"def").expect("write");

        assert_eq!(hasher.finish(), blake2b(b"abcdef"));
    }

    #[test]
    fn is_a_512_bit_digest_in_hex() {
        assert_eq!(blake2b(b"anything").len(), 128);
    }
}
