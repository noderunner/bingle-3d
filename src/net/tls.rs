//! TLS: certificates checked as a browser checks them, plus a self-signed certificate the player
//! has chosen to trust, recognised by its SHA-256 fingerprint.
//!
//! Bingle can serve a certificate it made for itself (`BINGLE_TLS_SELFSIGNED=1`) and prints that
//! certificate's fingerprint when it starts (Bingle's ADR 0065). No authority vouches for it, so
//! the normal check fails. Rather than turning checks off, the client shows the fingerprint it was
//! offered; if the player says it is the one the server printed, it is remembered for that server
//! and accepted from then on, and only that exact certificate is.
//!
//! ureq has no setting for a verifier of our own, so [`TlsConnector`] does the handshake itself,
//! in ureq's connector chain, where ureq's own rustls connector would be.

use std::fmt;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, RwLock};

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore,
    SignatureScheme, StreamOwned,
};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, Either, LazyBuffers, NextTimeout, Transport,
    TransportAdapter,
};

/// The SHA-256 of a certificate (of its DER encoding), as browsers and Bingle show it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    pub fn of(der: &[u8]) -> Self {
        let digest = ring::digest::digest(&ring::digest::SHA256, der);
        let mut out = [0; 32];
        out.copy_from_slice(digest.as_ref());
        Fingerprint(out)
    }

    /// Reads `AB:CD:…` as Bingle prints it, and also without colons, with spaces, in either case.
    pub fn parse(s: &str) -> Option<Self> {
        let hex: Vec<u8> = s
            .bytes()
            .filter(|b| !matches!(b, b':' | b' ' | b'-'))
            .collect();
        if hex.len() != 64 {
            return None;
        }
        let mut out = [0; 32];
        for (i, pair) in hex.chunks(2).enumerate() {
            let pair = std::str::from_utf8(pair).ok()?;
            out[i] = u8::from_str_radix(pair, 16).ok()?;
        }
        Some(Fingerprint(out))
    }
}

impl fmt::Display for Fingerprint {
    /// Upper-case pairs separated by colons, which is how Bingle prints it at start-up.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(":")?;
            }
            write!(f, "{b:02X}")?;
        }
        Ok(())
    }
}

impl Fingerprint {
    /// The same as [`Display`](fmt::Display) on two lines of sixteen pairs, which fit a panel.
    pub fn two_lines(&self) -> String {
        let text = self.to_string();
        // Each pair and its colon take three characters.
        let (first, second) = text.split_at(16 * 3);
        format!("{}\n{second}", first.trim_end_matches(':'))
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

/// A certificate that failed the normal check and is not one the player trusts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub fingerprint: Fingerprint,
    /// Why the normal check failed, in words for the player.
    pub reason: String,
}

/// Decides whether a server's certificate is good: the normal check against the Mozilla roots
/// first, and when that fails, whether this exact certificate is one the player trusts.
///
/// One `Trust` belongs to one server, so its pins are that server's.
#[derive(Debug)]
pub struct Trust {
    webpki: Arc<WebPkiServerVerifier>,
    pins: RwLock<Vec<Fingerprint>>,
    /// The last certificate turned down, so the caller can offer to trust it.
    rejected: Mutex<Option<Rejection>>,
}

impl Trust {
    pub fn new(pins: Vec<Fingerprint>) -> Arc<Self> {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let webpki = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider())
            .build()
            .expect("the built-in roots make a verifier");
        Arc::new(Trust {
            webpki,
            pins: RwLock::new(pins),
            rejected: Mutex::new(None),
        })
    }

    pub fn pin(&self, fingerprint: Fingerprint) {
        let mut pins = self.pins.write().unwrap_or_else(|e| e.into_inner());
        if !pins.contains(&fingerprint) {
            pins.push(fingerprint);
        }
    }

    /// The certificate most recently turned down, if any, and forgets it.
    pub fn take_rejection(&self) -> Option<Rejection> {
        self.rejected
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    fn is_pinned(&self, fingerprint: &Fingerprint) -> bool {
        self.pins
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(fingerprint)
    }
}

impl ServerCertVerifier for Trust {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self.webpki.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Ok(ok) => Ok(ok),
            Err(err) => {
                let fingerprint = Fingerprint::of(end_entity.as_ref());
                // The player looked at this certificate's fingerprint and said it is the server's.
                // That vouches for exactly this certificate, so its names and dates are not
                // checked again; a new certificate has a new fingerprint and is asked about anew.
                if self.is_pinned(&fingerprint) {
                    return Ok(ServerCertVerified::assertion());
                }
                *self.rejected.lock().unwrap_or_else(|e| e.into_inner()) = Some(Rejection {
                    fingerprint,
                    reason: describe(&err),
                });
                Err(err)
            }
        }
    }

    // The handshake's signatures are checked as usual, pinned or not: they prove the server holds
    // the certificate's key.
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.webpki.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.webpki.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.webpki.supported_verify_schemes()
    }
}

/// Why a certificate failed, for the player.
fn describe(err: &rustls::Error) -> String {
    match err {
        rustls::Error::InvalidCertificate(cert) => match cert {
            CertificateError::UnknownIssuer => {
                "no authority this client knows has signed it (is it self-signed?)".into()
            }
            CertificateError::Expired | CertificateError::ExpiredContext { .. } => {
                "it has expired".into()
            }
            CertificateError::NotValidYet | CertificateError::NotValidYetContext { .. } => {
                "it is not valid yet (is this computer's clock right?)".into()
            }
            CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
                "it is for a different name".into()
            }
            CertificateError::Revoked => "it has been revoked".into(),
            // A certificate that is its own authority, as the one Bingle makes for itself is, is
            // turned down for that before its issuer is looked for. rustls passes webpki's reason
            // through only as an opaque error, so it is recognised by name.
            CertificateError::Other(e) if format!("{e:?}").contains("CaUsedAsEndEntity") => {
                "it is self-signed, so no authority vouches for it".into()
            }
            other => format!("it is not valid ({other:?})"),
        },
        other => other.to_string(),
    }
}

