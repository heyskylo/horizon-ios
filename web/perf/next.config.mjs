// The perf app is self-hosted (docs/perf-platform.md §7.1) and moves to playhorizon.dev by copying
// `app/perf/` and `app/api/perf/` plus these env vars. Nothing here reaches another origin: the
// page reads a capture in the browser and only ever fetches its own wasm and its own API.
const development = process.env.NODE_ENV !== "production";

const csp = [
  "default-src 'self'",
  // The wasm needs `wasm-unsafe-eval`; Next's hydration scripts are inline, so development and a
  // nonce-less self-host need `unsafe-inline` for them too.
  `script-src 'self' 'wasm-unsafe-eval'${development ? " 'unsafe-inline' 'unsafe-eval'" : " 'unsafe-inline'"}`,
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data:",
  // §7.1: the page talks to its own origin only. A capture leaves the machine only through Share.
  "connect-src 'self'",
  "base-uri 'self'",
  "form-action 'self'",
  "frame-ancestors 'none'",
].join("; ");

/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // `next dev` would otherwise write a generated AGENTS.md into this folder, which would shadow the
  // repository's own AGENTS.md for anyone's agent working here.
  agentRules: false,
  // The deployable artifact is one folder (`.next/standalone`), which the perf-web workflow uploads.
  output: "standalone",
  async headers() {
    return [
      {
        source: "/perf/:path*",
        headers: [{ key: "Content-Security-Policy", value: csp }],
      },
    ];
  },
};

export default nextConfig;
