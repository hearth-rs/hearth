// Copyright (c) 2023 the Hearth contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::io::Result as IoResult;
use std::pin::Pin;
use std::task::{Context, Poll};

use chacha20::cipher::{KeyIvInit, StreamCipher};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::auth::SessionKey;

pub type Cipher = chacha20::ChaCha20;

/// An encryption key and IV.
///
/// This can be initialized from the [SessionKey] generated by the
/// authentication step using [Self::from_client_session] and
/// [Self::from_server_session].
pub struct Key {
    pub key: chacha20::Key,
    pub iv: chacha20::Nonce,
}

impl Key {
    /// Creates a key + IV pair from a session key for client-to-server communication.
    pub fn from_client_session(session: &SessionKey) -> Self {
        let key = chacha20::Key::clone_from_slice(&session[..32]);
        let iv = chacha20::Nonce::clone_from_slice(&session[32..44]);
        Self { key, iv }
    }

    /// Creates a key + IV pair from a session key for server-to-client communication.
    pub fn from_server_session(session: &SessionKey) -> Self {
        let key = chacha20::Key::clone_from_slice(&session[..32]);
        let iv = chacha20::Nonce::clone_from_slice(&session[44..56]);
        Self { key, iv }
    }

    /// Initializes a [Cipher] from this key and IV.
    pub fn make_cipher(&self) -> Cipher {
        Cipher::new(&self.key, &self.iv)
    }
}

pub struct AsyncDecryptor<T> {
    cipher: Cipher,
    transport: T,
}

impl<T: AsyncRead + Unpin> AsyncDecryptor<T> {
    pub fn new(key: &Key, transport: T) -> Self {
        let cipher = key.make_cipher();
        Self { cipher, transport }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for AsyncDecryptor<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<IoResult<()>> {
        let result = Pin::new(&mut self.transport).poll_read(cx, buf);
        self.cipher.apply_keystream(buf.filled_mut());
        result
    }
}

pub struct AsyncEncryptor<T> {
    cipher: Cipher,
    transport: T,
}

impl<T: AsyncWrite + Unpin> AsyncEncryptor<T> {
    pub fn new(key: &Key, transport: T) -> Self {
        let cipher = key.make_cipher();
        Self { cipher, transport }
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for AsyncEncryptor<T> {
    fn poll_write(mut self: Pin<&mut Self>, cw: &mut Context, buf: &[u8]) -> Poll<IoResult<usize>> {
        let mut encrypted = buf.to_owned();
        self.cipher.apply_keystream(&mut encrypted);
        Pin::new(&mut self.transport).poll_write(cw, &encrypted)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<IoResult<()>> {
        Pin::new(&mut self.transport).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<IoResult<()>> {
        Pin::new(&mut self.transport).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rand::{rngs::OsRng, Rng};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TEST_DATA: &'static [u8] = b"According to all known laws of aviation, there is no way that a bee should be able to fly. Its wings are too small to get its fat little body off the ground. The bee, of course, flies anyway. Because bees don't care what humans think is impossible.";

    fn generate_key() -> Key {
        let mut key = chacha20::Key::default();
        let mut iv = chacha20::Nonce::default();
        let mut rng = OsRng;
        rng.fill(key.as_mut_slice());
        rng.fill(iv.as_mut_slice());
        Key { key, iv }
    }

    #[tokio::test]
    async fn no_transport() {
        let key = generate_key();
        let mut decryptor = Cipher::new(&key.key, &key.iv);
        let mut encryptor = Cipher::new(&key.key, &key.iv);

        let mut encrypted = TEST_DATA.to_vec();
        encryptor.apply_keystream(&mut encrypted);

        let mut decrypted = encrypted.clone();
        decryptor.apply_keystream(&mut decrypted);

        assert_eq!(TEST_DATA, decrypted);
    }

    #[tokio::test]
    async fn single_message() {
        let key = generate_key();
        let (client, server) = tokio::io::duplex(2048);
        let mut encryptor = AsyncEncryptor::new(&key, server);
        let mut decryptor = AsyncDecryptor::new(&key, client);
        encryptor.write(&TEST_DATA).await.unwrap();
        let mut rx = vec![0u8; TEST_DATA.len()];
        decryptor.read_exact(&mut rx).await.unwrap();
        assert_eq!(TEST_DATA, rx);
    }

    #[tokio::test]
    async fn fragmented_message() {
        let key = generate_key();
        let (client, server) = tokio::io::duplex(2048);
        let mut encryptor = AsyncEncryptor::new(&key, server);
        let mut decryptor = AsyncDecryptor::new(&key, client);

        for chunk in TEST_DATA.chunks(7) {
            encryptor.write(chunk).await.unwrap();
        }

        let mut rx = vec![0u8; TEST_DATA.len()];
        decryptor.read_exact(&mut rx).await.unwrap();
        assert_eq!(TEST_DATA, rx);
    }
}
