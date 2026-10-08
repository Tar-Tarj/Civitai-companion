# Security model

## Enforced boundaries

- API credentials are held by Rust and persisted only through Windows Credential Manager.
- Authenticated requests require HTTPS, the exact `civitai.com` host and an `/api/` path. Redirects are followed only to that same origin, with a three-hop limit.
- Requests use fixed timeouts and responses are rejected above 5 MiB.
- The WebView IPC surface is a fixed list of typed Tauri commands. Every application command requires an explicit capability permission; event capabilities expose only listen/unlisten for the single state-change event.
- There is no shell, process, generic filesystem, arbitrary HTTP, remote IPC or frontend opener capability.
- External links are parsed in Rust and limited to HTTPS on `civitai.com` or `civitai.red`.
- WebView navigation is limited to the local application origin and all new-window requests are denied.
- Remote strings are rendered as text. Remote images are fetched without credentials by Rust, accept only HTTPS on the exact `image.civitai.com` entry host and its exact `blobs-b2.civitai.com` delivery host, reject every other redirect, SVG/HTML and payloads above 1 MiB, then reach the WebView only as local Blob URLs.
- Release WebView developer tools are disabled. The CSP contains no inline/eval script allowances, remote script/style/font sources or frames.
- Errors crossing the IPC boundary are fixed, sanitized messages; the app does not install a logging or telemetry subsystem.

## Data handling

`state-v1.json` may contain account summary, Buzz values, notifications, synchronization metadata and settings. It never contains an API key, Authorization header, cookies or browser data.

Preference backups exclude account data, notifications, cached reports and credentials. Imports are strict, versioned, bounded to 1 MiB, reject unknown or secret-bearing fields and cannot change Start with Windows.

## Trust boundary

This design limits the impact of hostile UI/API data and reduces available system primitives. It cannot protect against an already-compromised Windows user account, administrator-level malware, memory inspection by a privileged process, or a compromised Civitai service returning valid but misleading account data.

Before any public distribution, add Windows code signing and a separate review of cryptographically signed updater infrastructure. The personal build deliberately contains no updater.
