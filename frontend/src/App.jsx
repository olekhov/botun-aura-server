import React, { useEffect, useState } from "react";
import axios from "axios";

const LOCAL_IP_PREFIXES = ["127.", "10.", "192.168", "172.",];

function isLocalAddress(ip) {
  return LOCAL_IP_PREFIXES.some(prefix => ip.startsWith(prefix));
}

function extractIpFromMultiaddr(addr) {
  const match = addr.match(/\/ip4\/(\d+\.\d+\.\d+\.\d+)/);
  return match ? match[1] : null;
}

export default function PeerDashboard() {
  const [peers, setPeers] = useState([]);
  const [geoData, setGeoData] = useState({});

  useEffect(() => {
    const fetchData = async () => {
      try {
        const res = await axios.get("/peers");
        setPeers(res.data);

        const ips = Array.from(new Set(
          res.data.flatMap(p => p.addrinfo.map(a => extractIpFromMultiaddr(a.address)))
        )).filter(ip => ip && !isLocalAddress(ip));

        for (const ip of ips) {
          if (!geoData[ip]) {
            const geo = await axios.get(`https://ipapi.co/${ip}/country_code/`);
            setGeoData(prev => ({ ...prev, [ip]: geo.data }));
          }
        }
      } catch (err) {
        console.error("Fetch failed", err);
      }
    };

    fetchData();
    const interval = setInterval(fetchData, 5000);
    return () => clearInterval(interval);
  }, [geoData]);

  return (
    <div style={{ padding: "1rem", fontFamily: "sans-serif" }}>
      <h2>🌐 P2P Peers</h2>
      {peers.map(peer => (
        <div key={peer.peer} style={{ border: "1px solid #ccc", margin: "1rem 0", padding: "1rem", borderRadius: "8px" }}>
          <div><strong>Peer:</strong> {peer.peer}</div>
          <div><strong>Ping:</strong> {peer.ping} ms</div>
          <div><strong>Last Seen:</strong> {new Date(peer.last_seen * 1000).toLocaleString()}</div>
          <div>
            <strong>Addresses:</strong>
            <ul>
              {peer.addrinfo.map((addrObj, idx) => {
                const ip = extractIpFromMultiaddr(addrObj.address);
                const country = ip && geoData[ip];
                return (
                  <li key={idx}>
                    {addrObj.address} {" "}
                    {ip ? (
                      isLocalAddress(ip) ? (
                        <span title="Local/VPN">🕸️</span>
                      ) : country ? (
                        <span title={country}>
                          <img
                            src={`https://flagcdn.com/16x12/${country.toLowerCase()}.png`}
                            alt={country}
                            style={{ marginLeft: "0.5em" }}
                          />
                        </span>
                      ) : (" 🌍")
                    ) : null}
                  </li>
                );
              })}
            </ul>
          </div>
        </div>
      ))}
    </div>
  );
}
