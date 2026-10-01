//! Transport checks use ephemeral keys and loopback sockets only; no profile or live server.
use crate::{client::Client, common::encode64, server::secure_stream};
use hbb_common::{
    bytes::Bytes,
    futures::StreamExt,
    message_proto::{IdPk, Message, PublicKey},
    protobuf::Message as _,
    sodiumoxide::{self, crypto::sign},
    tokio, Stream,
};

const PEER: &str = "local-test-peer";

async fn streams() -> (Stream, Stream) {
    sodiumoxide::init().expect("local crypto initialization");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (client, server) = tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
    let client = client.unwrap();
    let (server, _) = server.unwrap();
    let client_addr = client.local_addr().unwrap();
    let server_addr = server.local_addr().unwrap();
    (
        Stream::from(client, client_addr),
        Stream::from(server, server_addr),
    )
}

fn proof(id: &str, peer: &sign::PublicKey, authority: &sign::SecretKey) -> Vec<u8> {
    sign::sign(
        &IdPk {
            id: id.to_owned(),
            pk: Bytes::copy_from_slice(&peer.0),
            ..Default::default()
        }
        .write_to_bytes()
        .unwrap(),
        authority,
    )
}

#[tokio::test]
async fn authenticated_loopback_roundtrip_has_no_plaintext_payload_on_wire() {
    let (mut client, mut server) = streams().await;
    let (authority_pk, authority_sk) = sign::gen_keypair();
    let (host_pk, host_sk) = sign::gen_keypair();
    let signed_peer = proof(PEER, &host_pk, &authority_sk);
    let key = encode64(authority_pk.0);
    let (accepted, connected) = tokio::join!(
        secure_stream(&mut server, PEER, &host_sk.0, &host_pk.0),
        Client::secure_connection(PEER, signed_peer, &key, &mut client)
    );
    accepted.unwrap();
    assert_eq!(connected.unwrap(), Some(host_pk.0.to_vec()));
    assert!(client.is_secured() && server.is_secured());
    let payload = "local-only 한글 abc 123\n🙂".as_bytes();
    client.send_raw(payload.to_vec()).await.unwrap();
    let Stream::Tcp(framed) = &mut server else {
        panic!("expected loopback TCP");
    };
    let mut wire = tokio::time::timeout(std::time::Duration::from_secs(1), framed.0.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_ne!(wire.as_ref(), payload);
    assert!(!wire.windows(payload.len()).any(|part| part == payload));
    framed.2.as_mut().unwrap().dec(&mut wire).unwrap();
    assert_eq!(wire.as_ref(), payload);
    server.send_raw(payload.to_vec()).await.unwrap();
    assert_eq!(
        client.next_timeout(1000).await.unwrap().unwrap().as_ref(),
        payload
    );
}

#[tokio::test]
async fn client_rejects_missing_authority_and_wrong_identity_before_sending() {
    for variant in 0..3 {
        let (mut client, mut server) = streams().await;
        let (authority_pk, authority_sk) = sign::gen_keypair();
        let (host_pk, _) = sign::gen_keypair();
        let id = if variant == 1 {
            "other-local-peer"
        } else {
            PEER
        };
        let key = if variant == 0 {
            String::new()
        } else {
            encode64(authority_pk.0)
        };
        let mut signed_peer = proof(id, &host_pk, &authority_sk);
        if variant == 2 {
            signed_peer[0] ^= 1;
        }
        assert!(
            Client::secure_connection(PEER, signed_peer, &key, &mut client)
                .await
                .is_err()
        );
        assert!(!client.is_secured());
        assert!(server.next_timeout(30).await.is_none());
    }
}

#[tokio::test]
async fn client_rejects_host_key_mismatch_without_plaintext_fallback() {
    let (mut client, mut server) = streams().await;
    let (authority_pk, authority_sk) = sign::gen_keypair();
    let (trusted_host_pk, _) = sign::gen_keypair();
    let (other_pk, other_sk) = sign::gen_keypair();
    let signed_peer = proof(PEER, &trusted_host_pk, &authority_sk);
    let server_task = tokio::spawn(async move {
        let result = secure_stream(&mut server, PEER, &other_sk.0, &other_pk.0).await;
        (result, server.is_secured())
    });
    assert!(
        Client::secure_connection(PEER, signed_peer, &encode64(authority_pk.0), &mut client)
            .await
            .is_err()
    );
    assert!(!client.is_secured());
    drop(client);
    let (result, secured) = server_task.await.unwrap();
    assert!(result.is_err());
    assert!(!secured);
}

#[tokio::test]
async fn host_rejects_empty_key_and_unexpected_message_without_starting_plaintext() {
    for public_key in [true, false] {
        let (mut client, mut server) = streams().await;
        let (host_pk, host_sk) = sign::gen_keypair();
        let server_task = tokio::spawn(async move {
            let result = secure_stream(&mut server, PEER, &host_sk.0, &host_pk.0).await;
            (result, server.is_secured())
        });
        client.next_timeout(1000).await.unwrap().unwrap();
        let mut message = Message::new();
        if public_key {
            message.set_public_key(PublicKey::new());
        }
        client.send(&message).await.unwrap();
        let (result, secured) = server_task.await.unwrap();
        assert!(result.is_err());
        assert!(!secured);
    }
}

#[tokio::test]
async fn missing_host_identity_fails_before_sending() {
    let (mut client, mut server) = streams().await;
    assert!(secure_stream(&mut server, PEER, &[], &[]).await.is_err());
    assert!(!server.is_secured());
    assert!(client.next_timeout(30).await.is_none());
}
