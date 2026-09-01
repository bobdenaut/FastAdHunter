use tracing::debug;

pub const MAX_HELLO_BYTES: usize = 16 * 1024;

const RECORD_HANDSHAKE: u8 = 0x16;
const RECORD_HEADER: usize = 5;
const HANDSHAKE_CLIENT_HELLO: u8 = 0x01;
const EXT_SERVER_NAME: u16 = 0x0000;
const NAME_TYPE_HOST: u8 = 0x00;
const MAX_NAME_LEN: usize = 253;
const MAX_LABEL_LEN: usize = 63;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelloScan {
    Sni(Box<str>),
    NoSni,
    NotTls,
    Incomplete,
}

struct Records<'a> {
    buf: &'a [u8],
    record_end: usize,
    next_record: usize,
    at: usize,
    read: usize,
    truncated: bool,
}

impl<'a> Records<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            record_end: 0,
            next_record: 0,
            at: 0,
            read: 0,
            truncated: false,
        }
    }

    fn advance_record(&mut self) -> bool {
        let start = self.next_record;
        if start + RECORD_HEADER > self.buf.len() {
            self.truncated = true;
            return false;
        }
        if self.buf[start] != RECORD_HANDSHAKE {
            return false;
        }
        let len = u16::from_be_bytes([self.buf[start + 3], self.buf[start + 4]]) as usize;
        let body = start + RECORD_HEADER;
        let end = body + len;
        if end > self.buf.len() {
            self.truncated = true;
            return false;
        }
        self.at = body;
        self.record_end = end;
        self.next_record = end;
        true
    }

    fn fill(&mut self) -> bool {
        while self.at == self.record_end {
            if !self.advance_record() {
                return false;
            }
        }
        true
    }

    fn read_u8(&mut self) -> Option<u8> {
        if !self.fill() {
            return None;
        }
        let byte = self.buf[self.at];
        self.at += 1;
        self.read += 1;
        Some(byte)
    }

    fn read_u16(&mut self) -> Option<u16> {
        let hi = self.read_u8()?;
        let lo = self.read_u8()?;
        Some(u16::from_be_bytes([hi, lo]))
    }

    fn read_u24(&mut self) -> Option<u32> {
        let a = u32::from(self.read_u8()?);
        let b = u32::from(self.read_u8()?);
        let c = u32::from(self.read_u8()?);
        Some((a << 16) | (b << 8) | c)
    }

    fn skip(&mut self, mut count: usize) -> Option<()> {
        while count > 0 {
            if !self.fill() {
                return None;
            }
            let step = (self.record_end - self.at).min(count);
            self.at += step;
            self.read += step;
            count -= step;
        }
        Some(())
    }

    fn copy_into(&mut self, out: &mut [u8]) -> Option<()> {
        let mut written = 0;
        while written < out.len() {
            if !self.fill() {
                return None;
            }
            let step = (self.record_end - self.at).min(out.len() - written);
            out[written..written + step].copy_from_slice(&self.buf[self.at..self.at + step]);
            self.at += step;
            self.read += step;
            written += step;
        }
        Some(())
    }
}

enum Walk {
    Found(usize),
    Absent,
    Malformed,
    Short,
}

pub fn scan_client_hello(buf: &[u8]) -> HelloScan {
    if buf.is_empty() {
        return HelloScan::Incomplete;
    }
    if buf[0] != RECORD_HANDSHAKE {
        return HelloScan::NotTls;
    }

    let mut name = [0u8; MAX_NAME_LEN];
    let mut reader = Records::new(buf);
    match walk(&mut reader, &mut name) {
        Walk::Found(len) => match normalize(&name[..len]) {
            Some(host) => HelloScan::Sni(host),
            None => {
                debug!(
                    len,
                    name = ?String::from_utf8_lossy(&name[..len]),
                    "SNI name rejected; treated as no SNI"
                );
                HelloScan::NoSni
            }
        },
        Walk::Absent => HelloScan::NoSni,
        Walk::Malformed => HelloScan::NotTls,
        Walk::Short if reader.truncated => HelloScan::Incomplete,
        Walk::Short => HelloScan::NotTls,
    }
}

