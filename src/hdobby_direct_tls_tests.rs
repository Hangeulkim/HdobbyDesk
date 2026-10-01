//! Ephemeral identity and loopback tests. No live endpoints, profiles, keys or documents.
use hbb_common::{
    direct_tls::{self, Identity, PeerTrust},
    serde_json, tokio, Stream,
};
use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::{TcpListener, TcpStream},
};

struct RecordedSocket {
    socket: TcpStream,
    writes: Arc<Mutex<Vec<u8>>>,
}

impl AsyncRead for RecordedSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_read(cx, buf)
    }
}
impl AsyncWrite for RecordedSocket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.socket).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                self.writes.lock().unwrap().extend_from_slice(&buf[..n]);
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}

async fn sockets() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (client, server) = tokio::join!(
        TcpStream::connect(listener.local_addr().unwrap()),
        listener.accept()
    );
    (client.unwrap(), server.unwrap().0)
}

async fn encrypted_pair() -> (Stream, Stream) {
    let identity = Identity::generate().unwrap();
    let trust = PeerTrust::from_pairing_code(&identity.pairing_code()).unwrap();
    let (client, server) = sockets().await;
    let (c, s) = tokio::join!(
        direct_tls::connect_stream(client, "127.0.0.1:0".parse().unwrap(), trust, 1000),
        direct_tls::accept_stream(
            server,
            "127.0.0.1:0".parse().unwrap(),
            identity.server_config().unwrap(),
            1000
        )
    );
    (c.unwrap(), s.unwrap())
}

#[derive(Default)]
struct WireEdit {
    tamper: bool,
    replay: bool,
    captured: Option<Vec<u8>>,
}

struct ProxyTask(tokio::task::JoinHandle<io::Result<()>>);

impl Drop for ProxyTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

// Forward complete TLS records so corruption/replay tests don't depend on TCP
// segmentation. This proxy has no TLS keys and only binds ephemeral loopback sockets.
async fn encrypted_pair_with_wire_edit() -> (Stream, Stream, Arc<Mutex<WireEdit>>, ProxyTask) {
    let (client, proxy_client) = sockets().await;
    let (proxy_server, server) = sockets().await;
    let state = Arc::new(Mutex::new(WireEdit::default()));
    let editor = state.clone();
    let task = tokio::spawn(async move {
        let (mut from_client, mut to_client) = proxy_client.into_split();
        let (mut from_server, mut to_server) = proxy_server.into_split();
        let forward = async move {
            loop {
                let mut header = [0u8; 5];
                from_client.read_exact(&mut header).await?;
                let length = u16::from_be_bytes([header[3], header[4]]) as usize;
                let mut record = header.to_vec();
                record.resize(5 + length, 0);
                from_client.read_exact(&mut record[5..]).await?;
                {
                    let mut edit = editor.lock().unwrap();
                    if edit.tamper {
                        *record.last_mut().unwrap() ^= 1;
                        edit.tamper = false;
                    } else if edit.replay {
                        record = edit.captured.clone().expect("an earlier record was captured");
                        edit.replay = false;
                    } else {
                        edit.captured = Some(record.clone());
                    }
                }
                to_server.write_all(&record).await?;
            }
            #[allow(unreachable_code)]
            Ok::<(), io::Error>(())
        };
        let backward = async move {
            tokio::io::copy(&mut from_server, &mut to_client).await?;
            Ok::<(), io::Error>(())
        };
        tokio::try_join!(forward, backward)?;
        Ok(())
    });
    let identity = Identity::generate().unwrap();
    let trust = PeerTrust::from_pairing_code(&identity.pairing_code()).unwrap();
    let (c, s) = tokio::join!(
        direct_tls::connect_stream(client, "127.0.0.1:0".parse().unwrap(), trust, 1000),
        direct_tls::accept_stream(server, "127.0.0.1:0".parse().unwrap(), identity.server_config().unwrap(), 1000)
    );
    state.lock().unwrap().captured = None;
    (c.unwrap(), s.unwrap(), state, ProxyTask(task))
}

