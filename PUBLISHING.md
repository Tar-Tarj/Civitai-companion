# Publishing on GitHub

1. Create a public GitHub repository and upload the source files from this directory.
2. Do not commit `release-assets/` to the repository history. Keep it locally as the staging area for downloadable builds.
3. Create a GitHub Release with tag `v1.0.23`.
4. Upload the three binaries and `SHA256SUMS.txt` from `release-assets/1.0.23/` as release assets.
5. Copy `release-assets/1.0.23/RELEASE_NOTES.md` into the release description.

The repository deliberately contains no `.env`, API key, user state, credentials, build cache or telemetry data.

No software license is currently included. Choose and add a license before describing the project as open source or granting reuse rights.
