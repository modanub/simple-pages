# Simple Pages

A lightweight static site hosting platform for students. Upload HTML/CSS/JS archives via a web UI, authenticate with invite codes, and get your site served at `pages.mwit.link/{username}/`.

## Features

- **Invite-code registration** — Admin generates codes, students register with them
- **Drag & drop upload** — Upload `.zip` or `.tar.gz` archives from a clean dashboard
- **Per-user disk quotas** — Configurable limits with real-time usage display
- **Static file serving** — Each student gets `/{username}/` with their site content
- **Admin panel** — Generate, list, and revoke invite codes
- **Single binary** — One Rust binary handles auth, uploads, and serving (~6MB)
- **SQLite storage** — No external database needed

## Architecture

```
[Browser] → [Traefik] → [simple-pages container]
                              ├── Management UI at /
                              ├── Upload API at /api/site/upload
                              └── Static files at /{username}/
```

## Quick Start

```bash
# Run locally
ADMIN_PASSWORD=secret JWT_SECRET=changeme DATA_DIR=./data cargo run

# Or with Docker
docker build -t simple-pages .
docker run -p 8080:8080 \
  -e ADMIN_PASSWORD=secret \
  -e JWT_SECRET=changeme \
  -v pages_data:/data \
  simple-pages
```

Then:
1. Go to `http://localhost:8080` and login as `admin` with your `ADMIN_PASSWORD`
2. Generate invite codes from the admin panel
3. Register a student account at `/register`
4. Upload a site archive from the dashboard

## Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `ADMIN_PASSWORD` | `admin` | Password for the admin account |
| `JWT_SECRET` | `change-me-in-production` | Secret for signing JWT tokens |
| `DISK_QUOTA_MB` | `50` | Per-user disk quota in MB |
| `MAX_UPLOAD_MB` | `50` | Maximum upload file size in MB |
| `DATA_DIR` | `/data` | Directory for SQLite DB and site files |
| `LISTEN_ADDR` | `0.0.0.0:8080` | Address to listen on |

## Tech Stack

- **Rust** + **Axum** — Fast async web framework
- **SQLite** via rusqlite — Embedded database, no external deps
- **Askama** — Compile-time HTML templates
- **Bulma CSS** — Clean responsive UI from CDN
- **HTMX** — Interactive uploads without heavy JS
- **Argon2** — Secure password hashing
- **JWT** — Stateless auth via HttpOnly cookies

## Storage Layout

```
/data/
├── pages.db              # SQLite database
└── sites/
    ├── alice/
    │   ├── index.html
    │   └── style.css
    └── bob/
        └── index.html
```

## Security

- Path traversal protection on archive extraction (rejects `..`, absolute paths, symlinks, dotfiles)
- Argon2 password hashing
- JWT in HttpOnly cookies
- Per-user upload size and disk quota enforcement
- Username validation and reserved name blocking

## API

```
POST   /api/auth/register     — Register with invite code
POST   /api/auth/login        — Login
GET    /api/auth/logout       — Logout
GET    /api/site              — Site info (files, quota usage)
POST   /api/site/upload       — Upload archive
DELETE /api/site              — Delete all site files
GET    /api/admin/codes       — List invite codes (admin)
POST   /api/admin/codes       — Generate invite codes (admin)
DELETE /api/admin/codes/:code — Revoke invite code (admin)
```

## Credits

Designed and built by [@modanub](https://github.com/modanub). Claude Opus 4.6 was used as a coding assistant during implementation.

## License

MIT
