//! How much one Claude Code session has used, from its transcript: the tokens of its replies since
//! a moment (the start of the plan's 5-hour window). The plan's limits are the account's, not a
//! session's; comparing sessions says which agent is spending them.

use serde_json::Value;

/// Tokens a session used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    /// Written to the prompt cache: billed like input, a little more.
    pub cache_write: u64,
    /// Read back from the prompt cache: a tenth of the price, and the bulk of every long session.
    pub cache_read: u64,
}

impl Tokens {
    /// What counts against a limit, roughly: everything but cache reads, which cost little and
    /// would swamp the rest.
    pub fn counted(&self) -> u64 {
        self.input + self.output + self.cache_write
    }
}

/// Seconds since 1970 for an ISO 8601 time in UTC (`2026-10-20T10:00:00.123Z`), as transcripts
/// write them.
pub fn iso_to_unix(text: &str) -> Option<i64> {
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    // Days from 1970-01-01 to the date, by Howard Hinnant's civil calendar formula.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let of_era_days = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    let days = era * 146_097 + of_era_days - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

/// The tokens of the replies in a transcript since `since` (Unix seconds; `None` for all of them).
/// A reply is written once per part, each with the same usage, so each counts once.
pub fn read(lines: impl Iterator<Item = String>, since: Option<i64>) -> Tokens {
    let mut tally = Tally::default();
    for line in lines {
        tally.add(&line);
    }
    tally.since(since.unwrap_or(i64::MIN))
}

/// A transcript read a little at a time: it only grows, so each read takes up where the last
/// stopped instead of parsing a session of many megabytes again.
#[derive(Debug, Default)]
pub struct Tally {
    path: std::path::PathBuf,
    /// How far the file has been read: the end of its last whole line.
    read_to: u64,
    /// Each reply's time and largest usage, by its message id.
    replies: std::collections::HashMap<String, (i64, Tokens)>,
}

impl Tally {
    /// Reads what `path` gained since the last read: a line still being written waits for the
    /// next one. A different file, or one that shrank, is read from the start.
    pub fn read_new(&mut self, path: &std::path::Path) {
        use std::io::{BufRead as _, Seek as _};
        let Ok(mut file) = std::fs::File::open(path) else { return };
        let length = file.metadata().map_or(0, |meta| meta.len());
        if self.path != path || length < self.read_to {
            *self = Self { path: path.to_owned(), ..Self::default() };
        }
        if length == self.read_to || file.seek(std::io::SeekFrom::Start(self.read_to)).is_err() {
            return;
        }
        let mut reader = std::io::BufReader::new(file);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(read) if read > 0 && line.ends_with('\n') => {
                    self.read_to += read as u64;
                    self.add(&line);
                }
                _ => break,
            }
        }
    }

    /// Counts one line of a transcript, if it's a reply with usage.
    pub fn add(&mut self, line: &str) {
        if !line.contains("\"usage\"") {
            return;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else { return };
        if entry.get("type").and_then(Value::as_str) != Some("assistant") {
            return;
        }
        let at = entry.get("timestamp").and_then(Value::as_str).and_then(iso_to_unix).unwrap_or(i64::MIN);
        let Some(message) = entry.get("message") else { return };
        let Some(usage) = message.get("usage") else { return };
        let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        let id = message.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
        // The last part has the reply's final output count; keep the largest of each.
        let (kept_at, kept) = self.replies.entry(id).or_insert((at, Tokens::default()));
        *kept_at = (*kept_at).max(at);
        kept.input = kept.input.max(count("input_tokens"));
        kept.output = kept.output.max(count("output_tokens"));
        kept.cache_write = kept.cache_write.max(count("cache_creation_input_tokens"));
        kept.cache_read = kept.cache_read.max(count("cache_read_input_tokens"));
    }

    /// The tokens of the replies since `since` (Unix seconds).
    pub fn since(&self, since: i64) -> Tokens {
        self.replies.values().filter(|(at, _)| *at >= since).fold(Tokens::default(), |sum, (_, tokens)| Tokens {
            input: sum.input + tokens.input,
            output: sum.output + tokens.output,
            cache_write: sum.cache_write + tokens.cache_write,
            cache_read: sum.cache_read + tokens.cache_read,
        })
    }

    /// Forgets the replies before `since`: the plan's window only moves forward, so they never
    /// count again.
    pub fn forget_before(&mut self, since: i64) {
        self.replies.retain(|_, (at, _)| *at >= since);
    }
}

