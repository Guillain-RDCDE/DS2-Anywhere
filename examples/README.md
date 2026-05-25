# examples/

Drop your `.ds2` or `.dss` files here for quick testing. This folder is mounted into both Docker containers (`daemon` and `webui`) by `docker-compose.yml`.

```bash
cp /path/to/your/recording.ds2 examples/
docker compose up --build
# Then convert via:
#   http://localhost:8080/convertisseur.php  (web UI)
#   curl -X POST http://localhost:8765/convert-upload?ext=ds2 --data-binary @examples/recording.ds2 -o out.mp3  (HTTP)
#   docker compose exec daemon conv-dss-ds2-to-mp3 /data/recording.ds2 /data/recording.mp3  (CLI inside the container)
```

## If you don't have a `.ds2` to test with

Use the FATE sample we ship for the upstream FFmpeg submission:

```bash
cp submission/fate/sample-qp.ds2 examples/
```

It's a 37-second DS2 QP file (16 kHz mono, 129 KiB), originally hosted as a public test artefact on dictate.com.au's CDN, with neutral content ("DICTATE" as author metadata, no third-party identification). We use it both as a FATE regression test for the FFmpeg patch and as a known-good starting point for anyone trying the toolchain.

## Bring your own

We don't ship your client recordings here, obviously. Any 20-second DS2 / DSS file you have access to works as a smoke test — the converter is format-aware (it inspects the magic bytes and routes to SP or QP automatically).

If you have a CC0 / public-domain DS2 recording you'd like to add as an additional fixture for community testing, open an issue or PR.
