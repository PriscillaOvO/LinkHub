use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::protocol::{encode_hex, sanitize_field};

pub(super) const FILE_CHUNK_SIZE: usize = 4096;

pub(super) struct FileReceiveState {
    pub(super) transfer_id: String,
    pub(super) filename: String,
    pub(super) size_bytes: u64,
    pub(super) expected_sha256_hex: Option<String>,
    pub(super) received_bytes: u64,
    pub(super) next_chunk_index: u64,
    pub(super) final_path: PathBuf,
    pub(super) temp_path: PathBuf,
    pub(super) metadata_path: PathBuf,
    pub(super) file: File,
}

#[derive(Debug, Eq, PartialEq)]
struct ReceiveProgressMetadata {
    transfer_id: String,
    filename: String,
    size_bytes: u64,
    expected_sha256_hex: Option<String>,
    received_bytes: u64,
    next_chunk_index: u64,
    partial_sha256_hex: String,
    temp_path: PathBuf,
    final_path: PathBuf,
}

struct ReceiveProgressMatch<'a> {
    transfer_id: &'a str,
    filename: &'a str,
    size_bytes: u64,
    expected_sha256_hex: Option<&'a str>,
    temp_path: &'a Path,
    final_path: &'a Path,
    temp_file_len: u64,
    temp_sha256_hex: &'a str,
}

impl FileReceiveState {
    pub(super) fn write_progress_metadata(&self) -> io::Result<()> {
        let partial_sha256_hex = file_sha256_hex(&self.temp_path)?;

        fs::write(
            &self.metadata_path,
            format_receive_progress_metadata(&ReceiveProgressMetadata {
                transfer_id: self.transfer_id.clone(),
                filename: self.filename.clone(),
                size_bytes: self.size_bytes,
                expected_sha256_hex: self.expected_sha256_hex.clone(),
                received_bytes: self.received_bytes,
                next_chunk_index: self.next_chunk_index,
                partial_sha256_hex,
                temp_path: self.temp_path.clone(),
                final_path: self.final_path.clone(),
            }),
        )
    }
}

pub(super) fn file_transfer_id(
    device_id: &str,
    filename: &str,
    size_bytes: u64,
    sha256_hex: &str,
) -> String {
    let hash_prefix = sha256_hex.get(..16).unwrap_or(sha256_hex);

    sanitize_field(&format!(
        "{}-{}-{}-{}",
        device_id,
        sanitize_path_component(filename),
        size_bytes,
        hash_prefix
    ))
}

pub(super) fn file_chunk_ack_id(transfer_id: &str, chunk_index: u64) -> String {
    format!("{transfer_id}:{chunk_index}")
}

pub(super) fn file_start_ack_status(include_resume_chunk: bool, resume_from_chunk: u64) -> String {
    if include_resume_chunk {
        format!("FILE_START_RECEIVED:{resume_from_chunk}")
    } else {
        "FILE_START_RECEIVED".to_string()
    }
}

pub(super) fn received_file_path(
    receive_dir: impl AsRef<Path>,
    transfer_id: &str,
    filename: &str,
) -> PathBuf {
    receive_dir.as_ref().join(format!(
        "{}_{}",
        sanitize_path_component(transfer_id),
        sanitize_path_component(filename)
    ))
}

pub(super) fn partial_file_path(final_path: impl AsRef<Path>) -> PathBuf {
    let final_path = final_path.as_ref();
    let mut filename = final_path.file_name().unwrap_or_default().to_os_string();
    filename.push(".part");

    final_path.with_file_name(filename)
}

pub(super) fn receive_metadata_path(temp_path: impl AsRef<Path>) -> PathBuf {
    let temp_path = temp_path.as_ref();
    let mut filename = temp_path.file_name().unwrap_or_default().to_os_string();
    filename.push(".meta");

    temp_path.with_file_name(filename)
}

fn format_receive_progress_metadata(metadata: &ReceiveProgressMetadata) -> String {
    [
        format!("transfer_id={}", metadata.transfer_id),
        format!("filename={}", metadata.filename),
        format!("size_bytes={}", metadata.size_bytes),
        format!(
            "expected_sha256_hex={}",
            metadata
                .expected_sha256_hex
                .as_deref()
                .unwrap_or("not-provided")
        ),
        format!("received_bytes={}", metadata.received_bytes),
        format!("next_chunk_index={}", metadata.next_chunk_index),
        format!("partial_sha256_hex={}", metadata.partial_sha256_hex),
        format!("temp_path={}", metadata.temp_path.display()),
        format!("final_path={}", metadata.final_path.display()),
    ]
    .join("\n")
}