/// Every connection uses *ring*: it builds with a plain C compiler on Windows, where aws-lc-rs
/// would need cmake and nasm.
fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The rustls client configuration for one server.
pub fn client_config(trust: Arc<Trust>) -> Arc<ClientConfig> {
    let config = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(trust)
        .with_no_client_auth();
    Arc::new(config)
}

/// The TLS step of ureq's connector chain, using [`client_config`].
#[derive(Debug)]
pub struct TlsConnector {
    config: Arc<ClientConfig>,
}

impl TlsConnector {
    pub fn new(config: Arc<ClientConfig>) -> Self {
        TlsConnector { config }
    }
}

impl<In: Transport> Connector<In> for TlsConnector {
    type Out = Either<In, TlsTransport>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let Some(transport) = chained else {
            return Err(ureq::Error::ConnectionFailed);
        };
        if !details.needs_tls() || transport.is_tls() {
            return Ok(Some(Either::A(transport)));
        }
        let host = details
            .uri
            .host()
            .ok_or(ureq::Error::Tls("the address has no host"))?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let name = ServerName::try_from(host.to_string())
            .map_err(|_| ureq::Error::Tls("the host is not a valid name for TLS"))?;

        let mut conn = ClientConnection::new(self.config.clone(), name)
            .map_err(|e| ureq::Error::Other(Box::new(e)))?;
        let mut sock = TransportAdapter::new(transport.boxed(), details.current_time.clone());
        sock.set_timeout(details.timeout);
        conn.complete_io(&mut sock)?;

        let buffers = LazyBuffers::new(
            details.config.input_buffer_size(),
            details.config.output_buffer_size(),
        );
        Ok(Some(Either::B(TlsTransport {
            buffers,
            stream: StreamOwned { conn, sock },
        })))
    }
}

/// An HTTP connection inside TLS.
pub struct TlsTransport {
    buffers: LazyBuffers,
    stream: StreamOwned<ClientConnection, TransportAdapter>,
}

impl fmt::Debug for TlsTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsTransport").finish_non_exhaustive()
    }
}

impl Transport for TlsTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let output = &self.buffers.output()[..amount];
        self.stream.write_all(output)?;
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let input = self.buffers.input_append_buf();
        let amount = self.stream.read(input)?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.get_mut().get_mut().is_open()
    }

    fn is_tls(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A certificate made the way Bingle makes its own (P-256, self-signed, a CA, for localhost,
    /// 127.0.0.1 and ::1), with `openssl`; its key was thrown away, since only the certificate is
    /// needed to test the check. It is valid until 2126.
    const SELF_SIGNED: &[u8] = include_bytes!("../../tests/fixtures/self-signed.der");
    const SELF_SIGNED_SHA256: &str = include_str!("../../tests/fixtures/self-signed.sha256");

    fn verify(trust: &Trust, der: &[u8]) -> Result<ServerCertVerified, rustls::Error> {
        trust.verify_server_cert(
            &CertificateDer::from(der),
            &[],
            &ServerName::try_from("localhost").unwrap(),
            &[],
            UnixTime::now(),
        )
    }

    #[test]
    fn fingerprints_read_and_print_as_bingle_prints_them() {
        let printed = SELF_SIGNED_SHA256.trim();
        let fp = Fingerprint::parse(printed).expect("parses");
        assert_eq!(fp.to_string(), printed);
        assert_eq!(fp, Fingerprint::of(SELF_SIGNED));
        let bare = printed.replace(':', "").to_lowercase();
        assert_eq!(Fingerprint::parse(&bare), Some(fp));
        assert_eq!(Fingerprint::parse("AB:CD"), None);
        let lines = fp.two_lines();
        assert_eq!(lines.replace('\n', ":"), printed);
        assert!(lines.lines().all(|l| l.len() == 16 * 3 - 1), "{lines}");
        assert_eq!(Fingerprint::parse(&"zz".repeat(32)), None);
    }

    #[test]
    fn a_self_signed_certificate_is_turned_down_and_its_fingerprint_kept() {
        let trust = Trust::new(vec![]);
        let err = verify(&trust, SELF_SIGNED).expect_err("not trusted");
        assert!(
            matches!(err, rustls::Error::InvalidCertificate(_)),
            "{err:?}"
        );
        let rejection = trust.take_rejection().expect("remembered");
        assert_eq!(rejection.fingerprint, Fingerprint::of(SELF_SIGNED));
        assert!(
            rejection.reason.contains("self-signed"),
            "{}",
            rejection.reason
        );
        assert_eq!(trust.take_rejection(), None, "taken once");
    }

    #[test]
    fn a_pinned_certificate_is_accepted_and_only_that_one() {
        let trust = Trust::new(vec![]);
        trust.pin(Fingerprint::of(SELF_SIGNED));
        assert!(verify(&trust, SELF_SIGNED).is_ok());
        assert_eq!(trust.take_rejection(), None);

        // A different certificate (here: the same bytes with one changed, which is not even a
        // certificate any more) is not let through by the pin.
        let mut other = SELF_SIGNED.to_vec();
        let last = other.len() - 1;
        other[last] ^= 1;
        assert!(verify(&trust, &other).is_err());
    }

    #[test]
    fn a_pin_for_another_certificate_does_not_help() {
        let trust = Trust::new(vec![Fingerprint([7; 32])]);
        assert!(verify(&trust, SELF_SIGNED).is_err());
    }
}
