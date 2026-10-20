# Shellsort website

This is a dependency-free static product site. The deployable site lives in `public/`; the Pages workflow checks its local links and uploads only that directory.

## Preview locally

From the repository root, run:

```sh
python3 -m http.server 8000 --directory website/public
```

Open <http://127.0.0.1:8000/>. Stop the server with Ctrl+C.

To validate local asset links, in-page anchors, and compatibility with a GitHub Pages project path:

```sh
python3 website/check.py
```

The site uses relative links such as `./styles.css`, so assets resolve both at the local server root and under `https://a1nbuselvan.github.io/Shellsort/`.

## Publishing

The workflow at `.github/workflows/pages.yml` deploys only pushes to `main` that change `website/**` or that workflow. Before the first deployment, a repository administrator must configure GitHub Pages to use **GitHub Actions** as its build and deployment source. No deployment is performed by local preview or validation.

The download area intentionally remains “Coming soon” until versioned release assets and their archive checksums are verified and published. Homebrew instructions remain unavailable while the tap formula contains placeholders.
