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

We don't ship a sample DS2 in the repo for licensing reasons — Olympus's own sample files are copyrighted. If you have access to a recorder, two minutes of any spoken text (including your own voice reading the Wikipedia article on something) gives you a usable test file.

Contributions welcome: if you have a CC0 / public-domain DS2 recording you're happy to donate as a fixture, open an issue or PR.
