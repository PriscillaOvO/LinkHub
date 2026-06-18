//! Throwaway spike: does webrtc-rs (full ICE/STUN/TURN/DTLS/DataChannel stack)
//! cross-compile for Windows host + Android NDK (arm64-v8a, x86_64)?

use webrtc::api::APIBuilder;
use webrtc::peer_connection::configuration::RTCConfiguration;

/// Build a peer connection config the way the cross-network path would: this
/// forces the ICE/DTLS/SCTP types to be linked and type-checked.
pub async fn can_build_peer_connection() -> bool {
    let api = APIBuilder::new().build();
    let config = RTCConfiguration::default();
    api.new_peer_connection(config).await.is_ok()
}
