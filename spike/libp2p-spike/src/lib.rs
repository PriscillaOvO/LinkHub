//! Throwaway spike: does libp2p (with dcutr/relay/noise/quic) cross-compile for
//! Windows host + Android NDK (arm64-v8a, x86_64)? Touch the API so the relevant
//! code paths are type-checked, not just downloaded.

use libp2p::{identity, PeerId};

/// Map a 32-byte Ed25519 seed (we already store Ed25519 identity keys in the
/// trust store) into a libp2p PeerId — this is the device_id <-> PeerId bridge
/// the design doc flags as an open question.
pub fn peer_id_from_ed25519_seed(seed: [u8; 32]) -> PeerId {
    let kp = identity::Keypair::ed25519_from_bytes(seed).expect("valid ed25519 seed");
    PeerId::from(kp.public())
}