#[tokio::test]
async fn direct_tls_rejects_modified_ciphertext_before_delivering_payload() {
    let (mut client, mut server, edit, _proxy) = encrypted_pair_with_wire_edit().await;
    // A successful control record proves the proxy itself passes real TLS traffic.
    client.send_raw(b"control".to_vec()).await.unwrap();
    assert_eq!(server.next_timeout(1000).await.unwrap().unwrap().as_ref(), b"control");
    edit.lock().unwrap().tamper = true;
    client.send_raw(b"must not be delivered".to_vec()).await.unwrap();
    assert!(server.next_timeout(1000).await.unwrap().is_err());
}

#[tokio::test]
async fn direct_tls_rejects_replayed_ciphertext_in_the_same_session() {
    let (mut client, mut server, edit, _proxy) = encrypted_pair_with_wire_edit().await;
    client.send_raw(b"first".to_vec()).await.unwrap();
    assert_eq!(server.next_timeout(1000).await.unwrap().unwrap().as_ref(), b"first");
    assert!(edit.lock().unwrap().captured.is_some());
    edit.lock().unwrap().replay = true;
    client.send_raw(b"other".to_vec()).await.unwrap();
    assert!(server.next_timeout(1000).await.unwrap().is_err());
}

#[tokio::test]
async fn direct_tls_requires_the_dedicated_alpn() {
    for protocols in [Vec::new(), vec![b"other-protocol/1".to_vec()]] {
        let identity = Identity::generate().unwrap();
        let mut config = identity.server_config().unwrap();
        Arc::make_mut(&mut config).alpn_protocols = protocols;
        let (client, server) = sockets().await;
        let (c, s) = tokio::join!(
            direct_tls::connect_stream(client, "127.0.0.1:0".parse().unwrap(),
                PeerTrust::from_pairing_code(&identity.pairing_code()).unwrap(), 1000),
            direct_tls::accept_stream(server, "127.0.0.1:0".parse().unwrap(), config, 1000)
        );
        assert!(c.is_err() && s.is_err());
    }
}

#[tokio::test]
async fn direct_tls_login_frame_limit_rejects_oversize_and_allows_normal_frames() {
    let (mut client, mut server) = encrypted_pair().await;
    let boundary = vec![42; direct_tls::UNAUTHENTICATED_MAX_FRAME];
    client.send_raw(boundary.clone()).await.unwrap();
    assert_eq!(server.next_timeout(1000).await.unwrap().unwrap().as_ref(), boundary.as_slice());
    client.send_raw(vec![42; direct_tls::UNAUTHENTICATED_MAX_FRAME + 1]).await.unwrap();
    assert!(server.next_timeout(1000).await.unwrap().is_err());
}

#[tokio::test]
async fn direct_tls_login_frame_limit_is_lifted_only_explicitly() {
    let (mut client, mut server) = encrypted_pair().await;
    if let Stream::DirectTls(framed) = &mut server {
        framed.0.codec_mut().set_max_packet_length(usize::MAX);
    } else {
        panic!("TLS transport required");
    }
    let payload = vec![42; direct_tls::UNAUTHENTICATED_MAX_FRAME + 1];
    client.send_raw(payload.clone()).await.unwrap();
    assert_eq!(server.next_timeout(1000).await.unwrap().unwrap().as_ref(), payload.as_slice());
}

