//! Checking a release archive and taking the binary out of it.

use std::io::Read;

use anyhow::Context;
use sha2::{Digest, Sha256};

/// The largest binary read out of an archive; a guard against a corrupt or hostile one.
const MAX_BINARY: u64 = 256 * 1024 * 1024;

/// Fails unless the SHA-256 of `archive` is the one in `checksum`, the text of a
/// `sha256sum` file for `archive_name`. Nothing is written before this passes.
pub fn verify_checksum(archive: &[u8], checksum: &str, archive_name: &str) -> anyhow::Result<()> {
    let mut words = checksum.split_whitespace();
    let expected = words.next().unwrap_or_default().to_ascii_lowercase();
    anyhow::ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "the checksum file for {archive_name} does not hold a SHA-256"
    );
    if let Some(named) = words.next() {
        anyhow::ensure!(
            named.trim_start_matches('*') == archive_name,
            "the checksum file is for {named}, not {archive_name}"
        );
    }
    let actual = hex::encode(Sha256::digest(archive));
    anyhow::ensure!(
        actual == expected,
        "the SHA-256 of {archive_name} does not match its .sha256 file; nothing was changed"
    );
    Ok(())
}

/// The `domovoi` executable inside the gzipped tarball `archive`, whose files all sit under
/// the directory `dir`. It is read into memory, never unpacked, so no archive path can
/// reach the disk.
pub fn extract_binary(archive: &[u8], dir: &str) -> anyhow::Result<Vec<u8>> {
    let wanted = format!("{dir}/domovoi");
    let mut tarball = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tarball.entries().context("the archive cannot be read")? {
        let entry = entry.context("the archive cannot be read")?;
        let path = entry.path().context("the archive has an unreadable path")?;
        if entry.header().entry_type().is_file() && path.as_ref() == std::path::Path::new(&wanted) {
            let mut binary = Vec::new();
            entry
                .take(MAX_BINARY + 1)
                .read_to_end(&mut binary)
                .context("the archive cannot be read")?;
            anyhow::ensure!(
                u64::try_from(binary.len()).is_ok_and(|len| len <= MAX_BINARY),
                "the domovoi binary in the archive is implausibly large"
            );
            anyhow::ensure!(
                !binary.is_empty(),
                "the domovoi binary in the archive is empty"
            );
            return Ok(binary);
        }
    }
    anyhow::bail!("the archive has no {wanted}")
}

#[cfg(test)]
mod tests {
    use super::{extract_binary, verify_checksum};
    use sha2::{Digest, Sha256};

    fn tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *bytes).unwrap();
        }
        let tar = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn extracts_the_binary_from_a_release_archive() {
        let archive = tarball(&[
            ("domovoi-v1.0.0-x/README.md", b"readme"),
            ("domovoi-v1.0.0-x/domovoi", b"\x7fELF new binary"),
        ]);
        let binary = extract_binary(&archive, "domovoi-v1.0.0-x").unwrap();
        assert_eq!(binary, b"\x7fELF new binary");
    }

    #[test]
    fn refuses_an_archive_without_the_binary() {
        let archive = tarball(&[("domovoi-v1.0.0-x/README.md", b"readme")]);
        assert!(extract_binary(&archive, "domovoi-v1.0.0-x").is_err());
        let other_dir = tarball(&[("elsewhere/domovoi", b"x")]);
        assert!(extract_binary(&other_dir, "domovoi-v1.0.0-x").is_err());
    }

    #[test]
    fn a_matching_checksum_passes() {
        let archive = b"archive bytes";
        let sum = hex::encode(Sha256::digest(archive));
        let file = format!("{sum}  a.tar.gz\n");
        verify_checksum(archive, &file, "a.tar.gz").unwrap();
        verify_checksum(archive, &sum, "a.tar.gz").unwrap();
    }

    #[test]
    fn a_checksum_mismatch_is_refused() {
        let wrong = hex::encode(Sha256::digest(b"something else"));
        let err = verify_checksum(b"archive bytes", &format!("{wrong}  a.tar.gz"), "a.tar.gz")
            .unwrap_err();
        assert!(err.to_string().contains("does not match"), "{err}");
    }

    #[test]
    fn a_checksum_file_for_another_archive_or_junk_is_refused() {
        let sum = hex::encode(Sha256::digest(b"archive bytes"));
        assert!(
            verify_checksum(b"archive bytes", &format!("{sum}  b.tar.gz"), "a.tar.gz").is_err()
        );
        assert!(verify_checksum(b"archive bytes", "not a checksum", "a.tar.gz").is_err());
        assert!(verify_checksum(b"archive bytes", "", "a.tar.gz").is_err());
    }
}
