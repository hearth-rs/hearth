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

use chacha20::cipher::Unsigned;
use opaque_ke::errors::*;
use opaque_ke::*;
use rand::rngs::OsRng;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The 64-byte key generated by the authentication step.
pub type SessionKey = [u8; 64];

#[derive(Debug)]
pub enum AuthenticationError {
    IoError(std::io::Error),
    ProtocolError(ProtocolError),
    InternalError(InternalError),
}

impl From<std::io::Error> for AuthenticationError {
    fn from(err: std::io::Error) -> Self {
        AuthenticationError::IoError(err)
    }
}

impl From<ProtocolError> for AuthenticationError {
    fn from(err: ProtocolError) -> Self {
        AuthenticationError::ProtocolError(err)
    }
}

impl From<InternalError> for AuthenticationError {
    fn from(err: InternalError) -> Self {
        AuthenticationError::InternalError(err)
    }
}

struct CS;

impl CipherSuite for CS {
    type OprfCs = Ristretto255;
    type KeGroup = Ristretto255;
    type KeyExchange = key_exchange::tripledh::TripleDh;
    type Ksf = argon2::Argon2<'static>;
}

pub struct ServerListener {}

pub struct ServerAuthenticator {
    setup: ServerSetup<CS>,
    registration: ServerRegistration<CS>,
}

impl ServerAuthenticator {
    pub fn from_password(pw: &[u8]) -> Result<Self, AuthenticationError> {
        let mut rng = OsRng;
        let client_start = ClientRegistration::start(&mut rng, pw)?;
        let setup = ServerSetup::new(&mut rng);
        let cred_id = b"";
        let server_start = ServerRegistration::start(&setup, client_start.message, cred_id)?;
        let client_finish =
            client_start
                .state
                .finish(&mut rng, pw, server_start.message, Default::default())?;
        let registration = ServerRegistration::finish(client_finish.message);

        Ok(Self {
            setup,
            registration,
        })
    }

    pub async fn login<T: AsyncRead + AsyncWrite + Unpin>(
        &self,
        client: &mut T,
    ) -> Result<SessionKey, AuthenticationError> {
        let request_len = CredentialRequestLen::<CS>::to_usize();
        let mut request_msg = vec![0u8; request_len];
        client.read_exact(&mut request_msg).await?;
        let request = CredentialRequest::deserialize(&request_msg)?;

        let mut rng = OsRng;
        let login_start = ServerLogin::start(
            &mut rng,
            &self.setup,
            Some(self.registration.clone()),
            request,
            b"",
            Default::default(),
        )?;

        let response_msg = login_start.message.serialize();
        client.write_all(&response_msg).await?;
        client.flush().await?;

        let finalize_len = CredentialFinalizationLen::<CS>::to_usize();
        let mut finalize_msg = vec![0u8; finalize_len];
        client.read_exact(&mut finalize_msg).await?;
        let finalize = CredentialFinalization::<CS>::deserialize(&finalize_msg)?;
        let finish = login_start.state.finish(finalize)?;
        Ok(finish.session_key.into())
    }
}

pub async fn login<T: AsyncRead + AsyncWrite + Unpin>(
    server: &mut T,
    pw: &[u8],
) -> Result<SessionKey, AuthenticationError> {
    let mut rng = OsRng;
    let start = ClientLogin::<CS>::start(&mut rng, pw)?;
    let start_msg = start.message.serialize();
    server.write_all(&start_msg).await?;
    server.flush().await?;

    let response_len = CredentialResponseLen::<CS>::to_usize();
    let mut response_msg = vec![0u8; response_len];
    server.read_exact(&mut response_msg).await?;
    let response = CredentialResponse::<CS>::deserialize(&response_msg)?;

    let finish = start.state.finish(pw, response, Default::default())?;
    let finish_msg = finish.message.serialize();
    server.write_all(&finish_msg).await?;
    server.flush().await?;
    Ok(finish.session_key.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticator_from_password() {
        let _auth = ServerAuthenticator::from_password(b"deadbeef").unwrap();
    }

    #[tokio::test]
    async fn authenticate_correct() {
        let password = b"deadbeef";
        let auth = ServerAuthenticator::from_password(password).unwrap();
        let (mut client, mut server) = tokio::io::duplex(128);
        let server_join = tokio::spawn(async move { auth.login(&mut client).await });
        let client_result = login(&mut server, password).await;
        let server_result = server_join.await.unwrap();
        let server_key = server_result.unwrap();
        let client_key = client_result.unwrap();
        assert_eq!(server_key, client_key);
    }

    #[tokio::test]
    async fn authenticate_incorrect() {
        let password = b"deadbeef";
        let wrong_password = b"bingus_love";
        let auth = ServerAuthenticator::from_password(password).unwrap();
        let (mut client, mut server) = tokio::io::duplex(128);
        tokio::spawn(async move { auth.login(&mut client).await });
        let client_result = login(&mut server, wrong_password).await;
        match client_result {
            Err(AuthenticationError::ProtocolError(ProtocolError::InvalidLoginError)) => {}
            result => panic!("Unexpected result: {:?}", result),
        }
    }
}