#[tokio::test]
async fn direct_tls_real_loopback_roundtrip_is_encrypted_in_both_directions() {
    let identity = Identity::generate().unwrap();
    let trust = PeerTrust::from_pairing_code(&identity.pairing_code()).unwrap();
    let (client, server) = sockets().await;
    let c_writes = Arc::new(Mutex::new(Vec::new()));
    let s_writes = Arc::new(Mutex::new(Vec::new()));
    let ca = client.local_addr().unwrap();
    let sa = server.local_addr().unwrap();
    let (c, s) = tokio::join!(
        direct_tls::connect_stream(
            RecordedSocket {
                socket: client,
                writes: c_writes.clone()
            },
            ca,
            trust,
            1000
        ),
        direct_tls::accept_stream(
            RecordedSocket {
                socket: server,
                writes: s_writes.clone()
            },
            sa,
            identity.server_config().unwrap(),
            1000
        )
    );
    let (mut client, mut server) = (c.unwrap(), s.unwrap());
    assert!(client.is_secured() && server.is_secured());
    c_writes.lock().unwrap().clear();
    s_writes.lock().unwrap().clear();
    let payload = "local-only 한영 abc 123\n🙂".as_bytes();
    client.send_raw(payload.to_vec()).await.unwrap();
    assert_eq!(
        server.next_timeout(1000).await.unwrap().unwrap().as_ref(),
        payload
    );
    server.send_raw(payload.to_vec()).await.unwrap();
    assert_eq!(
        client.next_timeout(1000).await.unwrap().unwrap().as_ref(),
        payload
    );
    let cw = c_writes.lock().unwrap();
    let sw = s_writes.lock().unwrap();
    assert!(!cw.is_empty() && !sw.is_empty());
    assert!(!cw.windows(payload.len()).any(|p| p == payload));
    assert!(!sw.windows(payload.len()).any(|p| p == payload));
    assert_ne!(
        *cw, *sw,
        "opposite directions must not reuse the same ciphertext"
    );
}

