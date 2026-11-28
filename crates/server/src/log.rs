//! The server's log: each line goes to stderr (the journal under systemd) and to a ring of the last [`KEEP`] lines that `GET /api/v1/logs` returns. Lines are redacted before they are kept: token- and key-shaped values never reach the ring or the journal.
use std::collections::VecDeque;
use std::sync::Mutex;

pub const KEEP: usize = 1000;

static RING: Mutex<VecDeque<(u64, String)>> = Mutex::new(VecDeque::new());

/// Hide session tokens, API keys, `apikey=` / `token=` query values and bearer credentials.
pub fn redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut after_bearer = false;
    for (i, w) in s.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let lw = w.to_ascii_lowercase();
        if after_bearer || w.starts_with("nst_") || w.starts_with("nsk_") {
            out.push_str("<redacted>");
        } else if let Some(p) = ["apikey=", "api_key=", "token=", "passkey="]
            .iter()
            .find_map(|k| lw.find(k).map(|p| p + k.len()))
        {
            let end = w[p..].find(['&', '"', '\'']).map_or(w.len(), |e| p + e);
            out.push_str(&w[..p]);
            out.push_str("<redacted>");
            out.push_str(&w[end..]);
        } else {
            out.push_str(w);
        }
        after_bearer = w.eq_ignore_ascii_case("bearer");
    }
    out
}

pub fn line(msg: &str) {
    let msg = redact(msg);
    eprintln!("nocturne-server: {msg}");
    let mut r = RING.lock().unwrap();
    if r.len() == KEEP {
        r.pop_front();
    }
    r.push_back((crate::state::now(), msg));
}

/// The newest `n` lines, oldest first.
pub fn recent(n: usize) -> Vec<(u64, String)> {
    let r = RING.lock().unwrap();
    r.iter().skip(r.len().saturating_sub(n)).cloned().collect()
}

/// `slog!("indexer {}: {e}", name)`: log a line (stderr and the ring), redacted.
#[macro_export]
macro_rules! slog {
    ($($t:tt)*) => { $crate::log::line(&format!($($t)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_redacted() {
        assert_eq!(redact("key nsk_abcdef used"), "key <redacted> used");
        assert_eq!(
            redact("GET http://x/api?t=search&apikey=SECRET&q=a"),
            "GET http://x/api?t=search&apikey=<redacted>&q=a"
        );
        assert_eq!(
            redact("Authorization: Bearer nst_123"),
            "Authorization: Bearer <redacted>"
        );
        assert_eq!(redact("Bearer abc.def"), "Bearer <redacted>");
        assert_eq!(redact("plain words stay"), "plain words stay");
        line("ring test nsk_zz");
        assert!(
            recent(KEEP)
                .iter()
                .any(|(_, l)| l == "ring test <redacted>")
        );
    }
}
