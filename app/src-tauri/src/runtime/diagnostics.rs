use std::collections::VecDeque;

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

        let message = truncate_utf8(source, self.max_record_bytes);
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
}
