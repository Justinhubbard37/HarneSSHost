use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::num::NonZeroU16;
use url::Url;

const READY_PREFIX: &[u8] = b"dsh web: ";
const BROWSER_OPEN_MESSAGE: &[u8] =
    b"dsh web: opening the default browser; pass --no-open to disable";
const MAX_UNFINISHED_LINE_BYTES: usize = 2_048;
const DEEPSEEK_TOKEN_LENGTH: usize = 43;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReadyOrigin {
    port: NonZeroU16,
}

pub(crate) struct SensitiveReadyTarget {
    origin: ReadyOrigin,
    token: Box<str>,
}

impl SensitiveReadyTarget {
    fn new(port: NonZeroU16, token: &str) -> Self {
        Self {
            origin: ReadyOrigin { port },
            token: token.into(),
        }
    }

    pub(crate) fn port(&self) -> NonZeroU16 {
        self.origin.port
    }

    pub(crate) fn into_authenticated_url(self) -> Url {
        let mut url = Url::parse("http://127.0.0.1/")
            .expect("the fixed DeepSeek loopback origin must be a valid URL");
        url.set_port(Some(self.origin.port.get()))
            .expect("the fixed HTTP URL accepts an explicit port");
        url.query_pairs_mut().append_pair("token", &self.token);
        url
    }

    fn token_matches(&self, token: &str) -> bool {
        self.token.as_ref() == token
    }
}

impl Debug for SensitiveReadyTarget {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SensitiveReadyTarget")
            .field("origin", &self.origin)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl Display for SensitiveReadyTarget {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "DeepSeek readiness target at http://127.0.0.1:{} with a redacted credential",
            self.origin.port
        )
    }
}

struct ReadyIdentity {
    port: NonZeroU16,
    token: Box<str>,
}

#[derive(Default)]
pub(crate) struct DeepSeekReadinessParser {
    unfinished_line: Vec<u8>,
    discarding_overlong_line: bool,
    ready_identity: Option<ReadyIdentity>,
}

impl DeepSeekReadinessParser {
    pub(crate) fn push(
        &mut self,
        chunk: &[u8],
    ) -> Vec<Result<SensitiveReadyTarget, ReadinessError>> {
        let mut events = Vec::new();

        for &byte in chunk {
            if self.discarding_overlong_line {
                if byte == b'\n' {
                    self.discarding_overlong_line = false;
                }
                continue;
            }

            if byte == b'\n' {
                let mut line = std::mem::take(&mut self.unfinished_line);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if let Some(event) = self.process_line(&line) {
                    events.push(event);
                }
                continue;
            }

            if self.unfinished_line.len() == MAX_UNFINISHED_LINE_BYTES {
                self.unfinished_line.clear();
                self.discarding_overlong_line = true;
                events.push(Err(ReadinessError::LineTooLong));
                continue;
            }

            self.unfinished_line.push(byte);
        }

        events
    }

    fn process_line(
        &mut self,
        line: &[u8],
    ) -> Option<Result<SensitiveReadyTarget, ReadinessError>> {
        if line == BROWSER_OPEN_MESSAGE || !line.starts_with(READY_PREFIX) {
            return None;
        }

        let source = match std::str::from_utf8(&line[READY_PREFIX.len()..]) {
            Ok(source) => source,
            Err(_) => return Some(Err(ReadinessError::MalformedReadiness)),
        };
        let target = match parse_target(source) {
            Ok(target) => target,
            Err(error) => return Some(Err(error)),
        };

        if let Some(identity) = &self.ready_identity {
            if identity.port == target.port() && target.token_matches(&identity.token) {
                return None;
            }
            return Some(Err(ReadinessError::ConflictingReadiness));
        }

        self.ready_identity = Some(ReadyIdentity {
            port: target.port(),
            token: target.token.clone(),
        });
        Some(Ok(target))
    }
}

fn parse_target(source: &str) -> Result<SensitiveReadyTarget, ReadinessError> {
    if !source.starts_with("http://127.0.0.1:") {
        return Err(ReadinessError::MalformedReadiness);
    }

    let parsed = Url::parse(source).map_err(|_| ReadinessError::MalformedReadiness)?;
    if parsed.scheme() != "http"
        || parsed.host_str() != Some("127.0.0.1")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.fragment().is_some()
    {
        return Err(ReadinessError::MalformedReadiness);
    }

    let port = parsed
        .port()
        .and_then(NonZeroU16::new)
        .ok_or(ReadinessError::MalformedReadiness)?;
    let query = parsed
        .query()
        .and_then(|query| query.strip_prefix("token="))
        .filter(|token| !token.contains('&'))
        .ok_or(ReadinessError::MalformedReadiness)?;
    if query.len() != DEEPSEEK_TOKEN_LENGTH
        || !query
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(ReadinessError::MalformedReadiness);
    }

    Ok(SensitiveReadyTarget::new(port, query))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReadinessError {
    MalformedReadiness,
    ConflictingReadiness,
    LineTooLong,
}

impl ReadinessError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::MalformedReadiness => "deepseek.readiness-malformed",
            Self::ConflictingReadiness => "deepseek.readiness-conflict",
            Self::LineTooLong => "deepseek.readiness-line-too-long",
        }
    }
}

impl Display for ReadinessError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::MalformedReadiness => "DeepSeek emitted an invalid readiness target.",
            Self::ConflictingReadiness => "DeepSeek emitted conflicting readiness targets.",
            Self::LineTooLong => "DeepSeek emitted an overlong unfinished output line.",
        };
        write!(formatter, "{}: {message}", self.code())
    }
}