fn walk(reader: &mut Records<'_>, name: &mut [u8; MAX_NAME_LEN]) -> Walk {
    macro_rules! need {
        ($expr:expr) => {
            match $expr {
                Some(value) => value,
                None => return Walk::Short,
            }
        };
    }

    if need!(reader.read_u8()) != HANDSHAKE_CLIENT_HELLO {
        return Walk::Malformed;
    }
    let body_len = need!(reader.read_u24()) as usize;
    let body_start = reader.read;

    need!(reader.skip(2 + 32));
    let session_id = need!(reader.read_u8()) as usize;
    need!(reader.skip(session_id));
    let cipher_suites = need!(reader.read_u16()) as usize;
    if !cipher_suites.is_multiple_of(2) {
        return Walk::Malformed;
    }
    need!(reader.skip(cipher_suites));
    let compression = need!(reader.read_u8()) as usize;
    need!(reader.skip(compression));

    let consumed = reader.read - body_start;
    if consumed > body_len {
        return Walk::Malformed;
    }
    if consumed == body_len {
        return Walk::Absent;
    }

    let extensions_len = need!(reader.read_u16()) as usize;
    if consumed + 2 + extensions_len > body_len {
        return Walk::Malformed;
    }

    let mut remaining = extensions_len;
    while remaining > 0 {
        if remaining < 4 {
            return Walk::Malformed;
        }
        let ext_type = need!(reader.read_u16());
        let ext_len = need!(reader.read_u16()) as usize;
        remaining -= 4;
        if ext_len > remaining {
            return Walk::Malformed;
        }
        remaining -= ext_len;
        if ext_type != EXT_SERVER_NAME {
            need!(reader.skip(ext_len));
            continue;
        }
        return server_name(reader, ext_len, name);
    }
    Walk::Absent
}

fn server_name(reader: &mut Records<'_>, ext_len: usize, name: &mut [u8; MAX_NAME_LEN]) -> Walk {
    macro_rules! need {
        ($expr:expr) => {
            match $expr {
                Some(value) => value,
                None => return Walk::Short,
            }
        };
    }

    if ext_len < 2 {
        return Walk::Malformed;
    }
    let list_len = need!(reader.read_u16()) as usize;
    if list_len + 2 != ext_len {
        return Walk::Malformed;
    }

    let mut left = list_len;
    let mut found = None;
    while left > 0 {
        if left < 3 {
            return Walk::Malformed;
        }
        let name_type = need!(reader.read_u8());
        let name_len = need!(reader.read_u16()) as usize;
        left -= 3;
        if name_len > left {
            return Walk::Malformed;
        }
        left -= name_len;
        if name_type == NAME_TYPE_HOST && found.is_none() && name_len <= MAX_NAME_LEN {
            need!(reader.copy_into(&mut name[..name_len]));
            found = Some(name_len);
        } else {
            need!(reader.skip(name_len));
        }
    }
    match found {
        Some(len) => Walk::Found(len),
        None => Walk::Absent,
    }
}

