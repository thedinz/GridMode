# GridMode website

The marketing site is a static Nginx container. Its download page reads the latest public GitHub release at runtime and links directly to the newest Windows `.exe` and macOS `.dmg` assets.

## Run

From the repository root:

```bash
docker compose up --build -d
```

Open `http://localhost:9999`. Set `GRIDMODE_SITE_PORT` to publish a different host port. Point your reverse proxy upstream at port `9999`; Nginx continues to listen on port `80` inside the container.

## Publish the private production image

The production image is `ghcr.io/thedinz/gridmode-website:latest`. The publishing workflow intentionally uses a dedicated `GHCR_TOKEN` repository secret instead of `GITHUB_TOKEN`, so the new package is created with private package visibility rather than inheriting visibility from this public repository.

Create a classic GitHub personal access token with `write:packages`, save it as the repository Actions secret `GHCR_TOKEN`, then push this branch or run the **Publish private website image** workflow manually. Do not add an `org.opencontainers.image.source` label or enable repository permission inheritance for this package.

After the first workflow succeeds, confirm **GridMode Website → Package settings → Danger Zone → Change visibility** shows **Private** before using it on the server. A public GHCR package cannot be made private again.

On the server, create a separate classic personal access token with only `read:packages`, then authenticate once:

```bash
echo "$GRIDMODE_GHCR_TOKEN" | docker login ghcr.io -u thedinz --password-stdin
docker compose -f compose.production.yaml pull
docker compose -f compose.production.yaml up -d
```

The production Compose file binds the site to `127.0.0.1:9999` by default for use behind a reverse proxy. Set `GRIDMODE_SITE_PORT` to override the host port.

## Update behavior

No website rebuild is needed for a normal app release. The browser requests `https://api.github.com/repos/thedinz/GridMode/releases/latest`, then selects the first matching Windows installer and macOS disk image. If GitHub is unavailable or rate-limited, the buttons fall back to the latest-release page.

The release owner and repository are configured at the top of `public/app.js`. If the application source repository becomes private, point those values at a separate public, source-free releases repository so the website and installed app can continue finding updates.

The release repository and its downloadable assets must remain public unless the website is later given a server-side authenticated download proxy. A public GitHub repository also exposes every pushed branch, so keep this website branch local or move the site to a private repository if its source should not be public.