impl Error for ReadinessError {}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGH123456789";

    fn ready_line(port: u16, token: &str) -> String {
        format!("dsh web: http://127.0.0.1:{port}/?token={token}\n")
    }

    fn one_result(input: &str) -> Result<SensitiveReadyTarget, ReadinessError> {
        let mut parser = DeepSeekReadinessParser::default();
        let mut events = parser.push(input.as_bytes());
        assert_eq!(events.len(), 1);
        events.remove(0)
    }

    #[test]
    fn parses_a_complete_readiness_line() {
        let target = one_result(&ready_line(3080, TOKEN)).unwrap();
        assert_eq!(target.port().get(), 3080);
    }

    #[test]
    fn parses_every_split_point_without_losing_or_duplicating_readiness() {
        let line = ready_line(3080, TOKEN);
        for split in 0..=line.len() {
            let mut parser = DeepSeekReadinessParser::default();
            let mut events = parser.push(&line.as_bytes()[..split]);
            events.extend(parser.push(&line.as_bytes()[split..]));

            assert_eq!(events.len(), 1, "split point {split}");
            assert_eq!(events[0].as_ref().unwrap().port().get(), 3080);
        }
    }

    #[test]
    fn handles_multiple_lines_in_one_chunk_and_crlf() {
        let readiness = ready_line(3080, TOKEN).replace('\n', "\r\n");
        let input = format!("booting\r\n{readiness}ignored\r\n");
        let mut parser = DeepSeekReadinessParser::default();
        let events = parser.push(input.as_bytes());

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].as_ref().unwrap().port().get(), 3080);
    }

    #[test]
    fn ignores_the_known_browser_open_informational_line() {
        let mut parser = DeepSeekReadinessParser::default();
        let events =
            parser.push(b"dsh web: opening the default browser; pass --no-open to disable\n");
        assert!(events.is_empty());
    }

    #[test]
    fn rejects_malformed_readiness_variants() {
        let cases = [
            "dsh web: not-a-url\n".to_string(),
            format!("dsh web: http://localhost:3080/?token={TOKEN}\n"),
            format!("dsh web: https://127.0.0.1:3080/?token={TOKEN}\n"),
            format!("dsh web: http://127.0.0.1:0/?token={TOKEN}\n"),
            "dsh web: http://127.0.0.1:3080/\n".to_string(),
            format!("dsh web: http://127.0.0.1:3080/?token={TOKEN}&token={TOKEN}\n"),
            format!("dsh web: http://127.0.0.1:3080/?token={TOKEN}&mode=dev\n"),
            "dsh web: http://127.0.0.1:3080/?token=short\n".to_string(),
            "dsh web: http://127.0.0.1:3080/?token=abcdefghijklmnopqrstuvwxyzABCDEFGH12345678!\n"
                .to_string(),
            format!("dsh web: http://user@127.0.0.1:3080/?token={TOKEN}\n"),
            format!("dsh web: http://127.0.0.1:3080/path?token={TOKEN}\n"),
            format!("dsh web: http://127.0.0.1:3080/?token={TOKEN}#fragment\n"),
        ];

        for case in cases {
            let error = one_result(&case).unwrap_err();
            assert_eq!(error, ReadinessError::MalformedReadiness);
        }
    }

    #[test]
    fn rejects_conflicting_repeated_readiness_but_ignores_identical_repeats() {
        let mut parser = DeepSeekReadinessParser::default();
        assert!(parser.push(ready_line(3080, TOKEN).as_bytes())[0].is_ok());
        assert!(parser.push(ready_line(3080, TOKEN).as_bytes()).is_empty());

        let events = parser.push(ready_line(3081, TOKEN).as_bytes());
        assert!(matches!(
            events.as_slice(),
            [Err(ReadinessError::ConflictingReadiness)]
        ));

        let different_token = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh123456789";
        let mut parser = DeepSeekReadinessParser::default();
        assert!(parser.push(ready_line(3080, TOKEN).as_bytes())[0].is_ok());
        let events = parser.push(ready_line(3080, different_token).as_bytes());
        assert!(matches!(
            events.as_slice(),
            [Err(ReadinessError::ConflictingReadiness)]
        ));
    }

    #[test]
    fn rejects_an_overlong_partial_line_and_bounds_retention() {
        let mut parser = DeepSeekReadinessParser::default();
        let events = parser.push(&vec![b'x'; MAX_UNFINISHED_LINE_BYTES + 500]);

        assert!(matches!(
            events.as_slice(),
            [Err(ReadinessError::LineTooLong)]
        ));
        assert!(parser.unfinished_line.is_empty());
        assert!(parser.discarding_overlong_line);
    }

    #[test]
    fn sensitive_target_debug_and_display_are_redacted() {
        let target = one_result(&ready_line(3080, TOKEN)).unwrap();
        let debug = format!("{target:?}");
        let display = target.to_string();

        assert!(!debug.contains(TOKEN));
        assert!(!display.contains(TOKEN));
        assert!(!debug.contains("?token="));
        assert!(!display.contains("?token="));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn phase_4c_authenticated_url_is_available_only_from_the_sensitive_backend_type() {
        let target = one_result(&ready_line(3080, TOKEN)).unwrap();
        let url = target.into_authenticated_url();

        assert_eq!(url.scheme(), "http");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.port(), Some(3080));
        let expected_query = format!("token={TOKEN}");
        assert_eq!(url.query(), Some(expected_query.as_str()));
    }

    #[test]
    fn parser_errors_never_reproduce_source_input() {
        let secret = "this-source-must-never-appear-in-an-error";
        let error = one_result(&format!("dsh web: {secret}\n")).unwrap_err();
        let rendered = error.to_string();

        assert!(!rendered.contains(secret));
        assert!(!rendered.contains("dsh web:"));
    }
}
