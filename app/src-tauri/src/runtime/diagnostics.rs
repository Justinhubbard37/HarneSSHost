use std::collections::VecDeque;

const REDACTED: &str = "[REDACTED]";
const REDACTED_READINESS: &str = "DeepSeek readiness output was redacted.";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiagnosticRecord {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl DiagnosticRecord {
    fn retained_size(&self) -> usize {
        self.code.len() + self.message.len()
    }
}

pub(crate) struct DiagnosticBuffer {
    records: VecDeque<DiagnosticRecord>,
    retained_bytes: usize,
    max_records: usize,
    max_retained_bytes: usize,
    max_record_bytes: usize,
}

impl DiagnosticBuffer {
    pub(crate) fn new(
        max_records: usize,
        max_retained_bytes: usize,
        max_record_bytes: usize,
    ) -> Self {
        Self {
            records: VecDeque::new(),
            retained_bytes: 0,
            max_records,
            max_retained_bytes,
            max_record_bytes,
        }
    }

    pub(crate) fn record(&mut self, code: &'static str, source: &str) {
        if self.max_records == 0 || self.max_retained_bytes == 0 || self.max_record_bytes == 0 {
            return;
        }

        let message = truncate_utf8(&sanitize(source), self.max_record_bytes);
        let record = DiagnosticRecord { code, message };
        let record_size = record.retained_size();
        if record_size > self.max_retained_bytes {
            return;
        }

        while self.records.len() >= self.max_records
            || self.retained_bytes + record_size > self.max_retained_bytes
        {
            let Some(removed) = self.records.pop_front() else {
                break;
            };
            self.retained_bytes -= removed.retained_size();
        }

        self.retained_bytes += record_size;
        self.records.push_back(record);
    }

    #[cfg(test)]
    pub(crate) fn records(&self) -> &VecDeque<DiagnosticRecord> {
        &self.records
    }
}

fn sanitize(source: &str) -> String {
    if source.contains("dsh web:") && contains_token_parameter(source) {
        return REDACTED_READINESS.to_string();
    }

    redact_token_parameters(source)
}

fn contains_token_parameter(source: &str) -> bool {
    source
        .as_bytes()
        .windows(b"token=".len())
        .any(|window| window.eq_ignore_ascii_case(b"token="))
}

fn redact_token_parameters(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut result = String::with_capacity(source.len());
    let mut cursor = 0;

    while let Some(relative) = find_token_parameter(&bytes[cursor..]) {
        let start = cursor + relative;
        let value_start = start + b"token=".len();
        result.push_str(&source[cursor..value_start]);
        result.push_str(REDACTED);

        let mut value_end = value_start;
        while value_end < bytes.len() && !is_parameter_terminator(bytes[value_end]) {
            value_end += 1;
        }
        cursor = value_end;
    }

    result.push_str(&source[cursor..]);
    result
}

fn find_token_parameter(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(b"token=".len())
        .position(|window| window.eq_ignore_ascii_case(b"token="))
}

fn is_parameter_terminator(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'&' | b'#' | b'\'' | b'"' | b')' | b']' | b'}')
}

fn truncate_utf8(source: &str, max_bytes: usize) -> String {
    if source.len() <= max_bytes {
        return source.to_string();
    }

    let mut end = max_bytes;
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    source[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGH123456789";

    #[test]
    fn bounds_record_count() {
        let mut diagnostics = DiagnosticBuffer::new(2, 1_000, 100);
        diagnostics.record("one", "first");
        diagnostics.record("two", "second");
        diagnostics.record("three", "third");

        assert_eq!(diagnostics.records().len(), 2);
        assert_eq!(diagnostics.records()[0].code, "two");
        assert_eq!(diagnostics.records()[1].code, "three");
    }

    #[test]
    fn bounds_total_retained_size() {
        let mut diagnostics = DiagnosticBuffer::new(10, 18, 100);
        diagnostics.record("a", "12345678");
        diagnostics.record("b", "abcdefgh");
        diagnostics.record("c", "ABCDEFGH");

        assert!(diagnostics.retained_bytes <= 18);
        assert_eq!(diagnostics.records().len(), 2);
        assert_eq!(diagnostics.records()[0].code, "b");
    }

    #[test]
    fn bounds_each_record_before_retention() {
        let mut diagnostics = DiagnosticBuffer::new(10, 1_000, 5);
        diagnostics.record("code", "abcdefghi");

        assert_eq!(diagnostics.records()[0].message, "abcde");
    }

    #[test]
    fn redacts_token_parameters_case_insensitively() {
        let mut diagnostics = DiagnosticBuffer::new(10, 1_000, 500);
        diagnostics.record(
            "request",
            &format!("request failed: http://127.0.0.1:3080/?TOKEN={TOKEN}&mode=local"),
        );
        let retained = &diagnostics.records()[0].message;

        assert!(!retained.contains(TOKEN));
        assert!(retained.contains("TOKEN=[REDACTED]&mode=local"));
    }

    #[test]
    fn replaces_authenticated_readiness_output_before_retention() {
        let source = format!("dsh web: http://127.0.0.1:3080/?token={TOKEN}");
        let mut diagnostics = DiagnosticBuffer::new(10, 1_000, 500);
        diagnostics.record("stdout", &source);
        let retained = &diagnostics.records()[0].message;

        assert_eq!(retained, REDACTED_READINESS);
        assert!(!retained.contains(TOKEN));
        assert!(!retained.contains("127.0.0.1"));
        assert!(!retained.contains("?token="));
        assert_ne!(retained, &source);
    }
}
