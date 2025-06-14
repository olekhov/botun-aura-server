use std::{collections::{HashMap, HashSet}, env, error::Error, sync::{Arc, Mutex}, time::Duration};

use axum::{routing::{get, post}, Json, Router};
use futures::StreamExt;
use libp2p::{
    identify, identity::Keypair, multiaddr::Protocol, noise, ping, rendezvous, swarm::{NetworkBehaviour, SwarmEvent}, tcp, yamux, Multiaddr, PeerId, Swarm
};
use serde::Serialize;
use tokio::sync::mpsc;
use tower_http::services::ServeDir;
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

#[derive(Serialize, Debug, Clone)]
struct AddrInfo {
    address: String,
}

#[derive(Serialize, Debug, Clone)]
struct PeerStat {
    peer: String,
    addrinfo: Vec<AddrInfo>,
    ping: Option<u64>,
    last_seen: i64,
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv()?;

    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let keypair = load_keypair_from_env();

    let mut swarm :Swarm<MyBehaviour> = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| MyBehaviour {
            identify: identify::Behaviour::new(identify::Config::new(
                "rendezvous-example/1.0.0".to_string(),
                key.public(),
            )),
            rendezvous: rendezvous::server::Behaviour::new(rendezvous::server::Config::default()),
            ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(10))),
        })?
        .build();

    listen_on_all_interfaces(&mut swarm);

    let peers_set = Arc::new(Mutex::new(HashMap::<PeerId, PeerStat>::new()));
    let peers_clone = peers_set.clone();

    let api_listen = env::var("BOTUN_AURA_SERVER_HTTP_ENDPOINT").expect("Http endpoint is not set");

    tokio::spawn(async move {
        let app = Router::new()
            .route("/peers", get({
                let peers = peers_clone.clone();
                move || {
                    let peers = peers.lock().unwrap().clone().values().cloned().collect::<Vec<_>>();
                    async move { Json(peers) }
                }
            }))
            .fallback_service(ServeDir::new("dist"))
        ;

        let listener = tokio::net::TcpListener::bind(api_listen).await.unwrap();

        axum::serve(listener, app)
            .await
            .unwrap();
    });

    let mut ping_peers_tick = tokio::time::interval(Duration::from_secs(10));

    loop {

        tokio::select! {
            _ = ping_peers_tick.tick() => {
                for (peer, stat) in peers_set.lock().unwrap().iter() {
                    tracing::info!("Checking peer: {peer}");
                    for addr in stat.addrinfo.iter() {
                        let ma: Multiaddr = addr.address.parse()?;
                        if let Err(e) = swarm.dial(ma) {
                            tracing::error!("Failed to dial {}: {}", addr.address, e);
                        }
                    }
                }
            }

            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => tracing::info!("Listening on {address:?}"),
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        tracing::info!("Connected to {}", peer_id);
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        tracing::info!("Disconnected from {}", peer_id);
                    }
                    SwarmEvent::Behaviour(MyBehaviourEvent::Rendezvous(
                            rendezvous::server::Event::RegistrationExpired( registration ),
                    )) => {
                        tracing::info!( "Peer {} registeration expired", registration.record.peer_id() );
                        peers_set.lock().unwrap().remove(&registration.record.peer_id());
                    }
                    SwarmEvent::Behaviour(MyBehaviourEvent::Rendezvous(
                            rendezvous::server::Event::PeerRegistered { peer, registration },
                    )) => {
                        tracing::info!(
                            "Peer {} registered for namespace '{}'",
                            peer,
                            registration.namespace
                        );

                        let mut addresses = vec![];

                        for address in registration.record.addresses() {
                            let peer = registration.record.peer_id();
                            tracing::info!(%peer, %address, "Discovered peer");

                            let p2p_suffix = Protocol::P2p(peer);
                            let address_with_p2p =
                                if !address.ends_with(&Multiaddr::empty().with(p2p_suffix.clone())) {
                                    address.clone().with(p2p_suffix)
                                } else {
                                    address.clone()
                                };
                            addresses.push( AddrInfo {
                                address: address_with_p2p.to_string(),
                            });
                        }

                        peers_set.lock().unwrap().insert(peer,
                            PeerStat {
                                peer: peer.to_string(),
                                addrinfo: addresses,
                                last_seen: chrono::Local::now().timestamp(),
                                ping: None,
                            });

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

                    SwarmEvent::Behaviour(MyBehaviourEvent::Ping(ping::Event {
                        peer,
                        result: Ok(rtt),
                        ..
                    })) => {
                        tracing::info!(%peer, "Ping is {}ms", rtt.as_millis());
                        if let Some(peer_stats) = peers_set.lock().unwrap().get_mut(&peer) {
                            peer_stats.ping = Some(rtt.as_millis() as u64);
                            peer_stats.last_seen = chrono::Local::now().timestamp();
                        }
                    }

                    other => {
                        tracing::debug!("Unhandled {:?}", other);
                    }
                }
            }
        }
    }

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

