//! Binary entry point for the LinkHub signaling server.
//!
//! Usage: `linkhub-signaling-server [BIND_ADDR]` (default `127.0.0.1:9000`,
//! or set `LINKHUB_SIGNALING_ADDR`). For M2 this is meant to run locally while
//! we validate the link; production deployment (TLS, public host) is deferred.

use std::env;

use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = env::args()
        .nth(1)
        .or_else(|| env::var("LINKHUB_SIGNALING_ADDR").ok())
        .unwrap_or_else(|| "127.0.0.1:9000".to_string());

    let listener = TcpListener::bind(&addr).await?;
    eprintln!(
        "linkhub-signaling-server listening on ws://{}",
        listener.local_addr()?
    );

    linkhub_signaling_server::serve(listener).await
}