#[tokio::test]
async fn direct_tls_dials_only_the_supplied_loopback_endpoint() {
    let identity = Identity::generate().unwrap();
    let trust = PeerTrust::from_pairing_code(&identity.pairing_code()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let host = async {
        let (socket, _) = listener.accept().await.unwrap();
        let addr = socket.local_addr().unwrap();
        direct_tls::accept_stream(socket, addr, identity.server_config().unwrap(), 1000).await
    };
    let (client, server) = tokio::join!(direct_tls::connect(&endpoint, trust, 1000), host);
    assert!(client.unwrap().is_secured() && server.unwrap().is_secured());
}

#[tokio::test]
async fn direct_tls_rejects_a_different_host_certificate() {
    let trusted = Identity::generate().unwrap();
    let impostor = Identity::generate().unwrap();
    let (client, server) = sockets().await;
    let (c, s) = tokio::join!(
        direct_tls::connect_stream(
            client,
            "127.0.0.1:0".parse().unwrap(),
            PeerTrust::from_pairing_code(&trusted.pairing_code()).unwrap(),
            1000
        ),
        direct_tls::accept_stream(
            server,
            "127.0.0.1:0".parse().unwrap(),
            impostor.server_config().unwrap(),
            1000
        )
    );
    assert!(s.is_err());
    assert_eq!(c.err().unwrap().to_string(), "Direct TLS certificate rejected");
}

#[tokio::test]
async fn direct_tls_checks_certificate_validity_and_protocol_name_even_when_paired() {
    for failure in ["name", "expired", "future"] {
        let name = if failure == "name" {
            "wrong-protocol.invalid"
        } else {
            "hdobby.direct"
        };
        let mut params = rcgen::CertificateParams::new(vec![name.to_owned()]).unwrap();
        if failure == "expired" {
            params.not_before = rcgen::date_time_ymd(2010, 1, 1);
            params.not_after = rcgen::date_time_ymd(2011, 1, 1);
        } else if failure == "future" {
            params.not_before = rcgen::date_time_ymd(4098, 1, 1);
            params.not_after = rcgen::date_time_ymd(4099, 1, 1);
        }
        let key = rcgen::KeyPair::generate().unwrap();
        let certificate = params.self_signed(&key).unwrap();
        let identity: Identity = serde_json::from_value(serde_json::json!({
            "certificate": certificate.der().to_vec(),
            "private_key": key.serialize_der(),
        }))
        .unwrap();
        let (client, server) = sockets().await;
        let (c, s) = tokio::join!(
            direct_tls::connect_stream(
                client,
                "127.0.0.1:0".parse().unwrap(),
                PeerTrust::from_pairing_code(&identity.pairing_code()).unwrap(),
                1000
            ),
            direct_tls::accept_stream(
                server,
                "127.0.0.1:0".parse().unwrap(),
                identity.server_config().unwrap(),
                1000
            )
        );
        assert!(s.is_err());
        let message = match failure {
            "expired" => "Direct TLS certificate expired",
            "future" => "Direct TLS certificate not yet valid",
            _ => "Direct TLS certificate rejected",
        };
        assert_eq!(c.err().unwrap().to_string(), message);
    }
}

#[tokio::test]
async fn direct_tls_eof_is_a_connection_failure_not_a_certificate_rejection() {
    let identity = Identity::generate().unwrap();
    let (client, mut server) = sockets().await;
    let host = async {
        // Consume ClientHello then close without sending a certificate.
        let mut header = [0; 5];
        server.read_exact(&mut header).await.unwrap();
        let length = u16::from_be_bytes([header[3], header[4]]) as usize;
        assert!(length < 16 * 1024);
        server.read_exact(&mut vec![0; length]).await.unwrap();
        server.shutdown().await.unwrap();
    };
    let (result, ()) = tokio::join!(
        direct_tls::connect_stream(client, "127.0.0.1:0".parse().unwrap(),
            PeerTrust::from_pairing_code(&identity.pairing_code()).unwrap(), 1000),
        host
    );
    let error = result.err().unwrap();
    assert_eq!(error.to_string(), "Direct TLS connection closed during handshake");
    assert_eq!(error.downcast_ref::<io::Error>().unwrap().kind(), io::ErrorKind::UnexpectedEof);
}

#[tokio::test]
async fn direct_tls_client_timeout_and_plaintext_have_distinct_safe_messages() {
    for plaintext in [false, true] {
        let identity = Identity::generate().unwrap();
        let (client, mut server) = sockets().await;
        if plaintext {
            server.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await.unwrap();
        }
        let result = direct_tls::connect_stream(client, "127.0.0.1:0".parse().unwrap(),
            PeerTrust::from_pairing_code(&identity.pairing_code()).unwrap(), 100).await;
        let expected = if plaintext { "Direct TLS protocol negotiation failed" }
            else { "Direct TLS handshake timed out" };
        assert_eq!(result.err().unwrap().to_string(), expected);
    }
}

#[test]
fn direct_tls_requires_explicit_well_formed_trust() {
    for invalid in [
        "",
        " ",
        "public",
        "hdobby1:",
        "hdobby1:!!!!",
        "hdobby1:YWJj",
    ] {
        assert!(PeerTrust::from_pairing_code(invalid).is_err());
    }
    assert!(PeerTrust::from_pairing_code(&format!("hdobby1:{}", "a".repeat(10000))).is_err());
}

#[test]
fn direct_tls_pairing_code_uses_no_padding_base32_and_reads_legacy_codes() {
    use hbb_common::base64::{engine::general_purpose::STANDARD, Engine};

    let identity = Identity::generate().unwrap();
    let code = identity.pairing_code();
    assert!(code.starts_with("hdobby2:"));
    assert!(!code.contains('='));
    assert!(!code.contains('+'));
    assert!(!code.contains('/'));

    let expected = PeerTrust::from_pairing_code(&code).unwrap().fingerprint();
    let lowercase = code.to_ascii_lowercase();
    assert_eq!(
        PeerTrust::from_pairing_code(&lowercase).unwrap().fingerprint(),
        expected
    );

    let value = serde_json::to_value(&identity).unwrap();
    let certificate: Vec<u8> = serde_json::from_value(value["certificate"].clone()).unwrap();
    let legacy = format!("hdobby1:{}", STANDARD.encode(certificate));
    assert_eq!(
        PeerTrust::from_pairing_code(&legacy).unwrap().fingerprint(),
        expected
    );
}

#[test]
fn direct_tls_address_validation_does_not_resolve_or_select_a_server() {
    for (input, expected) in [
        ("127.0.0.1", "127.0.0.1:21118"),
        ("::1", "[::1]:21118"),
        ("[::1]:4000", "[::1]:4000"),
        ("localhost:21118", "localhost:21118"),
        ("DEVICE.EXAMPLE:1234", "device.example:1234"),
    ] {
        assert_eq!(direct_tls::endpoint(input).unwrap(), expected);
    }
    for invalid in ["", "123456789", "127.0.0.1:0", "host.example:99999",
        "user@host.example:21118", " 127.0.0.1", "host .example:1234", "http://localhost:1234"] {
        assert!(direct_tls::endpoint(invalid).is_err());
    }
}

#[test]
fn direct_tls_preparation_reuses_saved_identity_and_refuses_corruption() {
    let dir = std::env::temp_dir().join(format!("hdobby-prepare-{}", hbb_common::uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("identity.json");
    let first = Identity::load_or_create(&path).unwrap();
    let saved = std::fs::read(&path).unwrap();
    let second = Identity::load_or_create(&path).unwrap();
    assert_eq!(first.pairing_code(), second.pairing_code());
    assert_eq!(saved, std::fs::read(&path).unwrap());
    std::fs::write(&path, b"damaged-test-identity").unwrap();
    assert!(Identity::load_or_create(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"damaged-test-identity");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn direct_tls_identity_migration_preserves_existing_pairings_and_never_overwrites_target() {
    let dir = std::env::temp_dir().join(format!(
        "hdobby-identity-migration-{}",
        hbb_common::uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&dir).unwrap();
    let legacy = dir.join("legacy.json");
    let target = dir.join("durable.json");
    let old = Identity::load_or_create(&legacy).unwrap();
    let old_code = old.pairing_code();
    let old_bytes = std::fs::read(&legacy).unwrap();

    let migrated = Identity::load_or_create_migrating(&target, &[legacy.as_path()]).unwrap();
    assert_eq!(migrated.pairing_code(), old_code);
    assert_eq!(std::fs::read(&legacy).unwrap(), old_bytes);

    let winner = Identity::generate().unwrap();
    std::fs::remove_file(&target).unwrap();
    winner.create_file(&target).unwrap();
    let loaded = Identity::load_or_create_migrating(&target, &[legacy.as_path()]).unwrap();
    assert_eq!(loaded.pairing_code(), winner.pairing_code());

    std::fs::remove_file(&target).unwrap();
    std::fs::write(&legacy, b"damaged-existing-identity").unwrap();
    assert!(Identity::load_or_create_migrating(&target, &[legacy.as_path()]).is_err());
    assert!(!target.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn direct_tls_rejects_plaintext_and_bounds_handshake_wait() {
    for send_plaintext in [true, false] {
        let identity = Identity::generate().unwrap();
        let (mut client, server) = sockets().await;
        if send_plaintext {
            client
                .write_all(b"plaintext input must never reach the remote host")
                .await
                .unwrap();
        }
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            direct_tls::accept_stream(
                server,
                "127.0.0.1:0".parse().unwrap(),
                identity.server_config().unwrap(),
                40,
            ),
        )
        .await;
        assert!(result.unwrap().is_err());
    }
    let identity = Identity::generate().unwrap();
    let (client, _silent_host) = sockets().await;
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        direct_tls::connect_stream(
            client,
            "127.0.0.1:0".parse().unwrap(),
            PeerTrust::from_pairing_code(&identity.pairing_code()).unwrap(),
            40,
        ),
    )
    .await;
    assert!(result.unwrap().is_err());
}

#[test]
fn direct_tls_rejects_mismatched_private_key() {
    let identity = Identity::generate().unwrap();
    let other = Identity::generate().unwrap();
    let mut value = serde_json::to_value(identity).unwrap();
    value["private_key"] = serde_json::to_value(other).unwrap()["private_key"].clone();
    let mismatch: Identity = serde_json::from_value(value).unwrap();
    assert!(mismatch.server_config().is_err());
}

#[test]
fn direct_tls_identity_storage_does_not_overwrite_or_use_partial_identity() {
    let dir = std::env::temp_dir().join(format!(
        "hdobby-direct-tls-{}",
        hbb_common::uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("identity.json");
    let identity = Identity::generate().unwrap();
    identity.create_file(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        Identity::load(&path).unwrap().pairing_code(),
        identity.pairing_code()
    );
    assert!(identity.create_file(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    std::fs::write(&path, b"{\"certificate\":[]").unwrap();
    assert!(Identity::load(&path).is_err());
    std::fs::write(&path, vec![b'a'; 20000]).unwrap();
    assert!(Identity::load(&path).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
struct PrivateIdentityTestDir(std::path::PathBuf);

#[cfg(unix)]
impl PrivateIdentityTestDir {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let path = std::env::temp_dir().join(format!(
            "hdobby-private-identity-{}",
            hbb_common::uuid::Uuid::new_v4()
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}

#[cfg(unix)]
impl Drop for PrivateIdentityTestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
#[cfg(unix)]
fn direct_tls_identity_rejects_links_without_changing_the_original() {
    use std::os::unix::fs::symlink;
    let dir = PrivateIdentityTestDir::new();
    let path = dir.0.join("identity.json");
    let identity = Identity::load_or_create(&path).unwrap();
    let saved = std::fs::read(&path).unwrap();
    let link = dir.0.join("alias.json");
    symlink(&path, &link).unwrap();
    assert!(Identity::load(&link).is_err());
    assert!(Identity::load_or_create(&link).is_err());
    std::fs::remove_file(&link).unwrap();
    std::fs::hard_link(&path, &link).unwrap();
    assert!(Identity::load(&path).is_err());
    assert!(Identity::load(&link).is_err());
    std::fs::remove_file(&link).unwrap();
    assert_eq!(Identity::load(&path).unwrap().pairing_code(), identity.pairing_code());
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    let missing = dir.0.join("missing.json");
    symlink(&missing, &link).unwrap();
    assert!(Identity::load_or_create(&link).is_err());
    assert!(!missing.exists(), "a dangling link must not trigger key creation");
}

#[test]
#[cfg(unix)]
fn direct_tls_identity_rejects_shared_permissions_and_preserves_the_key() {
    use std::os::unix::fs::PermissionsExt;
    let dir = PrivateIdentityTestDir::new();
    let path = dir.0.join("identity.json");
    let identity = Identity::load_or_create(&path).unwrap();
    let saved = std::fs::read(&path).unwrap();
    for mode in [0o640, 0o604, 0o620, 0o602] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(Identity::load_or_create(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), saved);
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(Identity::load(&path).unwrap().pairing_code(), identity.pairing_code());
}

#[test]
#[cfg(unix)]
fn direct_tls_identity_rejects_special_files_without_waiting_for_a_writer() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, time::Instant};
    let dir = PrivateIdentityTestDir::new();
    assert!(Identity::load(&dir.0).is_err());
    let path = dir.0.join("fifo");
    let c_path = CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { hbb_common::libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let started = Instant::now();
    assert!(Identity::load_or_create(&path).is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn direct_tls_raw_mode_keeps_tls_and_disconnect_does_not_hang() {
    let (mut client, mut server) = encrypted_pair().await;
    client.set_raw();
    server.set_raw();
    assert!(client.is_secured() && server.is_secured());
    client.send_raw(b"local raw mode".to_vec()).await.unwrap();
    assert_eq!(
        server.next_timeout(1000).await.unwrap().unwrap().as_ref(),
        b"local raw mode"
    );
    drop(client);
    let result = tokio::time::timeout(Duration::from_secs(1), server.next())
        .await
        .unwrap();
    assert!(result.is_none() || result.unwrap().is_err());
}

#[test]
fn direct_tls_listener_selection_never_wraps_or_falls_back_on_invalid_config() {
    assert_eq!(direct_tls::listener_endpoint("", "").unwrap(), (None, 21118));
    assert_eq!(direct_tls::listener_endpoint("127.0.0.1", "21118").unwrap(),
        (Some("127.0.0.1".parse().unwrap()), 21118));
    assert_eq!(direct_tls::listener_endpoint("::1", "65535").unwrap(),
        (Some("::1".parse().unwrap()), 65535));
    for port in ["0", "-1", "65536", "65537", "2147483647", "not-a-port", " 21118"] {
        assert!(direct_tls::listener_endpoint("127.0.0.1", port).is_err());
    }
    for ip in ["localhost", "127.0.0.1:21118", "invalid", " 127.0.0.1"] {
        assert!(direct_tls::listener_endpoint(ip, "21118").is_err());
    }
}
