use bytes::{BufMut, Bytes, BytesMut};

const SEED_FILE_PREFIX: &str = "kytes encrypted-seed";
const VERSION: u8 = 0;

/// Takes an encrypted seed and format it into a seed file as follows:
/// `kytes encrypted-seed v<version> <zbase32 encoded encrypted_seed>`
pub fn format_encrypted_seed_file(encrypted_seed: &[u8; 32]) -> Bytes {
    let mut seed_file = BytesMut::with_capacity(SEED_FILE_PREFIX.len() + 33);
    seed_file.extend_from_slice(SEED_FILE_PREFIX.as_bytes());
    seed_file.extend_from_slice(b" v");
    seed_file.put_u8(VERSION + 48);
    seed_file.extend_from_slice(b" ");
    seed_file.extend_from_slice(z32::encode(encrypted_seed).as_bytes());

    seed_file.freeze()
}

pub fn encrypted_seed_file_version(seed_file: &Bytes) -> Option<u8> {
    let version_start = SEED_FILE_PREFIX.len() + 2;
    let version_end = version_start + 2;

    seed_file
        .get(version_start..version_end)
        .map(|version| version[0] - 48_u8)
}

pub fn encrypt_seed_file() {
    unimplemented!()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_format_encrypted_seed_file() {
        let seed = [0u8; 32];
        let seed_file = format_encrypted_seed_file(&seed);

        dbg!(&seed_file);

        assert_eq!(seed_file.len(), 52 + 4 + SEED_FILE_PREFIX.len());
        assert!(seed_file.starts_with(SEED_FILE_PREFIX.as_bytes()));
        assert!(seed_file.starts_with(SEED_FILE_PREFIX.as_bytes()));
        assert_eq!(encrypted_seed_file_version(&seed_file).unwrap(), 0);
        assert!(seed_file.ends_with(&z32::encode(&seed).as_bytes()));
    }
}
