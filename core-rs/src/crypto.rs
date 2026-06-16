use snow::{Builder, HandshakeState, TransportState};

const NOISE_PATTERN: &str = "Noise_KK_25519_ChaChaPoly_BLAKE2s";
const NOISE_PROLOGUE: &str = "linkhub-noise-v1";

/// Noise KK handshake state.
///
/// KK pattern: both parties know each other's static X25519 public key
/// (from the TrustStore). One round-trip, mutual authentication + key exchange.
pub struct NoiseHandshake {
    state: HandshakeState,
    done: bool,
}

impl NoiseHandshake {
    /// Build the initiator (sender-side) handshake.
    ///
    /// `local_private_key` and `remote_public_key` are 32-byte X25519 keys.
    pub fn new_initiator(
        local_private_key: &[u8; 32],
        remote_public_key: &[u8; 32],
    ) -> Result<Self, String> {
        let params = NOISE_PATTERN
            .parse()
            .map_err(|err| format!("invalid noise pattern: {err}"))?;
        let builder = Builder::new(params)
            .prologue(NOISE_PROLOGUE.as_bytes())
            .local_private_key(local_private_key)
            .remote_public_key(remote_public_key);

        let state = builder
            .build_initiator()
            .map_err(|err| format!("noise initiator build error: {err}"))?;

        Ok(Self { state, done: false })
    }

    /// Build the responder (listener-side) handshake.
    ///
    /// `local_private_key` and `remote_public_key` are 32-byte X25519 keys.
    pub fn new_responder(
        local_private_key: &[u8; 32],
        remote_public_key: &[u8; 32],
    ) -> Result<Self, String> {
        let params = NOISE_PATTERN
            .parse()
            .map_err(|err| format!("invalid noise pattern: {err}"))?;
        let builder = Builder::new(params)
            .prologue(NOISE_PROLOGUE.as_bytes())
            .local_private_key(local_private_key)
            .remote_public_key(remote_public_key);

        let state = builder
            .build_responder()
            .map_err(|err| format!("noise responder build error: {err}"))?;

        Ok(Self { state, done: false })
    }

    /// Write the next handshake message.
    ///
    /// Returns the bytes to send to the peer. After the final write,
    /// the handshake is complete and `into_transport()` should be called.
    pub fn write_message(&mut self, payload: &[u8]) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; 65535];
        let len = self
            .state
            .write_message(payload, &mut buf)
            .map_err(|err| format!("noise write error: {err}"))?;

        buf.truncate(len);

        if self.state.is_handshake_finished() {
            self.done = true;
        }

        Ok(buf)
    }

    /// Read a handshake message from the peer.
    ///
    /// Returns any payload bytes decrypted from the message.
    pub fn read_message(&mut self, payload: &[u8]) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; 65535];
        let len = self
            .state
            .read_message(payload, &mut buf)
            .map_err(|err| format!("noise read error: {err}"))?;

        buf.truncate(len);

        if self.state.is_handshake_finished() {
            self.done = true;
        }

        Ok(buf)
    }

    /// Returns true when the handshake is finished.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Convert to transport mode after the handshake is complete.
    ///
    /// # Panics
    /// Panics if the handshake is not yet complete.
    pub fn into_transport(self) -> Result<NoiseTransport, String> {
        assert!(
            self.done,
            "handshake must be complete before converting to transport"
        );

        let state = self
            .state
            .into_transport_mode()
            .map_err(|err| format!("noise transport error: {err}"))?;

        Ok(NoiseTransport { state })
    }
}

/// Noise transport for encrypting/decrypting session payloads.
///
/// Each `encrypt` call produces an AEAD-encrypted blob (ciphertext + 16-byte
/// Poly1305 tag). Use length-prefixed framing on the wire to delimit messages.
pub struct NoiseTransport {
    state: TransportState,
}