fn normalize(raw: &[u8]) -> Option<Box<str>> {
    if raw.is_empty() || raw.len() > MAX_NAME_LEN {
        return None;
    }
    let mut host = String::with_capacity(raw.len());
    let mut label = 0usize;
    let last = raw.len() - 1;
    for (index, &byte) in raw.iter().enumerate() {
        let ch = byte.to_ascii_lowercase();
        match ch {
            b'.' => {
                if label == 0 || index == last {
                    return None;
                }
                label = 0;
            }
            b'-' => {
                if label == 0 || index == last {
                    return None;
                }
                label += 1;
            }
            b'a'..=b'z' | b'0'..=b'9' | b'_' => {
                label += 1;
                if label > MAX_LABEL_LEN {
                    return None;
                }
            }
            _ => return None,
        }
        host.push(char::from(ch));
    }
    Some(host.into_boxed_str())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn hello(server_name: Option<&str>) -> Vec<u8> {
        let mut extensions = Vec::new();
        if let Some(name) = server_name {
            let mut entry = vec![NAME_TYPE_HOST];
            entry.extend_from_slice(&(name.len() as u16).to_be_bytes());
            entry.extend_from_slice(name.as_bytes());

            let mut ext = Vec::new();
            ext.extend_from_slice(&(entry.len() as u16).to_be_bytes());
            ext.extend_from_slice(&entry);

            extensions.extend_from_slice(&EXT_SERVER_NAME.to_be_bytes());
            extensions.extend_from_slice(&(ext.len() as u16).to_be_bytes());
            extensions.extend_from_slice(&ext);
        }
        extensions.extend_from_slice(&0xfe0du16.to_be_bytes());
        extensions.extend_from_slice(&4u16.to_be_bytes());
        extensions.extend_from_slice(&[0, 1, 2, 3]);

        let mut body = Vec::new();
        body.extend_from_slice(&[0x03, 0x03]);
        body.extend_from_slice(&[0x42; 32]);
        body.push(0);
        body.extend_from_slice(&2u16.to_be_bytes());
        body.extend_from_slice(&[0x13, 0x01]);
        body.push(1);
        body.push(0);
        body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        body.extend_from_slice(&extensions);

        let mut handshake = vec![HANDSHAKE_CLIENT_HELLO];
        handshake.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        handshake.extend_from_slice(&body);

        record(&handshake)
    }

    fn record(payload: &[u8]) -> Vec<u8> {
        let mut out = vec![RECORD_HANDSHAKE, 0x03, 0x01];
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn fragment(bytes: &[u8], at: usize) -> Vec<u8> {
        let (head, tail) = bytes[RECORD_HEADER..].split_at(at);
        let mut out = record(head);
        out.extend_from_slice(&record(tail));
        out
    }

    async fn capture_hello(name: rustls::pki_types::ServerName<'static>) -> Vec<u8> {
        use tokio::io::AsyncReadExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let provider = std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth();
            let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));
            let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
            let _ = connector.connect(name, stream).await;
        });

        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; MAX_HELLO_BYTES];
        let mut len = 0;
        while len < buf.len() {
            let read = stream.read(&mut buf[len..]).await.unwrap();
            if read == 0 {
                break;
            }
            len += read;
            if !matches!(scan_client_hello(&buf[..len]), HelloScan::Incomplete) {
                break;
            }
        }
        buf.truncate(len);
        buf
    }

    #[tokio::test]
    async fn a_real_rustls_client_hello_yields_its_sni() {
        let name = rustls::pki_types::ServerName::try_from("origin.test").unwrap();
        assert_eq!(
            scan_client_hello(&capture_hello(name).await),
            HelloScan::Sni("origin.test".into())
        );
    }

    #[tokio::test]
    async fn a_real_client_hello_addressed_to_an_ip_carries_no_sni() {
        let name =
            rustls::pki_types::ServerName::from("127.0.0.1".parse::<std::net::IpAddr>().unwrap());
        assert_eq!(
            scan_client_hello(&capture_hello(name).await),
            HelloScan::NoSni
        );
    }

    #[test]
    fn extracts_the_server_name() {
        assert_eq!(
            scan_client_hello(&hello(Some("origin.test"))),
            HelloScan::Sni("origin.test".into())
        );
    }

    #[test]
    fn a_hello_without_the_extension_has_no_sni() {
        assert_eq!(scan_client_hello(&hello(None)), HelloScan::NoSni);
    }

    #[test]
    fn a_name_is_lowercased() {
        assert_eq!(
            scan_client_hello(&hello(Some("Origin.TEST"))),
            HelloScan::Sni("origin.test".into())
        );
    }

    #[test]
    fn an_underscore_is_a_name_the_resolver_will_take() {
        assert_eq!(
            scan_client_hello(&hello(Some("my_host.example.com"))),
            HelloScan::Sni("my_host.example.com".into())
        );
    }

    #[test]
    fn a_hello_split_across_records_is_reassembled() {
        let bytes = hello(Some("origin.test"));
        for split in [1, 4, 40, 60] {
            assert_eq!(
                scan_client_hello(&fragment(&bytes, split)),
                HelloScan::Sni("origin.test".into()),
                "split at {split}"
            );
        }
    }

    #[test]
    fn a_truncated_hello_asks_for_more_bytes() {
        let bytes = hello(Some("origin.test"));
        for cut in 1..bytes.len() {
            assert_eq!(
                scan_client_hello(&bytes[..cut]),
                HelloScan::Incomplete,
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn bytes_that_are_not_a_handshake_record_are_not_tls() {
        assert_eq!(scan_client_hello(b"GET / HTTP/1.1\r\n"), HelloScan::NotTls);
        assert_eq!(scan_client_hello(&[]), HelloScan::Incomplete);
    }

    #[test]
    fn a_handshake_that_is_not_a_client_hello_is_not_tls() {
        let mut bytes = hello(Some("origin.test"));
        bytes[RECORD_HEADER] = 0x02;
        assert_eq!(scan_client_hello(&bytes), HelloScan::NotTls);
    }

    #[test]
    fn hostile_names_are_refused_rather_than_forwarded() {
        for name in [
            "\0evil",
            "exa mple.com",
            "exam\u{0001}ple.com",
            "-example.com",
            "example.com-",
            ".example.com",
            "example.com.",
            "example..com",
            "exämple.com",
            "https://example.com",
            "example.com:443",
        ] {
            assert_eq!(
                scan_client_hello(&hello(Some(name))),
                HelloScan::NoSni,
                "{name}"
            );
        }
        let long = format!("{}.com", "a".repeat(250));
        assert_eq!(scan_client_hello(&hello(Some(&long))), HelloScan::NoSni);
    }

    #[test]
    fn trailing_records_after_the_hello_do_not_disturb_the_scan() {
        let mut bytes = hello(Some("origin.test"));
        bytes.extend_from_slice(&record(&[0xab; 4096]));
        assert!(bytes.len() < MAX_HELLO_BYTES);
        assert_eq!(
            scan_client_hello(&bytes),
            HelloScan::Sni("origin.test".into())
        );
    }

    #[test]
    fn mutated_length_fields_never_panic() {
        let base = hello(Some("origin.test"));
        for index in 0..base.len() {
            for patch in [0x00u8, 0x01, 0x7f, 0xff] {
                let mut bytes = base.clone();
                bytes[index] = patch;
                let _ = scan_client_hello(&bytes);
                bytes.extend_from_slice(&[0xff; 64]);
                let _ = scan_client_hello(&bytes);
            }
            let _ = scan_client_hello(&base[..index]);
        }
    }
}
