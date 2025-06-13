use std::{env, error::Error, time::Duration};

use futures::StreamExt;
use libp2p::{
    identify, identity::Keypair, noise, ping, rendezvous, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux, Multiaddr, Swarm
};
use tracing_subscriber::EnvFilter;

use dotenv;


fn load_keypair_from_env() -> Keypair {
    let hex = env::var("BOTUN_AURA_RENDEZVOUS_SERVER_KEY")
        .expect("BOTUN_AURA_RENDEZVOUS_SERVER_KEY not set");

    let key_bytes = hex::decode(hex)
        .expect("Invalid hex in key");

    assert_eq!(key_bytes.len(), 32, "Key must be exactly 32 bytes");

    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&key_bytes);

    Keypair::ed25519_from_bytes(key_array)
        .expect("Invalid Ed25519 key")
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv()?;

    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let keypair = load_keypair_from_env();

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default().ttl(20),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| MyBehaviour {
            identify: identify::Behaviour::new(identify::Config::new(
                "rendezvous-example/1.0.0".to_string(),
                key.public(),
            )),
            rendezvous: rendezvous::server::Behaviour::new(rendezvous::server::Config::default()),
            ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(1))),
        })?
        .build();

    listen_on_all_interfaces(&mut swarm);

    while let Some(event) = swarm.next().await {
        match event {
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                tracing::info!("Connected to {}", peer_id);
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                tracing::info!("Disconnected from {}", peer_id);
            }
            SwarmEvent::Behaviour(MyBehaviourEvent::Rendezvous(
                rendezvous::server::Event::PeerRegistered { peer, registration },
            )) => {
                tracing::info!(
                    "Peer {} registered for namespace '{}'",
                    peer,
                    registration.namespace
                );
            }
            SwarmEvent::Behaviour(MyBehaviourEvent::Rendezvous(
                rendezvous::server::Event::DiscoverServed {
                    enquirer,
                    registrations,
                },
            )) => {
                tracing::info!(
                    "Served peer {} with {} registrations",
                    enquirer,
                    registrations.len()
                );
            }
            other => {
                tracing::debug!("Unhandled {:?}", other);
            }
        }
    }

    Ok(())
}

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    identify: identify::Behaviour,
    rendezvous: rendezvous::server::Behaviour,
    ping: ping::Behaviour,
}


fn listen_on_all_interfaces<B: NetworkBehaviour>(swarm: &mut Swarm<B>) {
    let port: u16 = env::var("BOTUN_AURA_RENDEZVOUS_SERVER_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(64001);

    // IPv4: /ip4/0.0.0.0/tcp/{port}
    let addr_v4: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port)
        .parse()
        .expect("Invalid IPv4 multiaddr");

    // IPv6: /ip6/::/tcp/{port}
    let addr_v6: Multiaddr = format!("/ip6/::/tcp/{}", port)
        .parse()
        .expect("Invalid IPv6 multiaddr");

    swarm.listen_on(addr_v4).expect("Failed to listen on IPv4");
    swarm.listen_on(addr_v6).expect("Failed to listen on IPv6");
}