fn parse_receive_progress_metadata(value: &str) -> Result<ReceiveProgressMetadata, String> {
    let mut fields = HashMap::new();

    for line in value.lines() {
        let Some((key, field_value)) = line.split_once('=') else {
            return Err(format!("invalid metadata line: {line}"));
        };

        fields.insert(key, field_value);
    }

    let expected_sha256_hex = required_metadata_field(&fields, "expected_sha256_hex")?;
    let expected_sha256_hex = match expected_sha256_hex {
        "not-provided" => None,
        value => Some(value.to_string()),
    };

    Ok(ReceiveProgressMetadata {
        transfer_id: required_metadata_field(&fields, "transfer_id")?.to_string(),
        filename: required_metadata_field(&fields, "filename")?.to_string(),
        size_bytes: parse_metadata_u64(&fields, "size_bytes")?,
        expected_sha256_hex,
        received_bytes: parse_metadata_u64(&fields, "received_bytes")?,
        next_chunk_index: parse_metadata_u64(&fields, "next_chunk_index")?,
        partial_sha256_hex: required_metadata_field(&fields, "partial_sha256_hex")?.to_string(),
        temp_path: PathBuf::from(required_metadata_field(&fields, "temp_path")?),
        final_path: PathBuf::from(required_metadata_field(&fields, "final_path")?),
    })
}

fn read_receive_progress_metadata(path: impl AsRef<Path>) -> io::Result<ReceiveProgressMetadata> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)?;

    parse_receive_progress_metadata(&content)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub(super) fn reusable_receive_progress_metadata(
    metadata_path: impl AsRef<Path>,
    transfer_id: &str,
    filename: &str,
    size_bytes: u64,
    expected_sha256_hex: Option<&str>,
    temp_path: impl AsRef<Path>,
    final_path: impl AsRef<Path>,
) -> Option<(u64, u64)> {
    let metadata_path = metadata_path.as_ref();
    let temp_path = temp_path.as_ref();
    let final_path = final_path.as_ref();

    if !metadata_path.exists() || !temp_path.exists() {
        return None;
    }

    let temp_file_len = match fs::metadata(temp_path) {
        Ok(metadata) => metadata.len(),
        Err(err) => {
            eprintln!(
                "Ignored unreadable partial receive file at {}: {err}",
                temp_path.display()
            );
            return None;
        }
    };
    let temp_sha256_hex = match file_sha256_hex(temp_path) {
        Ok(hash) => hash,
        Err(err) => {
            eprintln!(
                "Ignored unreadable partial receive file at {}: {err}",
                temp_path.display()
            );
            return None;
        }
    };

    match read_receive_progress_metadata(metadata_path) {
        Ok(metadata)
            if metadata_matches_transfer(
                &metadata,
                &ReceiveProgressMatch {
                    transfer_id,
                    filename,
                    size_bytes,
                    expected_sha256_hex,
                    temp_path,
                    final_path,
                    temp_file_len,
                    temp_sha256_hex: &temp_sha256_hex,
                },
            ) =>
        {
            Some((metadata.received_bytes, metadata.next_chunk_index))
        }
        Ok(metadata) => {
            eprintln!(
                "Ignored partial receive metadata for unmatched transfer: {}",
                metadata.transfer_id
            );
            None
        }
        Err(err) => {
            eprintln!(
                "Ignored unreadable partial receive metadata at {}: {err}",
                metadata_path.display()
            );
            None
        }
    }
}

fn metadata_matches_transfer(
    metadata: &ReceiveProgressMetadata,
    expected: &ReceiveProgressMatch<'_>,
) -> bool {
    metadata.transfer_id == expected.transfer_id
        && metadata.filename == expected.filename
        && metadata.size_bytes == expected.size_bytes
        && metadata.expected_sha256_hex.as_deref() == expected.expected_sha256_hex
        && metadata.temp_path == expected.temp_path
        && metadata.final_path == expected.final_path
        && metadata.received_bytes <= metadata.size_bytes
        && metadata.received_bytes == expected.temp_file_len
        && metadata.partial_sha256_hex == expected.temp_sha256_hex
        && expected_next_chunk_index(metadata.size_bytes, metadata.received_bytes)
            == Some(metadata.next_chunk_index)
}

fn expected_next_chunk_index(size_bytes: u64, received_bytes: u64) -> Option<u64> {
    if received_bytes > size_bytes {
        return None;
    }

    if received_bytes == size_bytes {
        return Some(received_bytes.div_ceil(FILE_CHUNK_SIZE as u64));
    }

    if received_bytes.is_multiple_of(FILE_CHUNK_SIZE as u64) {
        return Some(received_bytes / FILE_CHUNK_SIZE as u64);
    }

    None
}