impl NoiseTransport {
    /// Encrypt a plaintext message.
    ///
    /// Returns the ciphertext with the AEAD authentication tag appended.
    /// The returned buffer has room for the tag (auth tag auto-appended).
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; plaintext.len() + 16]; // AEAD overhead
        let len = self
            .state
            .write_message(plaintext, &mut buf)
            .map_err(|err| format!("noise encrypt error: {err}"))?;

        buf.truncate(len);
        Ok(buf)
    }

    /// Decrypt a ciphertext message.
    ///
    /// `ciphertext` must include the AEAD auth tag. Returns the plaintext.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; ciphertext.len()];
        let len = self
            .state
            .read_message(ciphertext, &mut buf)
            .map_err(|err| format!("noise decrypt error: {err}"))?;

        buf.truncate(len);
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::{OsRng, RngCore};

    fn random_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        key
    }

    #[test]
    fn noise_kk_handshake_and_transport_round_trip() {
        let initiator_key = random_key();
        let responder_key = random_key();

        // Derive public keys (X25519)
        let initiator_static = x25519_dalek::StaticSecret::from(initiator_key);
        let initiator_pub = x25519_dalek::PublicKey::from(&initiator_static);
        let responder_static = x25519_dalek::StaticSecret::from(responder_key);
        let responder_pub = x25519_dalek::PublicKey::from(&responder_static);

        // Initiate handshake
        let mut initiator =
            NoiseHandshake::new_initiator(&initiator_key, &responder_pub.to_bytes()).unwrap();
        let mut responder =
            NoiseHandshake::new_responder(&responder_key, &initiator_pub.to_bytes()).unwrap();

        // Step 1: initiator -> responder
        let msg1 = initiator.write_message(&[]).unwrap();
        assert!(!initiator.is_done());

        // Step 2: responder processes msg1, sends msg2
        let payload = responder.read_message(&msg1).unwrap();
        assert!(payload.is_empty());
        let msg2 = responder.write_message(&[]).unwrap();
        assert!(responder.is_done());

        // Step 3: initiator processes msg2
        let payload = initiator.read_message(&msg2).unwrap();
        assert!(payload.is_empty());
        assert!(initiator.is_done());

        // Convert to transport
        let mut init_transport = initiator.into_transport().unwrap();
        let mut resp_transport = responder.into_transport().unwrap();

        // Encrypt a plaintext message
        let plaintext = b"hello from initiator";
        let ciphertext = init_transport.encrypt(plaintext).unwrap();
        let decrypted = resp_transport.decrypt(&ciphertext).unwrap();
        assert_eq!(&decrypted, plaintext);

        // Responder -> Initiator
        let plaintext2 = b"hello from responder";
        let ciphertext2 = resp_transport.encrypt(plaintext2).unwrap();
        let decrypted2 = init_transport.decrypt(&ciphertext2).unwrap();
        assert_eq!(&decrypted2, plaintext2);

        // Multiple messages in sequence
        for i in 0..10 {
            let msg = format!("message {i}");
            let ct = init_transport.encrypt(msg.as_bytes()).unwrap();
            let pt = resp_transport.decrypt(&ct).unwrap();
            assert_eq!(pt, msg.as_bytes());
        }
    }

    #[test]
    fn noise_transport_rejects_tampered_ciphertext() {
        let initiator_key = random_key();
        let responder_key = random_key();

        let initiator_static = x25519_dalek::StaticSecret::from(initiator_key);
        let initiator_pub = x25519_dalek::PublicKey::from(&initiator_static);
        let responder_static = x25519_dalek::StaticSecret::from(responder_key);
        let responder_pub = x25519_dalek::PublicKey::from(&responder_static);

        let mut initiator =
            NoiseHandshake::new_initiator(&initiator_key, &responder_pub.to_bytes()).unwrap();
        let mut responder =
            NoiseHandshake::new_responder(&responder_key, &initiator_pub.to_bytes()).unwrap();

        // Complete handshake
        let msg1 = initiator.write_message(&[]).unwrap();
        responder.read_message(&msg1).unwrap();
        let msg2 = responder.write_message(&[]).unwrap();
        initiator.read_message(&msg2).unwrap();

        let mut init_transport = initiator.into_transport().unwrap();
        let mut resp_transport = responder.into_transport().unwrap();

        // Encrypt, tamper, and try to decrypt
        let plaintext = b"sensitive data";
        let mut ciphertext = init_transport.encrypt(plaintext).unwrap();

        // Flip a bit in the ciphertext
        ciphertext[3] ^= 1;

        let result = resp_transport.decrypt(&ciphertext);
        assert!(result.is_err());
    }

    #[test]
    fn noise_handshake_fails_with_wrong_responder_key() {
        let initiator_key = random_key();
        let responder_key = random_key();
        let wrong_responder_key = random_key();

        let initiator_static = x25519_dalek::StaticSecret::from(initiator_key);
        let initiator_pub = x25519_dalek::PublicKey::from(&initiator_static);
        let wrong_static = x25519_dalek::StaticSecret::from(wrong_responder_key);
        let wrong_pub = x25519_dalek::PublicKey::from(&wrong_static);

        // Initiator uses wrong remote public key
        let mut initiator =
            NoiseHandshake::new_initiator(&initiator_key, &wrong_pub.to_bytes()).unwrap();
        let mut responder =
            NoiseHandshake::new_responder(&responder_key, &initiator_pub.to_bytes()).unwrap();

        let msg1 = initiator.write_message(&[]).unwrap();
        let result = responder.read_message(&msg1);
        // Handshake should fail because initiator used the wrong responder key
        assert!(result.is_err());
    }
}
