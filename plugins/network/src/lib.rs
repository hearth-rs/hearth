// Copyright (c) 2023 the Hearth contributors.
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// This file is part of Hearth.
//
// Hearth is free software: you can redistribute it and/or modify it under the
// terms of the GNU Affero General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// Hearth is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License for more
// details.
//
// You should have received a copy of the GNU Affero General Public License
// along with Hearth. If not, see <https://www.gnu.org/licenses/>.

pub mod auth;
pub mod connection;
pub mod encryption;

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use auth::ServerAuthenticator;
    use encryption::{AsyncDecryptor, AsyncEncryptor, Key};

    #[tokio::test]
    async fn auth_then_encrypt() {
        const PASSWORD: &[u8] = b"deadbeef";
        const SENT: &[u8] = b"Hello, world!";
        const RECEIVED: &[u8] = b"Hello, lowly ego!";

        let authenticator = ServerAuthenticator::from_password(PASSWORD).unwrap();
        let (mut client, mut server) = tokio::io::duplex(128);

        tokio::spawn(async move {
            let session_key = authenticator.login(&mut client).await.unwrap();
            let client_key = Key::from_client_session(&session_key);
            let server_key = Key::from_server_session(&session_key);
            let (rx, tx) = tokio::io::split(client);
            let mut decryptor = AsyncDecryptor::new(&client_key, rx);
            let mut encryptor = AsyncEncryptor::new(&server_key, tx);

            let mut sent = vec![0u8; SENT.len()];
            decryptor.read_exact(&mut sent).await.unwrap();
            assert_eq!(sent, SENT);

            encryptor.write_all(RECEIVED).await.unwrap();
            encryptor.flush().await.unwrap();
        });

        let session_key = auth::login(&mut server, PASSWORD).await.unwrap();
        let client_key = Key::from_client_session(&session_key);
        let server_key = Key::from_server_session(&session_key);
        let (rx, tx) = tokio::io::split(server);
        let mut decryptor = AsyncDecryptor::new(&server_key, rx);
        let mut encryptor = AsyncEncryptor::new(&client_key, tx);

        encryptor.write_all(SENT).await.unwrap();
        encryptor.flush().await.unwrap();

        let mut received = vec![0u8; RECEIVED.len()];
        decryptor.read_exact(&mut received).await.unwrap();
        assert_eq!(received, RECEIVED);
    }
}