pub(super) fn received_bytes_after_chunk(
    received_bytes: u64,
    chunk_len: usize,
    size_bytes: u64,
) -> Option<u64> {
    let next_received_bytes = received_bytes.checked_add(chunk_len as u64)?;

    if next_received_bytes <= size_bytes {
        Some(next_received_bytes)
    } else {
        None
    }
}

fn required_metadata_field<'a>(
    fields: &'a HashMap<&str, &'a str>,
    key: &str,
) -> Result<&'a str, String> {
    fields
        .get(key)
        .copied()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing metadata field: {key}"))
}

fn parse_metadata_u64(fields: &HashMap<&str, &str>, key: &str) -> Result<u64, String> {
    let value = required_metadata_field(fields, key)?;

    value
        .parse()
        .map_err(|_| format!("invalid metadata field {key}: {value}"))
}

pub(super) fn file_sha256_hex(path: impl AsRef<Path>) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; FILE_CHUNK_SIZE];

    loop {
        let bytes_read = file.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(encode_hex(&hasher.finalize()))
}

fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\0'..='\u{1f}' => '_',
            _ => ch,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_transfer_id_is_stable_for_same_file_identity() {
        let transfer_id = file_transfer_id(
            "phone-001",
            "notes.txt",
            42,
            "abcdef1234567890abcdef1234567890",
        );

        assert_eq!(transfer_id, "phone-001-notes.txt-42-abcdef1234567890");
    }

    #[test]
    fn file_chunk_ack_id_includes_transfer_and_index() {
        assert_eq!(file_chunk_ack_id("phone-001-100", 2), "phone-001-100:2");
    }

    #[test]
    fn file_start_ack_status_can_include_resume_chunk() {
        assert_eq!(
            file_start_ack_status(true, 3),
            "FILE_START_RECEIVED:3".to_string()
        );
        assert_eq!(
            file_start_ack_status(false, 3),
            "FILE_START_RECEIVED".to_string()
        );
    }

    #[test]
    fn sha256_hashes_file_content() {
        let path = std::env::temp_dir().join("linkhub-sha256-test-file-transfer.txt");
        fs::write(&path, b"hello").unwrap();

        let hash = file_sha256_hex(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn received_file_path_sanitizes_unsafe_components() {
        let path = received_file_path("received", "phone:001", "..\\secret.txt");

        assert_eq!(path, Path::new("received").join("phone_001_.._secret.txt"));
    }

    #[test]
    fn partial_file_path_appends_part_suffix() {
        let path = partial_file_path(Path::new("received").join("phone-001_notes.txt"));

        assert_eq!(path, Path::new("received").join("phone-001_notes.txt.part"));
    }

    #[test]
    fn receive_metadata_path_appends_meta_suffix() {
        let path = receive_metadata_path(Path::new("received").join("phone-001_notes.txt.part"));

        assert_eq!(
            path,
            Path::new("received").join("phone-001_notes.txt.part.meta")
        );
    }

    #[test]
    fn receive_progress_metadata_records_resume_fields() {
        let temp_path = Path::new("received").join("phone-001_notes.txt.part");
        let final_path = Path::new("received").join("phone-001_notes.txt");
        let metadata = format_receive_progress_metadata(&ReceiveProgressMetadata {
            transfer_id: "phone-001-100".to_string(),
            filename: "notes.txt".to_string(),
            size_bytes: 42,
            expected_sha256_hex: Some("abc123".to_string()),
            received_bytes: 16,
            next_chunk_index: 4,
            partial_sha256_hex: "partialabc".to_string(),
            temp_path,
            final_path,
        });

        assert!(metadata.contains("transfer_id=phone-001-100"));
        assert!(metadata.contains("filename=notes.txt"));
        assert!(metadata.contains("size_bytes=42"));
        assert!(metadata.contains("expected_sha256_hex=abc123"));
        assert!(metadata.contains("received_bytes=16"));
        assert!(metadata.contains("next_chunk_index=4"));
        assert!(metadata.contains("partial_sha256_hex=partialabc"));
        assert!(metadata.contains("temp_path="));
        assert!(metadata.contains("final_path="));
    }

    #[test]
    fn receive_progress_metadata_round_trips() {
        let original = ReceiveProgressMetadata {
            transfer_id: "phone-001-100".to_string(),
            filename: "notes.txt".to_string(),
            size_bytes: 42,
            expected_sha256_hex: Some("abc123".to_string()),
            received_bytes: 16,
            next_chunk_index: 4,
            partial_sha256_hex: "partialabc".to_string(),
            temp_path: Path::new("received").join("phone-001_notes.txt.part"),
            final_path: Path::new("received").join("phone-001_notes.txt"),
        };

        let serialized = format_receive_progress_metadata(&original);
        let parsed = parse_receive_progress_metadata(&serialized).unwrap();

        assert_eq!(parsed, original);
    }

    #[test]
    fn receive_progress_metadata_reads_from_file() {
        let path = std::env::temp_dir().join("linkhub-receive-metadata-file-transfer-test.meta");
        fs::write(
            &path,
            "transfer_id=phone-001-100\nfilename=notes.txt\nsize_bytes=42\nexpected_sha256_hex=abc123\nreceived_bytes=16\nnext_chunk_index=4\npartial_sha256_hex=partialabc\ntemp_path=received\\notes.txt.part\nfinal_path=received\\notes.txt",
        )
        .unwrap();

        let metadata = read_receive_progress_metadata(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(metadata.transfer_id, "phone-001-100");
        assert_eq!(metadata.received_bytes, 16);
        assert_eq!(metadata.next_chunk_index, 4);
    }

    #[test]
    fn receive_progress_metadata_parses_missing_hash() {
        let parsed = parse_receive_progress_metadata(
            "transfer_id=phone-001-100\nfilename=notes.txt\nsize_bytes=42\nexpected_sha256_hex=not-provided\nreceived_bytes=16\nnext_chunk_index=4\npartial_sha256_hex=partialabc\ntemp_path=received\\notes.txt.part\nfinal_path=received\\notes.txt",
        )
        .unwrap();

        assert_eq!(parsed.expected_sha256_hex, None);
    }

    #[test]
    fn receive_progress_metadata_rejects_missing_required_fields() {
        let err = parse_receive_progress_metadata("transfer_id=phone-001-100").unwrap_err();

        assert!(err.contains("missing metadata field"));
    }

    #[test]
    fn receive_progress_metadata_rejects_invalid_numbers() {
        let err = parse_receive_progress_metadata(
            "transfer_id=phone-001-100\nfilename=notes.txt\nsize_bytes=bad\nexpected_sha256_hex=abc123\nreceived_bytes=16\nnext_chunk_index=4\npartial_sha256_hex=partialabc\ntemp_path=received\\notes.txt.part\nfinal_path=received\\notes.txt",
        )
        .unwrap_err();

        assert!(err.contains("invalid metadata field size_bytes"));
    }

    #[test]
    fn metadata_matches_transfer_when_resume_fields_are_consistent() {
        let temp_path = Path::new("received").join("notes.txt.part");
        let final_path = Path::new("received").join("notes.txt");
        let metadata = ReceiveProgressMetadata {
            transfer_id: "phone-001-notes.txt-8192-abc123".to_string(),
            filename: "notes.txt".to_string(),
            size_bytes: 8192,
            expected_sha256_hex: Some("abc123".to_string()),
            received_bytes: FILE_CHUNK_SIZE as u64,
            next_chunk_index: 1,
            partial_sha256_hex: "partialabc".to_string(),
            temp_path: temp_path.clone(),
            final_path: final_path.clone(),
        };

        assert!(metadata_matches_transfer(
            &metadata,
            &ReceiveProgressMatch {
                transfer_id: "phone-001-notes.txt-8192-abc123",
                filename: "notes.txt",
                size_bytes: 8192,
                expected_sha256_hex: Some("abc123"),
                temp_path: &temp_path,
                final_path: &final_path,
                temp_file_len: FILE_CHUNK_SIZE as u64,
                temp_sha256_hex: "partialabc",
            },
        ));
    }

    #[test]
    fn metadata_rejects_inconsistent_next_chunk() {
        let temp_path = Path::new("received").join("notes.txt.part");
        let final_path = Path::new("received").join("notes.txt");
        let metadata = ReceiveProgressMetadata {
            transfer_id: "phone-001-notes.txt-8192-abc123".to_string(),
            filename: "notes.txt".to_string(),
            size_bytes: 8192,
            expected_sha256_hex: Some("abc123".to_string()),
            received_bytes: FILE_CHUNK_SIZE as u64,
            next_chunk_index: 2,
            partial_sha256_hex: "partialabc".to_string(),
            temp_path: temp_path.clone(),
            final_path: final_path.clone(),
        };

        assert!(!metadata_matches_transfer(
            &metadata,
            &ReceiveProgressMatch {
                transfer_id: "phone-001-notes.txt-8192-abc123",
                filename: "notes.txt",
                size_bytes: 8192,
                expected_sha256_hex: Some("abc123"),
                temp_path: &temp_path,
                final_path: &final_path,
                temp_file_len: FILE_CHUNK_SIZE as u64,
                temp_sha256_hex: "partialabc",
            },
        ));
    }

    #[test]
    fn metadata_rejects_mismatched_part_file_length() {
        let temp_path = Path::new("received").join("notes.txt.part");
        let final_path = Path::new("received").join("notes.txt");
        let metadata = ReceiveProgressMetadata {
            transfer_id: "phone-001-notes.txt-8192-abc123".to_string(),
            filename: "notes.txt".to_string(),
            size_bytes: 8192,
            expected_sha256_hex: Some("abc123".to_string()),
            received_bytes: FILE_CHUNK_SIZE as u64,
            next_chunk_index: 1,
            partial_sha256_hex: "partialabc".to_string(),
            temp_path: temp_path.clone(),
            final_path: final_path.clone(),
        };

        assert!(!metadata_matches_transfer(
            &metadata,
            &ReceiveProgressMatch {
                transfer_id: "phone-001-notes.txt-8192-abc123",
                filename: "notes.txt",
                size_bytes: 8192,
                expected_sha256_hex: Some("abc123"),
                temp_path: &temp_path,
                final_path: &final_path,
                temp_file_len: FILE_CHUNK_SIZE as u64 - 1,
                temp_sha256_hex: "partialabc",
            },
        ));
    }

    #[test]
    fn metadata_rejects_partial_non_final_chunk() {
        let temp_path = Path::new("received").join("notes.txt.part");
        let final_path = Path::new("received").join("notes.txt");
        let metadata = ReceiveProgressMetadata {
            transfer_id: "phone-001-notes.txt-8192-abc123".to_string(),
            filename: "notes.txt".to_string(),
            size_bytes: 8192,
            expected_sha256_hex: Some("abc123".to_string()),
            received_bytes: 1,
            next_chunk_index: 1,
            partial_sha256_hex: "partialabc".to_string(),
            temp_path: temp_path.clone(),
            final_path: final_path.clone(),
        };

        assert!(!metadata_matches_transfer(
            &metadata,
            &ReceiveProgressMatch {
                transfer_id: "phone-001-notes.txt-8192-abc123",
                filename: "notes.txt",
                size_bytes: 8192,
                expected_sha256_hex: Some("abc123"),
                temp_path: &temp_path,
                final_path: &final_path,
                temp_file_len: 1,
                temp_sha256_hex: "partialabc",
            },
        ));
    }

    #[test]
    fn metadata_rejects_mismatched_partial_hash() {
        let temp_path = Path::new("received").join("notes.txt.part");
        let final_path = Path::new("received").join("notes.txt");
        let metadata = ReceiveProgressMetadata {
            transfer_id: "phone-001-notes.txt-8192-abc123".to_string(),
            filename: "notes.txt".to_string(),
            size_bytes: 8192,
            expected_sha256_hex: Some("abc123".to_string()),
            received_bytes: FILE_CHUNK_SIZE as u64,
            next_chunk_index: 1,
            partial_sha256_hex: "partialabc".to_string(),
            temp_path: temp_path.clone(),
            final_path: final_path.clone(),
        };

        assert!(!metadata_matches_transfer(
            &metadata,
            &ReceiveProgressMatch {
                transfer_id: "phone-001-notes.txt-8192-abc123",
                filename: "notes.txt",
                size_bytes: 8192,
                expected_sha256_hex: Some("abc123"),
                temp_path: &temp_path,
                final_path: &final_path,
                temp_file_len: FILE_CHUNK_SIZE as u64,
                temp_sha256_hex: "different",
            },
        ));
    }

    #[test]
    fn expected_next_chunk_index_accepts_complete_chunks_or_complete_file() {
        assert_eq!(expected_next_chunk_index(8192, 0), Some(0));
        assert_eq!(
            expected_next_chunk_index(8192, FILE_CHUNK_SIZE as u64),
            Some(1)
        );
        assert_eq!(expected_next_chunk_index(5000, 5000), Some(2));
        assert_eq!(expected_next_chunk_index(8192, 1), None);
        assert_eq!(expected_next_chunk_index(8192, 8193), None);
    }

    #[test]
    fn received_bytes_after_chunk_rejects_overflow_and_oversized_chunks() {
        assert_eq!(received_bytes_after_chunk(4096, 904, 5000), Some(5000));
        assert_eq!(received_bytes_after_chunk(4096, 905, 5000), None);
        assert_eq!(received_bytes_after_chunk(u64::MAX, 1, u64::MAX), None);
    }
}
