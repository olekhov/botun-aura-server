Botun Aura Server
=================

This is a rendezvous server for p2p network "Botun Aura".

To operate properly it requires 32 bytes key. Generate with
```sh
openssl rand -hex 32
```
and place the value into environment variable `BOTUN_AURA_RENDEZVOUS_SERVER_KEY`.
You can use `.env` file as well.

When run, the server will output it's PeerId into log, save the value and
distribute to peers.