/// A count of tokens in a few characters: `950`, `12k`, `1.2M`.
pub fn short(tokens: u64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "a count shown to one decimal")]
    let value = tokens as f64;
    if tokens >= 1_000_000 {
        format!("{:.1}M", value / 1_000_000.)
    } else if tokens >= 1_000 {
        format!("{:.0}k", value / 1_000.)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_iso_times() {
        assert_eq!(iso_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_unix("2026-10-20T10:00:00.123Z"), Some(1_792_490_400));
        assert_eq!(iso_to_unix("2024-02-29T12:30:15Z"), Some(1_709_209_815));
        assert_eq!(iso_to_unix("soon"), None);
    }

    #[test]
    fn counts_each_reply_once_since_a_moment() {
        let transcript = r#"{"type":"assistant","timestamp":"2026-10-20T08:00:00Z","message":{"id":"m0","usage":{"input_tokens":999,"output_tokens":999}}}
{"type":"user","timestamp":"2026-10-20T10:00:00Z","message":{"content":"go"}}
{"type":"assistant","timestamp":"2026-10-20T10:00:01Z","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":3,"cache_creation_input_tokens":100,"cache_read_input_tokens":5000}}}
{"type":"assistant","timestamp":"2026-10-20T10:00:02Z","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":40,"cache_creation_input_tokens":100,"cache_read_input_tokens":5000}}}
{"type":"assistant","timestamp":"2026-10-20T10:05:00Z","message":{"id":"m2","usage":{"input_tokens":5,"output_tokens":20,"cache_read_input_tokens":7000}}}"#;
        let since = iso_to_unix("2026-10-20T09:00:00Z");
        let tokens = read(transcript.lines().map(str::to_owned), since);
        assert_eq!(tokens, Tokens { input: 15, output: 60, cache_write: 100, cache_read: 12_000 });
        assert_eq!(tokens.counted(), 175);
        assert_eq!(read(transcript.lines().map(str::to_owned), None).input, 1014);
    }

    #[test]
    fn a_growing_transcript_is_read_from_where_it_stopped() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let path = dir.path().join("session.jsonl");
        let reply = |id: &str, output: u64| {
            format!(
                r#"{{"type":"assistant","timestamp":"2026-10-20T10:00:00Z","message":{{"id":"{id}","usage":{{"input_tokens":1,"output_tokens":{output}}}}}}}"#
            )
        };
        std::fs::write(&path, format!("{}\n{}", reply("m1", 10), &reply("m2", 5)[..20])).expect("written");
        let mut tally = Tally::default();
        tally.read_new(&path);
        // The half-written second reply waits.
        assert_eq!(tally.since(0).output, 10);
        std::fs::write(&path, format!("{}\n{}\n{}\n", reply("m1", 10), reply("m2", 5), reply("m1", 30)))
            .expect("written");
        tally.read_new(&path);
        assert_eq!(tally.since(0), Tokens { input: 2, output: 35, cache_write: 0, cache_read: 0 });
        // Started again (a new, shorter file): read from the start.
        std::fs::write(&path, format!("{}\n", reply("m9", 7))).expect("written");
        tally.read_new(&path);
        assert_eq!(tally.since(0).output, 7);
    }

    #[test]
    fn writes_counts_short() {
        assert_eq!(short(950), "950");
        assert_eq!(short(12_400), "12k");
        assert_eq!(short(1_240_000), "1.2M");
    }
}
