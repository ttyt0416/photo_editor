use std::sync::atomic::{AtomicU64, Ordering};
static SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub fn new() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // Keep editing available if the operating system random source fails.
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        bytes = time.to_be_bytes();
        let process = std::process::id().to_be_bytes();
        for i in 0..4 {
            bytes[i] ^= process[i];
        }
    }
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed).to_be_bytes();
    for i in 0..8 {
        bytes[8 + i] ^= sequence[i];
    }
    if bytes == [0; 16] {
        bytes[0] = 1;
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn valid(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && id.bytes().any(|b| b != b'0')
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifiers_are_valid_and_unique_across_concurrent_documents() {
        let workers: Vec<_> = (0..8)
            .map(|_| std::thread::spawn(|| (0..512).map(|_| new()).collect::<Vec<_>>()))
            .collect();
        let mut ids = std::collections::HashSet::new();
        for worker in workers {
            for id in worker.join().unwrap() {
                assert!(valid(&id));
                assert!(ids.insert(id));
            }
        }
        for invalid in [
            "",
            "../a",
            "00000000000000000000000000000000",
            "ABCDEFABCDEFABCDEFABCDEFABCDEFABCD",
            "0123456789abcdef0123456789abcdeg",
        ] {
            assert!(!valid(invalid));
        }
    }
}
